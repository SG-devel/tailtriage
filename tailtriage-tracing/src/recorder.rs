use std::collections::BTreeMap;
use std::fmt;
use std::fmt::Write as _;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
#[cfg(feature = "tokio")]
use std::time::Duration;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tailtriage_core::{CaptureLimits, CaptureLimitsOverride, CaptureMode, LocalJsonSink, RunSink};
#[cfg(feature = "tokio")]
use tailtriage_core::{MemorySink, RuntimeSnapshot, Tailtriage};
#[cfg(feature = "tokio")]
use tailtriage_tokio::RuntimeSampler;

use tracing::field::{Field, Visit};
use tracing::{Id, Subscriber};
use tracing_subscriber::layer::{Context, Layer};

use crate::{
    ensure_persistable_run_with_warnings, run_from_span_records, FieldValue, ImportError,
    ImportOptions, ImportedRun, SpanRecord, TT_KIND,
};

/// In-memory recorder for completed tracing spans with `tt.*` fields.
#[derive(Debug, Clone)]
pub(crate) struct LiveRecorder {
    state: Arc<Mutex<RecorderState>>,
    options: ImportOptions,
    limits: RecorderLimits,
}
/// High-level tracing intake bridge for completed `tt.*` spans.
///
/// A session attaches to an existing `tracing_subscriber` registry via [`Self::layer`],
/// captures completed `tt.*` spans, and converts them into standard `tailtriage_core::Run`
/// artifacts through [`Self::snapshot_run`] or [`Self::shutdown`].
///
/// When configured, the session emits stable completed-span JSONL records in the
/// wrapper form `{"format":"tailtriage.tracing-span.v1","span":{...}}` from
/// retained original source spans and can optionally write a Run JSON file on shutdown.
/// The JSONL evidence preserves source span identity and fields represented by
/// [`SpanRecord`], but does not encode Run-only metadata, runtime/in-flight snapshots,
/// lifecycle warnings, truncation counters, or omitted-source diagnostics.
///
/// This API is intentionally a tracing intake bridge; it does not implement OTel/OTLP.
/// Tracing-only evidence does not fabricate runtime-pressure snapshots, and suspects
/// in resulting diagnosis reports remain triage leads rather than root-cause proof.
#[derive(Debug)]
pub struct TracingSession {
    recorder: LiveRecorder,
    completed_span_jsonl_path: Option<PathBuf>,
    run_json_path: Option<PathBuf>,
    #[cfg(feature = "tokio")]
    runtime_collector: Option<Arc<Tailtriage>>,
    #[cfg(feature = "tokio")]
    sampler: Option<RuntimeSampler>,
}
/// Builder for [`TracingSession`].
#[derive(Debug, Clone)]
pub struct TracingSessionBuilder {
    recorder_builder: LiveRecorderBuilder,
    completed_span_jsonl_path: Option<PathBuf>,
    run_json_path: Option<PathBuf>,
    #[cfg(feature = "tokio")]
    sampler_interval: Option<Duration>,
    #[cfg(feature = "tokio")]
    manual_runtime_snapshots: bool,
}

/// Internal builder for the live tracing recorder.
#[derive(Debug, Clone)]
pub(crate) struct LiveRecorderBuilder {
    options: ImportOptions,
    limits: RecorderLimits,
}

/// `tracing_subscriber` layer that feeds completed spans into a [`TracingSession`].
#[derive(Debug, Clone)]
pub struct TailtriageLayer {
    state: Arc<Mutex<RecorderState>>,
    limits: RecorderLimits,
}
/// Default maximum number of concurrently tracked open candidate spans.
pub const DEFAULT_MAX_OPEN_SPANS: usize = 8_192;
/// Default maximum number of closed raw completed candidate spans retained before conversion.
pub const DEFAULT_MAX_COMPLETED_CANDIDATE_SPANS: usize = 65_536;
/// Configurable in-memory limits for live tracing recorder retention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct RecorderLimits {
    /// Maximum number of concurrently tracked open candidate spans.
    pub max_open_spans: usize,
    /// Maximum number of closed raw completed candidate spans retained before semantic conversion.
    ///
    /// This is a live recorder memory cap for raw closed candidates. Request/stage/queue
    /// semantic retention remains controlled by [`CaptureMode`], [`CaptureLimits`], and
    /// [`CaptureLimitsOverride`] during conversion.
    pub max_completed_candidate_spans: usize,
}
impl Default for RecorderLimits {
    fn default() -> Self {
        Self {
            max_open_spans: DEFAULT_MAX_OPEN_SPANS,
            max_completed_candidate_spans: DEFAULT_MAX_COMPLETED_CANDIDATE_SPANS,
        }
    }
}

#[derive(Debug)]
struct RecorderState {
    start_instant: Instant,
    open: BTreeMap<u64, OpenSpan>,
    completed_requests: Vec<SpanRecord>,
    completed_stages: Vec<SpanRecord>,
    completed_queues: Vec<SpanRecord>,
    dropped_open_spans: u64,
    dropped_completed_request_candidates: u64,
    dropped_completed_child_candidates: u64,
    evicted_child_candidates_to_preserve_request: u64,
    closed_missing_kind_spans: u64,
    closed_unknown_kind_spans: u64,
    closed_malformed_kind_spans: u64,
    closed_kind_samples: Vec<ClosedKindIssueSample>,
    closed_incomplete_candidate_spans: u64,
    closed_incomplete_candidate_samples: Vec<ClosedKindIssueSample>,
}

#[derive(Debug)]
struct OpenSpan {
    id: Option<String>,
    parent_id: Option<String>,
    name: String,
    fields: BTreeMap<String, FieldValue>,
    started_at_unix_ms: u64,
    started_at_run_us: u64,
    started_instant: Instant,
    is_tt_candidate: bool,
}

impl Default for RecorderState {
    fn default() -> Self {
        Self {
            start_instant: Instant::now(),
            open: BTreeMap::new(),
            completed_requests: Vec::new(),
            completed_stages: Vec::new(),
            completed_queues: Vec::new(),
            dropped_open_spans: 0,
            dropped_completed_request_candidates: 0,
            dropped_completed_child_candidates: 0,
            evicted_child_candidates_to_preserve_request: 0,
            closed_missing_kind_spans: 0,
            closed_unknown_kind_spans: 0,
            closed_malformed_kind_spans: 0,
            closed_kind_samples: Vec::new(),
            closed_incomplete_candidate_spans: 0,
            closed_incomplete_candidate_samples: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
struct OpenSpanSample {
    name: String,
    span_id: Option<String>,
    tt_kind: Option<String>,
    tt_request_id: Option<String>,
}

#[derive(Debug, Clone)]
struct ClosedKindIssueSample {
    name: String,
    span_id: Option<String>,
    tt_request_id: Option<String>,
    tt_kind: Option<String>,
    reason: &'static str,
}

struct SnapshotStats {
    dropped_open_spans: u64,
    dropped_completed_request_candidates: u64,
    dropped_completed_child_candidates: u64,
    evicted_child_candidates_to_preserve_request: u64,
    open_candidate_count: u64,
    open_samples: Vec<OpenSpanSample>,
    closed_missing_kind_spans: u64,
    closed_unknown_kind_spans: u64,
    closed_malformed_kind_spans: u64,
    closed_kind_samples: Vec<ClosedKindIssueSample>,
    closed_incomplete_candidate_spans: u64,
    closed_incomplete_candidate_samples: Vec<ClosedKindIssueSample>,
}
fn lock_state(state: &Arc<Mutex<RecorderState>>) -> std::sync::MutexGuard<'_, RecorderState> {
    match state.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

impl LiveRecorder {
    /// Creates a builder with required service name metadata.
    pub fn builder(service_name: impl Into<String>) -> LiveRecorderBuilder {
        LiveRecorderBuilder {
            options: ImportOptions::new(service_name),
            limits: RecorderLimits::default(),
        }
    }

    /// Returns a cloneable layer that captures `on_new_span`, `on_record`, and `on_close`.
    #[must_use]
    pub fn layer(&self) -> TailtriageLayer {
        TailtriageLayer {
            state: Arc::clone(&self.state),
            limits: self.limits,
        }
    }

    /// Returns a non-consuming snapshot of currently completed spans as an imported run.
    ///
    /// Span completion is driven by span close/drop, not enter/exit.
    ///
    /// # Errors
    ///
    /// Returns [`ImportError`] when strict conversion fails.
    pub fn snapshot_run(&self) -> Result<ImportedRun, ImportError> {
        let (spans, stats) = {
            let state = lock_state(&self.state);
            let mut samples = Vec::new();
            let mut count = 0_u64;
            for open in state.open.values() {
                if open.is_tt_candidate {
                    count = count.saturating_add(1);
                    if samples.len() < 3 {
                        samples.push(OpenSpanSample {
                            name: open.name.clone(),
                            span_id: open.id.clone(),
                            tt_kind: scalar_field_string(open.fields.get(TT_KIND)),
                            tt_request_id: scalar_field_string(open.fields.get("tt.request_id")),
                        });
                    }
                }
            }
            (
                completed_span_records(&state),
                SnapshotStats {
                    dropped_open_spans: state.dropped_open_spans,
                    dropped_completed_request_candidates: state
                        .dropped_completed_request_candidates,
                    dropped_completed_child_candidates: state.dropped_completed_child_candidates,
                    evicted_child_candidates_to_preserve_request: state
                        .evicted_child_candidates_to_preserve_request,
                    open_candidate_count: count,
                    open_samples: samples,
                    closed_missing_kind_spans: state.closed_missing_kind_spans,
                    closed_unknown_kind_spans: state.closed_unknown_kind_spans,
                    closed_malformed_kind_spans: state.closed_malformed_kind_spans,
                    closed_kind_samples: state.closed_kind_samples.clone(),
                    closed_incomplete_candidate_spans: state.closed_incomplete_candidate_spans,
                    closed_incomplete_candidate_samples: state
                        .closed_incomplete_candidate_samples
                        .clone(),
                },
            )
        };
        imported_with_drop_warnings(spans, self.options.clone(), &stats, self.limits)
    }

    /// Consumes this recorder handle and returns a final imported run snapshot.
    ///
    /// Span completion is driven by span close/drop, not enter/exit.
    ///
    /// # Errors
    ///
    /// Returns [`ImportError`] when strict conversion fails.
    fn shutdown(self) -> Result<ImportedRun, ImportError> {
        self.snapshot_run()
    }
}

impl TracingSession {
    /// Creates a tracing intake session builder with required service metadata.
    ///
    /// Service startup should install `session.layer()` in the process-wide subscriber setup; use scoped defaults only for local/test-style usage.
    ///
    /// ```no_run
    /// use tailtriage_tracing::TracingSession;
    /// use tracing_subscriber::prelude::*;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let session = TracingSession::builder("checkout")
    ///     .completed_span_jsonl_path("completed-spans.jsonl")
    ///     .build()?;
    ///
    /// tracing_subscriber::registry().with(session.layer()).init();
    ///
    /// {
    ///     let _guard = tracing::info_span!(
    ///         "request",
    ///         tt.kind = "request",
    ///         tt.request_id = "r1",
    ///         tt.route = "/checkout"
    ///     )
    ///     .entered();
    ///     // measured work goes here
    /// } // the request span is closed before shutdown
    ///
    /// # futures_executor::block_on(async move {
    /// session.shutdown().await?;
    /// # Ok::<_, tailtriage_tracing::ImportError>(())
    /// # })?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn builder(service_name: impl Into<String>) -> TracingSessionBuilder {
        TracingSessionBuilder {
            recorder_builder: LiveRecorder::builder(service_name),
            completed_span_jsonl_path: None,
            run_json_path: None,
            #[cfg(feature = "tokio")]
            sampler_interval: None,
            #[cfg(feature = "tokio")]
            manual_runtime_snapshots: false,
        }
    }
    /// Returns a `tracing_subscriber` layer for this intake session.
    ///
    /// Add this layer beside your existing subscriber layers; this does not replace
    /// your tracing pipeline.
    #[must_use]
    pub fn layer(&self) -> TailtriageLayer {
        self.recorder.layer()
    }
    /// Returns a non-consuming imported snapshot of completed spans.
    ///
    /// # Errors
    ///
    /// Returns an error when strict conversion fails.
    pub fn snapshot_run(&self) -> Result<ImportedRun, ImportError> {
        let imported = self.recorder.snapshot_run()?;
        #[cfg(feature = "tokio")]
        {
            if let Some(runtime_collector) = &self.runtime_collector {
                let runtime = runtime_collector.snapshot();
                return Ok(crate::tokio::with_manual_sampler_warning(
                    crate::tokio::merge_runtime_data(imported, &runtime),
                    self.sampler.is_none(),
                ));
            }
        }
        Ok(imported)
    }
    /// Records one Tokio runtime snapshot directly into the session when runtime collection is enabled.
    ///
    /// # Errors
    ///
    /// Returns [`ImportError::InvalidConfiguration`] when neither
    /// [`TracingSessionBuilder::manual_runtime_snapshots`] nor
    /// [`TracingSessionBuilder::sampler_interval`] enabled runtime collection.
    #[cfg(feature = "tokio")]
    pub fn record_runtime_snapshot(&self, snapshot: RuntimeSnapshot) -> Result<(), ImportError> {
        let Some(runtime_collector) = &self.runtime_collector else {
            return Err(ImportError::InvalidConfiguration {
                option: "runtime_snapshots",
                reason: "runtime collection is not enabled; call manual_runtime_snapshots() or sampler_interval(...) before build".to_string(),
            });
        };
        runtime_collector.record_runtime_snapshot(snapshot);
        Ok(())
    }
    /// Finalizes intake and optionally writes configured output artifacts.
    ///
    /// Each configured file (`completed_span_jsonl_path` and `run_json_path`) is written
    /// independently through its own temp/rename path. When both are configured, shutdown
    /// is not an atomic multi-file transaction: if the later write fails, an earlier output
    /// may already exist as a finalized artifact.
    ///
    /// # Errors
    ///
    /// Returns an error when conversion fails or when configured output artifacts cannot be written.
    pub async fn shutdown(self) -> Result<ImportedRun, ImportError> {
        #[cfg(feature = "tokio")]
        let sampler_disabled = self.sampler.is_none();
        #[cfg(feature = "tokio")]
        let runtime_collector = self.runtime_collector.clone();
        #[cfg(feature = "tokio")]
        if let Some(sampler) = self.sampler {
            sampler.shutdown().await;
        }
        let imported = self.recorder.shutdown()?;
        #[cfg(feature = "tokio")]
        let imported = if let Some(runtime_collector) = runtime_collector {
            let runtime = runtime_collector.snapshot();
            crate::tokio::with_manual_sampler_warning(
                crate::tokio::merge_runtime_data(imported, &runtime),
                sampler_disabled,
            )
        } else {
            imported
        };
        let (run, warnings, retained_sources) = imported.into_internal_parts();
        if self.run_json_path.is_some() || self.completed_span_jsonl_path.is_some() {
            ensure_persistable_run_with_warnings(&run, &warnings)?;
        }
        if let Some(path) = &self.completed_span_jsonl_path {
            validate_completed_span_jsonl_retained_sources(&run, &retained_sources, path)?;
            write_completed_span_jsonl_from_retained_sources(&retained_sources, path)?;
        }
        if let Some(path) = self.run_json_path {
            create_output_parent_dir(&path, "create run json parent directory")?;
            LocalJsonSink::new(&path)
                .write(&run)
                .map_err(|err| ImportError::RunJsonWrite {
                    path: path.display().to_string(),
                    reason: err.to_string(),
                })?;
        }
        Ok(ImportedRun::with_retained_sources(
            run,
            warnings,
            retained_sources,
        ))
    }
}

fn validate_completed_span_jsonl_retained_sources(
    run: &tailtriage_core::Run,
    retained_sources: &[SpanRecord],
    path: &Path,
) -> Result<(), ImportError> {
    if retained_sources.is_empty() && !run.requests.is_empty() {
        return Err(ImportError::Io {
            operation: "prepare completed span jsonl retained sources",
            context: path.display().to_string(),
            reason: "internal invariant violation: completed-span JSONL output requires retained original source spans".to_string(),
        });
    }

    Ok(())
}

fn write_completed_span_jsonl_from_retained_sources(
    retained_sources: &[SpanRecord],
    path: &Path,
) -> Result<(), ImportError> {
    create_output_parent_dir(path, "create completed span jsonl parent directory")?;
    let temp_path = completed_span_jsonl_temp_path(path);
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&temp_path)
        .map_err(|err| ImportError::Io {
            operation: "open completed span jsonl path",
            context: temp_path.display().to_string(),
            reason: err.to_string(),
        })?;

    let write_result = (|| -> Result<(), ImportError> {
        for span in retained_sources {
            let wrapped =
                serde_json::json!({ "format": "tailtriage.tracing-span.v1", "span": span });

            serde_json::to_writer(&mut file, &wrapped).map_err(|err| ImportError::Io {
                operation: "write completed span jsonl record",
                context: temp_path.display().to_string(),
                reason: err.to_string(),
            })?;

            file.write_all(b"\n").map_err(|err| ImportError::Io {
                operation: "write completed span jsonl newline",
                context: temp_path.display().to_string(),
                reason: err.to_string(),
            })?;
        }

        file.flush().map_err(|err| ImportError::Io {
            operation: "flush completed span jsonl file",
            context: temp_path.display().to_string(),
            reason: err.to_string(),
        })?;

        Ok(())
    })();

    drop(file);

    if let Err(err) = write_result {
        let _ = std::fs::remove_file(&temp_path);
        return Err(err);
    }

    std::fs::rename(&temp_path, path).map_err(|err| {
        let _ = std::fs::remove_file(&temp_path);
        ImportError::Io {
            operation: "rename completed span jsonl temp file",
            context: format!("{} -> {}", temp_path.display(), path.display()),
            reason: err.to_string(),
        }
    })?;

    Ok(())
}

fn create_output_parent_dir(path: &Path, operation: &'static str) -> Result<(), ImportError> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    if parent.as_os_str().is_empty() {
        return Ok(());
    }
    std::fs::create_dir_all(parent).map_err(|err| ImportError::Io {
        operation,
        context: parent.display().to_string(),
        reason: err.to_string(),
    })
}

fn completed_span_jsonl_temp_path(path: &Path) -> PathBuf {
    let file_name = path.file_name().map_or_else(
        || "completed-spans.jsonl".to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    let nanos_since_unix_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let temp_name = format!(
        ".{file_name}.tailtriage-tmp-{}-{nanos_since_unix_epoch}",
        std::process::id(),
    );
    path.with_file_name(temp_name)
}

impl TracingSessionBuilder {
    /// Enables or disables strict mode for conversion warnings.
    #[must_use]
    pub fn strict(mut self, strict: bool) -> Self {
        self.recorder_builder = self.recorder_builder.strict(strict);
        self
    }
    /// Sets capture mode used to resolve live completed-evidence retention limits.
    #[must_use]
    pub fn mode(mut self, mode: CaptureMode) -> Self {
        self.recorder_builder = self.recorder_builder.mode(mode);
        self
    }
    /// Sets base capture limits used for live completed-evidence retention.
    #[must_use]
    pub fn capture_limits(mut self, capture_limits: CaptureLimits) -> Self {
        self.recorder_builder = self.recorder_builder.capture_limits(capture_limits);
        self
    }
    /// Sets capture-limit overrides applied on top of the selected capture mode.
    #[must_use]
    pub fn capture_limits_override(mut self, override_limits: CaptureLimitsOverride) -> Self {
        self.recorder_builder = self
            .recorder_builder
            .capture_limits_override(override_limits);
        self
    }
    /// Sets service version metadata for converted run output.
    #[must_use]
    pub fn service_version(mut self, service_version: impl Into<String>) -> Self {
        self.recorder_builder = self.recorder_builder.service_version(service_version);
        self
    }
    /// Sets explicit run id metadata for converted run output.
    #[must_use]
    pub fn run_id(mut self, run_id: impl Into<String>) -> Self {
        self.recorder_builder = self.recorder_builder.run_id(run_id);
        self
    }
    /// Sets live recorder memory limits.
    #[must_use]
    pub fn limits(mut self, limits: RecorderLimits) -> Self {
        self.recorder_builder = self.recorder_builder.limits(limits);
        self
    }
    /// Sets maximum concurrently tracked open candidate spans.
    #[must_use]
    pub fn max_open_spans(mut self, v: usize) -> Self {
        self.recorder_builder = self.recorder_builder.max_open_spans(v);
        self
    }
    /// Sets maximum retained closed raw completed candidate spans before semantic conversion.
    ///
    /// This is a live recorder memory cap. Request/stage/queue semantic retention remains
    /// controlled by [`CaptureMode`], [`CaptureLimits`], and [`CaptureLimitsOverride`].
    #[must_use]
    pub fn max_completed_candidate_spans(mut self, v: usize) -> Self {
        self.recorder_builder = self.recorder_builder.max_completed_candidate_spans(v);
        self
    }
    /// Enables completed-span JSONL output at the given path.
    ///
    /// Writes retained original [`SpanRecord`] values as stable span-shaped JSONL on shutdown.
    /// Where present, those retained records preserve span name, span ID, parent ID,
    /// `tt.*` fields, non-`tt.*` fields, Unix-ms bounds, optional run-relative offsets,
    /// and optional explicit duration.
    ///
    /// Excluded, semantically dropped, and raw-unavailable records are absent.
    /// Completed-span JSONL does not encode Run-only metadata, runtime snapshots,
    /// lifecycle warnings, drop counters, or omitted-source diagnostics.
    ///
    /// When both output paths are configured, this file is finalized independently and may
    /// exist even if the later run-json write fails.
    #[must_use]
    pub fn completed_span_jsonl_path(mut self, path: impl AsRef<Path>) -> Self {
        self.completed_span_jsonl_path = Some(path.as_ref().to_path_buf());
        self
    }
    /// Enables Run JSON output on shutdown at the given path.
    ///
    /// When both output paths are configured, this file is written independently from
    /// completed-span JSONL; shutdown does not commit both files as one atomic transaction.
    #[must_use]
    pub fn run_json_path(mut self, path: impl AsRef<Path>) -> Self {
        self.run_json_path = Some(path.as_ref().to_path_buf());
        self
    }
    /// Builds a tracing intake session.
    ///
    /// # Errors
    ///
    /// Returns an error when required recorder configuration is invalid,
    /// including blank/whitespace service metadata.
    pub fn build(self) -> Result<TracingSession, ImportError> {
        let recorder = self.recorder_builder.clone().build()?;
        #[cfg(feature = "tokio")]
        let runtime_collector = self.build_runtime_collector()?;
        #[cfg(feature = "tokio")]
        let sampler = self.build_runtime_sampler(runtime_collector.as_ref())?;
        Ok(TracingSession {
            recorder,
            completed_span_jsonl_path: self.completed_span_jsonl_path,
            run_json_path: self.run_json_path,
            #[cfg(feature = "tokio")]
            runtime_collector,
            #[cfg(feature = "tokio")]
            sampler,
        })
    }

    /// Enables background Tokio runtime sampling at the given interval.
    #[cfg(feature = "tokio")]
    #[must_use]
    pub fn sampler_interval(mut self, interval: Duration) -> Self {
        self.sampler_interval = Some(interval);
        self
    }

    /// Enables manual Tokio runtime snapshot collection without starting the background sampler.
    #[cfg(feature = "tokio")]
    #[must_use]
    pub fn manual_runtime_snapshots(mut self) -> Self {
        self.manual_runtime_snapshots = true;
        self
    }

    #[cfg(feature = "tokio")]
    fn build_runtime_collector(&self) -> Result<Option<Arc<Tailtriage>>, ImportError> {
        if self.sampler_interval.is_none() && !self.manual_runtime_snapshots {
            return Ok(None);
        }
        let mode = self.recorder_builder.selected_mode();
        let resolved_limits = self.recorder_builder.resolved_capture_limits();
        let sink = MemorySink::new();
        let builder = Tailtriage::builder("tailtriage-tracing-runtime")
            .sink(sink)
            .strict_lifecycle(false)
            .capture_limits(resolved_limits);
        let builder = match mode {
            CaptureMode::Light => builder.light(),
            CaptureMode::Investigation => builder.investigation(),
        };
        let collector = builder.build().map_err(|err| ImportError::Io {
            operation: "build tracing Tokio runtime collector",
            context: "tailtriage-tracing-runtime".to_string(),
            reason: err.to_string(),
        })?;
        Ok(Some(Arc::new(collector)))
    }

    #[cfg(feature = "tokio")]
    fn build_runtime_sampler(
        &self,
        runtime_collector: Option<&Arc<Tailtriage>>,
    ) -> Result<Option<RuntimeSampler>, ImportError> {
        let Some(runtime_collector) = runtime_collector else {
            return Ok(None);
        };
        if self.sampler_interval.is_none() {
            return Ok(None);
        }
        if let Some(interval) = self.sampler_interval {
            if interval.is_zero() {
                return Err(ImportError::InvalidConfiguration {
                    option: "sampler_interval",
                    reason: "sampler interval must be greater than zero".to_string(),
                });
            }
        }
        let mut sampler_builder = RuntimeSampler::builder(Arc::clone(runtime_collector));
        if let Some(interval) = self.sampler_interval {
            sampler_builder = sampler_builder.interval(interval);
        }
        let resolved_limits = self.recorder_builder.resolved_capture_limits();
        sampler_builder =
            sampler_builder.max_runtime_snapshots(resolved_limits.max_runtime_snapshots);
        sampler_builder
            .start()
            .map(Some)
            .map_err(|err| ImportError::Io {
                operation: "start tracing Tokio runtime sampler",
                context: "active Tokio runtime".to_string(),
                reason: err.to_string(),
            })
    }
}

impl LiveRecorderBuilder {
    /// Returns selected capture mode for import conversion semantics.
    #[cfg(feature = "tokio")]
    #[must_use]
    pub(crate) fn selected_mode(&self) -> CaptureMode {
        self.options.mode_value()
    }

    /// Returns capture limits resolved from configured mode/base/override settings.
    #[cfg(feature = "tokio")]
    #[must_use]
    pub(crate) fn resolved_capture_limits(&self) -> CaptureLimits {
        self.options.resolved_capture_limits()
    }

    /// Sets service version metadata for converted run output.
    #[must_use]
    pub fn service_version(mut self, service_version: impl Into<String>) -> Self {
        self.options = self.options.service_version(service_version);
        self
    }

    /// Sets explicit run id metadata for converted run output.
    #[must_use]
    pub fn run_id(mut self, run_id: impl Into<String>) -> Self {
        self.options = self.options.run_id(run_id);
        self
    }

    /// Enables or disables strict mode for conversion warnings.
    #[must_use]
    pub fn strict(mut self, strict: bool) -> Self {
        self.options = self.options.strict(strict);
        self
    }
    /// Sets capture mode used to resolve live completed-evidence retention limits.
    #[must_use]
    pub fn mode(mut self, mode: CaptureMode) -> Self {
        self.options = self.options.mode(mode);
        self
    }
    /// Sets base capture limits used for live completed-evidence retention.
    #[must_use]
    pub fn capture_limits(mut self, capture_limits: CaptureLimits) -> Self {
        self.options = self.options.capture_limits(capture_limits);
        self
    }
    /// Sets capture-limit overrides applied on top of the selected capture mode.
    #[must_use]
    pub fn capture_limits_override(mut self, override_limits: CaptureLimitsOverride) -> Self {
        self.options = self.options.capture_limits_override(override_limits);
        self
    }

    /// Builds a recorder instance.
    ///
    /// # Errors
    ///
    /// Returns [`ImportError::EmptyServiceName`] when the configured service name
    /// is blank or whitespace-only.
    pub fn build(self) -> Result<LiveRecorder, ImportError> {
        if self.options.service_name().trim().is_empty() {
            return Err(ImportError::EmptyServiceName);
        }
        Ok(LiveRecorder {
            state: Arc::new(Mutex::new(RecorderState::default())),
            options: self.options,
            limits: self.limits,
        })
    }
    /// Sets live recorder memory limits.
    #[must_use]
    pub fn limits(mut self, limits: RecorderLimits) -> Self {
        self.limits = limits;
        self
    }
    /// Sets maximum number of concurrently tracked candidate open spans.
    #[must_use]
    pub fn max_open_spans(mut self, max_open_spans: usize) -> Self {
        self.limits.max_open_spans = max_open_spans;
        self
    }
    /// Sets maximum retained closed raw completed candidate spans before semantic conversion.
    ///
    /// This is a live recorder memory cap. Request/stage/queue semantic retention remains
    /// controlled by [`CaptureMode`], [`CaptureLimits`], and [`CaptureLimitsOverride`].
    #[must_use]
    pub fn max_completed_candidate_spans(mut self, max_completed_candidate_spans: usize) -> Self {
        self.limits.max_completed_candidate_spans = max_completed_candidate_spans;
        self
    }
}

impl<S> Layer<S> for TailtriageLayer
where
    S: Subscriber,
{
    fn on_new_span(&self, attrs: &tracing::span::Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let mut visitor = FieldVisitor::default();
        attrs.record(&mut visitor);
        let parent_id = attrs
            .parent()
            .map(|pid| pid.into_u64().to_string())
            .or_else(|| {
                ctx.current_span()
                    .id()
                    .map(|pid| pid.into_u64().to_string())
            });
        let metadata_candidate = metadata_has_tailtriage_field(attrs.metadata());
        let initial_candidate = fields_have_tailtriage_key(&visitor.fields);
        if !(metadata_candidate || initial_candidate) {
            return;
        }
        let started_at_unix_ms = tailtriage_core::unix_time_ms();
        let started_instant = Instant::now();

        let mut state = lock_state(&self.state);
        if state.open.len() >= self.limits.max_open_spans {
            state.dropped_open_spans = state.dropped_open_spans.saturating_add(1);
            return;
        }
        let open_span = open_span_from_start_sample(
            Some(id.into_u64().to_string()),
            parent_id,
            attrs.metadata().name().to_owned(),
            visitor.fields,
            started_at_unix_ms,
            started_instant,
            state.start_instant,
            metadata_candidate || initial_candidate,
        );
        state.open.insert(id.into_u64(), open_span);
    }

    fn on_record(&self, span_id: &Id, values: &tracing::span::Record<'_>, _ctx: Context<'_, S>) {
        let mut visitor = FieldVisitor::default();
        values.record(&mut visitor);
        let mut state = lock_state(&self.state);
        if let Some(span) = state.open.get_mut(&span_id.into_u64()) {
            span.fields.extend(visitor.fields);
        }
    }

    fn on_close(&self, id: Id, _ctx: Context<'_, S>) {
        let closed_at_unix_ms = tailtriage_core::unix_time_ms();
        let closed_instant = std::time::Instant::now();

        let mut state = lock_state(&self.state);
        let finished_at_run_us = duration_us_between(state.start_instant, closed_instant);
        if let Some(open) = state.open.remove(&id.into_u64()) {
            let kind = classify_kind(&open.fields);
            if let Err(reason) = kind {
                record_invalid_kind_issue(&mut state, &open, reason);
                return;
            }
            let kind = kind.expect("validated above");
            if let Some(reason) = precheck_required_fields(kind, &open.fields) {
                record_incomplete_candidate_issue(&mut state, &open, kind, reason);
                return;
            }
            let duration_us = duration_us_between(open.started_instant, closed_instant);
            let mut record = SpanRecord::new(open.name, open.started_at_unix_ms, closed_at_unix_ms)
                .started_at_run_us(open.started_at_run_us)
                .finished_at_run_us(finished_at_run_us)
                .duration_us(duration_us);
            if let Some(span_id) = open.id {
                record = record.id(span_id);
            }
            if let Some(parent_id) = open.parent_id {
                record = record.parent_id(parent_id);
            }
            for (k, v) in open.fields {
                record = record.field(k, v);
            }
            push_completed_candidate_with_kind_aware_retention(
                &mut state,
                record,
                kind,
                self.limits,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn open_span_from_start_sample(
    id: Option<String>,
    parent_id: Option<String>,
    name: String,
    fields: BTreeMap<String, FieldValue>,
    started_at_unix_ms: u64,
    started_instant: Instant,
    recorder_start_instant: Instant,
    is_tt_candidate: bool,
) -> OpenSpan {
    OpenSpan {
        id,
        parent_id,
        name,
        fields,
        started_at_unix_ms,
        started_at_run_us: duration_us_between(recorder_start_instant, started_instant),
        started_instant,
        is_tt_candidate,
    }
}

fn duration_us_between(
    started_instant: std::time::Instant,
    closed_instant: std::time::Instant,
) -> u64 {
    u64::try_from(closed_instant.duration_since(started_instant).as_micros()).unwrap_or(u64::MAX)
}

fn completed_span_records(state: &RecorderState) -> Vec<SpanRecord> {
    let mut spans = Vec::with_capacity(
        state.completed_requests.len()
            + state.completed_stages.len()
            + state.completed_queues.len(),
    );
    spans.extend(state.completed_requests.iter().cloned());
    spans.extend(state.completed_stages.iter().cloned());
    spans.extend(state.completed_queues.iter().cloned());
    spans
}

fn push_completed_candidate_with_kind_aware_retention(
    state: &mut RecorderState,
    record: SpanRecord,
    kind: &str,
    limits: RecorderLimits,
) {
    let total = state.completed_requests.len()
        + state.completed_stages.len()
        + state.completed_queues.len();
    if kind != "request" {
        if total < limits.max_completed_candidate_spans {
            push_record_by_kind(state, record, kind);
        } else if matches!(kind, "stage" | "queue") {
            state.dropped_completed_child_candidates =
                state.dropped_completed_child_candidates.saturating_add(1);
        }
        return;
    }

    if total < limits.max_completed_candidate_spans {
        state.completed_requests.push(record);
        return;
    }

    if !state.completed_queues.is_empty() {
        state.completed_queues.pop();
        state.evicted_child_candidates_to_preserve_request = state
            .evicted_child_candidates_to_preserve_request
            .saturating_add(1);
    } else if !state.completed_stages.is_empty() {
        state.completed_stages.pop();
        state.evicted_child_candidates_to_preserve_request = state
            .evicted_child_candidates_to_preserve_request
            .saturating_add(1);
    } else {
        state.dropped_completed_request_candidates =
            state.dropped_completed_request_candidates.saturating_add(1);
        return;
    }

    state.completed_requests.push(record);
}

fn push_record_by_kind(state: &mut RecorderState, record: SpanRecord, kind: &str) {
    match kind {
        "request" => state.completed_requests.push(record),
        "stage" => state.completed_stages.push(record),
        "queue" => state.completed_queues.push(record),
        _ => {}
    }
}
fn precheck_required_fields(
    kind: &str,
    fields: &BTreeMap<String, FieldValue>,
) -> Option<&'static str> {
    match kind {
        "request" => require_string_field(fields, "tt.request_id")
            .or_else(|| require_string_field(fields, "tt.route")),
        "stage" => require_string_field(fields, "tt.request_id")
            .or_else(|| require_string_field(fields, "tt.stage")),
        "queue" => require_string_field(fields, "tt.request_id")
            .or_else(|| require_string_field(fields, "tt.queue")),
        _ => None,
    }
}

fn require_string_field(
    fields: &BTreeMap<String, FieldValue>,
    field_name: &'static str,
) -> Option<&'static str> {
    match fields.get(field_name) {
        None => Some(match field_name {
            "tt.request_id" => "missing required field tt.request_id",
            "tt.route" => "missing required field tt.route",
            "tt.stage" => "missing required field tt.stage",
            "tt.queue" => "missing required field tt.queue",
            _ => "missing required field",
        }),
        Some(FieldValue::String(value)) => {
            if value.trim().is_empty() {
                Some(match field_name {
                    "tt.request_id" => {
                        "invalid required field tt.request_id: required string cannot be empty or whitespace"
                    }
                    "tt.route" => {
                        "invalid required field tt.route: required string cannot be empty or whitespace"
                    }
                    "tt.stage" => {
                        "invalid required field tt.stage: required string cannot be empty or whitespace"
                    }
                    "tt.queue" => {
                        "invalid required field tt.queue: required string cannot be empty or whitespace"
                    }
                    _ => "invalid required field: required string cannot be empty or whitespace",
                })
            } else {
                None
            }
        }
        Some(_) => Some(match field_name {
            "tt.request_id" => "invalid required field tt.request_id: expected string",
            "tt.route" => "invalid required field tt.route: expected string",
            "tt.stage" => "invalid required field tt.stage: expected string",
            "tt.queue" => "invalid required field tt.queue: expected string",
            _ => "invalid required field: expected string",
        }),
    }
}

fn record_incomplete_candidate_issue(
    state: &mut RecorderState,
    open: &OpenSpan,
    kind: &'static str,
    reason: &'static str,
) {
    state.closed_incomplete_candidate_spans =
        state.closed_incomplete_candidate_spans.saturating_add(1);
    if state.closed_incomplete_candidate_samples.len() < 16 {
        state
            .closed_incomplete_candidate_samples
            .push(ClosedKindIssueSample {
                name: open.name.clone(),
                span_id: open.id.clone(),
                tt_request_id: match open.fields.get("tt.request_id") {
                    Some(FieldValue::String(v)) => Some(v.clone()),
                    _ => None,
                },
                tt_kind: Some(kind.to_owned()),
                reason,
            });
    }
}
fn record_invalid_kind_issue(state: &mut RecorderState, open: &OpenSpan, reason: &'static str) {
    if !open.is_tt_candidate {
        return;
    }
    match reason {
        "missing" => {
            state.closed_missing_kind_spans = state.closed_missing_kind_spans.saturating_add(1);
        }
        "unknown" => {
            state.closed_unknown_kind_spans = state.closed_unknown_kind_spans.saturating_add(1);
        }
        "malformed" => {
            state.closed_malformed_kind_spans = state.closed_malformed_kind_spans.saturating_add(1);
        }
        _ => {}
    }
    if state.closed_kind_samples.len() < 16 {
        state.closed_kind_samples.push(ClosedKindIssueSample {
            name: open.name.clone(),
            span_id: open.id.clone(),
            tt_request_id: scalar_field_string(open.fields.get("tt.request_id")),
            tt_kind: scalar_field_string(open.fields.get(TT_KIND)),
            reason,
        });
    }
}

fn classify_kind(fields: &BTreeMap<String, FieldValue>) -> Result<&'static str, &'static str> {
    match fields.get(TT_KIND) {
        None => Err("missing"),
        Some(FieldValue::String(v)) if v == "request" => Ok("request"),
        Some(FieldValue::String(v)) if v == "stage" => Ok("stage"),
        Some(FieldValue::String(v)) if v == "queue" => Ok("queue"),
        Some(FieldValue::String(_)) => Err("unknown"),
        Some(_) => Err("malformed"),
    }
}
fn metadata_has_tailtriage_field(metadata: &tracing::Metadata<'_>) -> bool {
    metadata
        .fields()
        .iter()
        .any(|f| f.name().starts_with("tt."))
}

fn fields_have_tailtriage_key(fields: &BTreeMap<String, FieldValue>) -> bool {
    fields.keys().any(|k| k.starts_with("tt."))
}

fn push_strict_recorder_messages(
    messages: &mut Vec<String>,
    stats: &SnapshotStats,
    limits: RecorderLimits,
) {
    if stats.open_candidate_count > 0 {
        messages.push(format!(
            "live recorder observed {} open candidate span(s) at snapshot/shutdown; incomplete spans are not converted into fabricated completions",
            stats.open_candidate_count
        ));
    }
    if stats.closed_missing_kind_spans > 0 {
        messages.push(format!(
            "live recorder closed {} candidate span(s) missing tt.kind; closed candidate spans without tt.kind are not converted",
            stats.closed_missing_kind_spans
        ));
    }
    if stats.closed_unknown_kind_spans > 0 {
        messages.push(format!(
            "live recorder closed {} candidate span(s) with unknown tt.kind; closed candidate spans with unknown tt.kind are not converted",
            stats.closed_unknown_kind_spans
        ));
    }
    if stats.closed_malformed_kind_spans > 0 {
        messages.push(format!(
            "live recorder closed {} candidate span(s) with malformed tt.kind; closed candidate spans with malformed tt.kind are not converted",
            stats.closed_malformed_kind_spans
        ));
    }
    if stats.closed_incomplete_candidate_spans > 0 {
        messages.push(format!(
            "live recorder closed {} candidate span(s) with incomplete required fields for tt.kind=request|stage|queue; these spans are not converted",
            stats.closed_incomplete_candidate_spans
        ));
    }
    if stats.dropped_open_spans > 0 {
        messages.push(format!(
            "live recorder dropped {} open candidate span(s) because max_open_spans={} was reached; raise max_open_spans or reduce capture scope",
            stats.dropped_open_spans, limits.max_open_spans
        ));
    }
    if stats.dropped_completed_request_candidates > 0 {
        messages.push(format!(
            "live recorder dropped {} completed request candidate span(s) because max_completed_candidate_spans={} was reached and no child candidate was available to evict",
            stats.dropped_completed_request_candidates, limits.max_completed_candidate_spans
        ));
    }
    if stats.dropped_completed_child_candidates > 0 {
        messages.push(format!(
            "live recorder dropped {} completed child candidate span(s) because max_completed_candidate_spans={} was reached while preserving request roots",
            stats.dropped_completed_child_candidates, limits.max_completed_candidate_spans
        ));
    }
    if stats.evicted_child_candidates_to_preserve_request > 0 {
        messages.push(format!(
            "live recorder evicted {} completed child candidate span(s) to preserve completed request candidate span(s) under max_completed_candidate_spans={}",
            stats.evicted_child_candidates_to_preserve_request, limits.max_completed_candidate_spans
        ));
    }
}

fn append_non_strict_drop_warnings(
    run: &mut tailtriage_core::Run,
    warnings: &mut Vec<crate::ImportWarning>,
    stats: &SnapshotStats,
    limits: RecorderLimits,
) {
    if stats.open_candidate_count > 0 {
        let mut msg = format!(
            "live recorder observed {} open candidate span(s) at snapshot/shutdown; incomplete spans are not converted into fabricated completions",
            stats.open_candidate_count
        );
        if !stats.open_samples.is_empty() {
            let sample_text = stats
                .open_samples
                .iter()
                .map(|sample| {
                    format!(
                        "name={}, id={}, tt.kind={}, tt.request_id={}",
                        sample.name,
                        sample.span_id.as_deref().unwrap_or("-"),
                        sample.tt_kind.as_deref().unwrap_or("-"),
                        sample.tt_request_id.as_deref().unwrap_or("-")
                    )
                })
                .collect::<Vec<_>>()
                .join("; ");
            let _ = write!(&mut msg, "; samples: {sample_text}");
        }
        run.metadata.lifecycle_warnings.push(msg.clone());
        warnings.push(crate::ImportWarning::new(msg));
    }

    let closed_invalid_kind_total = stats.closed_missing_kind_spans
        + stats.closed_unknown_kind_spans
        + stats.closed_malformed_kind_spans;
    if closed_invalid_kind_total > 0 {
        let mut msg = format!(
            "live recorder closed candidate spans with invalid tt.kind (missing={}, unknown={}, malformed={}); these spans are not converted",
            stats.closed_missing_kind_spans,
            stats.closed_unknown_kind_spans,
            stats.closed_malformed_kind_spans
        );
        if !stats.closed_kind_samples.is_empty() {
            let sample_text = stats
                .closed_kind_samples
                .iter()
                .map(|sample| {
                    format!(
                        "reason={}, name={}, id={}, tt.kind={}, tt.request_id={}",
                        sample.reason,
                        sample.name,
                        sample.span_id.as_deref().unwrap_or("-"),
                        sample.tt_kind.as_deref().unwrap_or("-"),
                        sample.tt_request_id.as_deref().unwrap_or("-")
                    )
                })
                .collect::<Vec<_>>()
                .join("; ");
            let _ = write!(&mut msg, "; samples: {sample_text}");
        }
        run.metadata.lifecycle_warnings.push(msg.clone());
        warnings.push(crate::ImportWarning::new(msg));
    }
    if stats.closed_incomplete_candidate_spans > 0 {
        let msg = format_incomplete_closed_candidate_message(stats);
        run.metadata.lifecycle_warnings.push(msg.clone());
        warnings.push(crate::ImportWarning::new(msg));
    }
    if stats.dropped_open_spans > 0 {
        let msg = format!(
            "live recorder dropped {} candidate spans because max_open_spans was reached",
            stats.dropped_open_spans
        );
        run.metadata.lifecycle_warnings.push(msg.clone());
        warnings.push(crate::ImportWarning::new(msg));
    }
    if stats.dropped_completed_request_candidates > 0 {
        let msg = format!(
            "live recorder dropped {} completed request candidate span(s) because max_completed_candidate_spans={} was reached and no child candidate was available to evict",
            stats.dropped_completed_request_candidates, limits.max_completed_candidate_spans
        );
        run.metadata.lifecycle_warnings.push(msg.clone());
        warnings.push(crate::ImportWarning::new(msg));
    }
    if stats.dropped_completed_child_candidates > 0 {
        let msg = format!(
            "live recorder dropped {} completed child candidate span(s) because max_completed_candidate_spans={} was reached while preserving request roots",
            stats.dropped_completed_child_candidates, limits.max_completed_candidate_spans
        );
        run.metadata.lifecycle_warnings.push(msg.clone());
        warnings.push(crate::ImportWarning::new(msg));
    }
    if stats.evicted_child_candidates_to_preserve_request > 0 {
        let msg = format!(
            "live recorder evicted {} completed child candidate span(s) to preserve completed request candidate span(s) under max_completed_candidate_spans={}",
            stats.evicted_child_candidates_to_preserve_request, limits.max_completed_candidate_spans
        );
        run.metadata.lifecycle_warnings.push(msg.clone());
        warnings.push(crate::ImportWarning::new(msg));
    }
    if stats.dropped_open_spans > 0
        || stats.dropped_completed_request_candidates > 0
        || stats.dropped_completed_child_candidates > 0
        || stats.evicted_child_candidates_to_preserve_request > 0
    {
        run.truncation.limits_hit = true;
    }
}

fn format_incomplete_closed_candidate_message(stats: &SnapshotStats) -> String {
    let mut msg = format!(
        "live recorder closed {} candidate span(s) with incomplete required fields for tt.kind=request|stage|queue; these spans are not converted",
        stats.closed_incomplete_candidate_spans
    );
    if !stats.closed_incomplete_candidate_samples.is_empty() {
        let sample_text = stats
            .closed_incomplete_candidate_samples
            .iter()
            .map(|sample| {
                format!(
                    "reason={}, name={}, id={}, tt.kind={}, tt.request_id={}",
                    sample.reason,
                    sample.name,
                    sample.span_id.as_deref().unwrap_or("-"),
                    sample.tt_kind.as_deref().unwrap_or("-"),
                    sample.tt_request_id.as_deref().unwrap_or("-")
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        let _ = write!(&mut msg, "; samples: {sample_text}");
    }
    msg
}

fn imported_with_drop_warnings(
    spans: Vec<SpanRecord>,
    options: ImportOptions,
    stats: &SnapshotStats,
    limits: RecorderLimits,
) -> Result<ImportedRun, ImportError> {
    let mut strict_messages = Vec::new();
    if options.strict_mode() {
        push_strict_recorder_messages(&mut strict_messages, stats, limits);
    }

    let imported = match run_from_span_records(spans, options) {
        Ok(imported) => imported,
        Err(ImportError::StrictViolation(message)) if !strict_messages.is_empty() => {
            strict_messages.push(message);
            return Err(ImportError::StrictViolation(strict_messages.join("; ")));
        }
        Err(err) => return Err(err),
    };

    if !strict_messages.is_empty() {
        return Err(ImportError::StrictViolation(strict_messages.join("; ")));
    }

    if stats.dropped_open_spans == 0
        && stats.dropped_completed_request_candidates == 0
        && stats.dropped_completed_child_candidates == 0
        && stats.evicted_child_candidates_to_preserve_request == 0
        && stats.open_candidate_count == 0
        && stats.closed_missing_kind_spans == 0
        && stats.closed_unknown_kind_spans == 0
        && stats.closed_malformed_kind_spans == 0
        && stats.closed_incomplete_candidate_spans == 0
    {
        return Ok(imported);
    }

    let (mut run, mut warnings, retained_sources) = imported.into_internal_parts();
    append_non_strict_drop_warnings(&mut run, &mut warnings, stats, limits);
    Ok(ImportedRun::with_retained_sources(
        run,
        warnings,
        retained_sources,
    ))
}

fn scalar_field_string(value: Option<&FieldValue>) -> Option<String> {
    match value {
        Some(FieldValue::String(v)) => Some(v.clone()),
        Some(FieldValue::Bool(v)) => Some(v.to_string()),
        Some(FieldValue::U64(v)) => Some(v.to_string()),
        Some(FieldValue::I64(v)) => Some(v.to_string()),
        Some(FieldValue::F64(v)) => Some(v.to_string()),
        Some(FieldValue::Null) | None => None,
    }
}

#[derive(Default)]
struct FieldVisitor {
    fields: BTreeMap<String, FieldValue>,
}

impl Visit for FieldVisitor {
    fn record_bool(&mut self, field: &Field, value: bool) {
        self.fields
            .insert(field.name().to_owned(), FieldValue::Bool(value));
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.fields
            .insert(field.name().to_owned(), FieldValue::I64(value));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.fields
            .insert(field.name().to_owned(), FieldValue::U64(value));
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        self.fields
            .insert(field.name().to_owned(), FieldValue::F64(value));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.fields.insert(
            field.name().to_owned(),
            FieldValue::String(value.to_owned()),
        );
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.fields.insert(
            field.name().to_owned(),
            FieldValue::String(format!("{value:?}")),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tailtriage_analyzer::{analyze_run, AnalyzeOptions};
    use tracing_subscriber::prelude::*;

    fn with_recorder<T>(f: impl FnOnce(&LiveRecorder) -> T) -> T {
        let recorder = LiveRecorder::builder("svc").run_id("rid").build().unwrap();
        let subscriber = tracing_subscriber::registry().with(recorder.layer());
        tracing::subscriber::with_default(subscriber, || f(&recorder))
    }

    fn decode_completed_span_jsonl(path: &Path) -> Vec<SpanRecord> {
        std::fs::read_to_string(path)
            .unwrap()
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| {
                let value: serde_json::Value = serde_json::from_str(line).unwrap();
                assert_eq!(value["format"], "tailtriage.tracing-span.v1");
                serde_json::from_value(value["span"].clone()).unwrap()
            })
            .collect()
    }

    fn source_projection(spans: &[SpanRecord]) -> Vec<(String, String)> {
        spans
            .iter()
            .map(|span| {
                (
                    span.name().to_owned(),
                    span.id_ref().unwrap_or_default().to_owned(),
                )
            })
            .collect()
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct RepresentableEvidence {
        requests: Vec<RepresentableRequest>,
        stages: Vec<RepresentableStage>,
        queues: Vec<RepresentableQueue>,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct RepresentableRequest {
        request_id: String,
        route: String,
        kind: Option<String>,
        started_at_unix_ms: u64,
        finished_at_unix_ms: u64,
        started_at_run_us: Option<u64>,
        finished_at_run_us: Option<u64>,
        latency_us: u64,
        outcome: String,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct RepresentableStage {
        request_id: String,
        stage: String,
        started_at_unix_ms: u64,
        finished_at_unix_ms: u64,
        started_at_run_us: Option<u64>,
        finished_at_run_us: Option<u64>,
        latency_us: u64,
        success: bool,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct RepresentableQueue {
        request_id: String,
        queue: String,
        waited_from_unix_ms: u64,
        waited_until_unix_ms: u64,
        waited_from_run_us: Option<u64>,
        waited_until_run_us: Option<u64>,
        wait_us: u64,
        depth_at_start: Option<u64>,
    }

    fn representable_evidence(run: &tailtriage_core::Run) -> RepresentableEvidence {
        RepresentableEvidence {
            requests: run
                .requests
                .iter()
                .map(|request| RepresentableRequest {
                    request_id: request.request_id.clone(),
                    route: request.route.clone(),
                    kind: request.kind.clone(),
                    started_at_unix_ms: request.started_at_unix_ms,
                    finished_at_unix_ms: request.finished_at_unix_ms,
                    started_at_run_us: request.started_at_run_us,
                    finished_at_run_us: request.finished_at_run_us,
                    latency_us: request.latency_us,
                    outcome: request.outcome.clone(),
                })
                .collect(),
            stages: run
                .stages
                .iter()
                .map(|stage| RepresentableStage {
                    request_id: stage.request_id.clone(),
                    stage: stage.stage.clone(),
                    started_at_unix_ms: stage.started_at_unix_ms,
                    finished_at_unix_ms: stage.finished_at_unix_ms,
                    started_at_run_us: stage.started_at_run_us,
                    finished_at_run_us: stage.finished_at_run_us,
                    latency_us: stage.latency_us,
                    success: stage.success,
                })
                .collect(),
            queues: run
                .queues
                .iter()
                .map(|queue| RepresentableQueue {
                    request_id: queue.request_id.clone(),
                    queue: queue.queue.clone(),
                    waited_from_unix_ms: queue.waited_from_unix_ms,
                    waited_until_unix_ms: queue.waited_until_unix_ms,
                    waited_from_run_us: queue.waited_from_run_us,
                    waited_until_run_us: queue.waited_until_run_us,
                    wait_us: queue.wait_us,
                    depth_at_start: queue.depth_at_start,
                })
                .collect(),
        }
    }

    #[derive(Debug, Clone, PartialEq)]
    struct SourceIdentity {
        name: String,
        id: Option<String>,
        parent_id: Option<String>,
        fields: BTreeMap<String, FieldValue>,
        started_at_unix_ms: u64,
        finished_at_unix_ms: u64,
        started_at_run_us: Option<u64>,
        finished_at_run_us: Option<u64>,
        duration_us: Option<u64>,
    }

    fn source_identity(spans: &[SpanRecord]) -> Vec<SourceIdentity> {
        spans
            .iter()
            .map(|span| SourceIdentity {
                name: span.name().to_owned(),
                id: span.id_ref().map(ToOwned::to_owned),
                parent_id: span.parent_id_ref().map(ToOwned::to_owned),
                fields: span.fields().clone(),
                started_at_unix_ms: span.started_at_unix_ms(),
                finished_at_unix_ms: span.finished_at_unix_ms(),
                started_at_run_us: span.started_at_run_us_ref(),
                finished_at_run_us: span.finished_at_run_us_ref(),
                duration_us: span.duration_us_ref(),
            })
            .collect()
    }

    #[derive(Debug, Clone, PartialEq)]
    struct IssueProjection {
        severity: &'static str,
        code: &'static str,
        section: tailtriage_core::RunSection,
        section_input_index: Option<usize>,
        field: Option<&'static str>,
    }

    fn relevant_issue_projection(
        report: &tailtriage_core::RunValidationReport,
    ) -> Vec<IssueProjection> {
        report
            .issues
            .iter()
            .filter(|issue| {
                matches!(
                    issue.location.section,
                    tailtriage_core::RunSection::Requests
                        | tailtriage_core::RunSection::Stages
                        | tailtriage_core::RunSection::Queues
                )
            })
            .map(|issue| IssueProjection {
                severity: match issue.severity {
                    tailtriage_core::RunValidationSeverity::Warning => "warning",
                    tailtriage_core::RunValidationSeverity::Error => "error",
                },
                code: issue.code.as_str(),
                section: issue.location.section,
                section_input_index: issue.location.index,
                field: issue.location.field,
            })
            .collect()
    }

    #[test]
    fn open_span_from_start_sample_uses_supplied_start_times() {
        let started_at_unix_ms = 123_456_789;
        let recorder_start = Instant::now();
        let started_instant = recorder_start
            .checked_add(std::time::Duration::from_micros(42))
            .expect("started instant within range");

        let open = open_span_from_start_sample(
            Some("span-1".to_owned()),
            Some("parent-1".to_owned()),
            "request".to_owned(),
            BTreeMap::new(),
            started_at_unix_ms,
            started_instant,
            recorder_start,
            true,
        );

        assert_eq!(open.started_at_unix_ms, started_at_unix_ms);
        assert_eq!(open.started_at_run_us, 42);
        assert_eq!(open.started_instant, started_instant);
    }

    #[test]
    fn live_recorder_request_conversion_populates_run_relative_fields() {
        with_recorder(|recorder| {
            let span = tracing::info_span!(
                "request",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/a"
            );
            drop(span);

            let imported = recorder.snapshot_run().unwrap();
            let request = &imported.run().requests[0];
            let start = request
                .started_at_run_us
                .expect("request run-relative start");
            let finish = request
                .finished_at_run_us
                .expect("request run-relative finish");
            assert!(finish >= start);
        });
    }

    #[test]
    fn live_recorder_stage_conversion_populates_run_relative_fields() {
        with_recorder(|recorder| {
            let request = tracing::info_span!(
                "request",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/a"
            );
            let live_request_guard = request.enter();
            drop(tracing::info_span!(
                "stage",
                tt.kind = "stage",
                tt.request_id = "r1",
                tt.stage = "db"
            ));
            drop(live_request_guard);
            drop(request);

            let imported = recorder.snapshot_run().unwrap();
            let stage = &imported.run().stages[0];
            let start = stage.started_at_run_us.expect("stage run-relative start");
            let finish = stage.finished_at_run_us.expect("stage run-relative finish");
            assert!(finish >= start);
        });
    }

    #[test]
    fn live_recorder_queue_conversion_populates_run_relative_fields() {
        with_recorder(|recorder| {
            let request = tracing::info_span!(
                "request",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/a"
            );
            let live_request_guard = request.enter();
            drop(tracing::info_span!(
                "queue",
                tt.kind = "queue",
                tt.request_id = "r1",
                tt.queue = "permits"
            ));
            drop(live_request_guard);
            drop(request);

            let imported = recorder.snapshot_run().unwrap();
            let queue = &imported.run().queues[0];
            let start = queue.waited_from_run_us.expect("queue run-relative start");
            let finish = queue
                .waited_until_run_us
                .expect("queue run-relative finish");
            assert!(finish >= start);
        });
    }

    #[test]
    fn request_span_collected() {
        with_recorder(|recorder| {
            let span = tracing::info_span!(
                "request",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/a"
            );
            drop(span);
            let run = recorder.snapshot_run().unwrap();
            assert_eq!(run.run().requests.len(), 1);
        });
    }

    #[test]
    fn strict_snapshot_run_succeeds_for_completed_request_span() {
        let recorder = LiveRecorder::builder("svc").strict(true).build().unwrap();
        let subscriber = tracing_subscriber::registry().with(recorder.layer());
        tracing::subscriber::with_default(subscriber, || {
            let span = tracing::info_span!(
                "request",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/a"
            );
            drop(span);
        });

        let snapshot = recorder.snapshot_run().unwrap();
        assert_eq!(snapshot.run().requests.len(), 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn live_completed_request_records_close_wall_clock_and_monotonic_latency() {
        use tracing::Instrument as _;

        let recorder = LiveRecorder::builder("svc").run_id("rid").build().unwrap();
        let subscriber = tracing_subscriber::registry().with(recorder.layer());
        let _guard = tracing::subscriber::set_default(subscriber);

        let span = tracing::info_span!(
            "request",
            tt.kind = "request",
            tt.request_id = "r1",
            tt.route = "/a"
        );
        async { tokio::time::sleep(std::time::Duration::from_millis(1)).await }
            .instrument(span)
            .await;

        let snapshot = recorder.snapshot_run().unwrap();
        assert_eq!(snapshot.run().requests.len(), 1);
        let request = &snapshot.run().requests[0];
        assert!(request.finished_at_unix_ms >= request.started_at_unix_ms);
        assert!(request.latency_us > 0);
    }

    #[test]
    fn duration_uses_captured_close_instant_not_current_time() {
        let started_instant = std::time::Instant::now();
        let closed_instant = started_instant + std::time::Duration::from_millis(1);
        std::thread::sleep(std::time::Duration::from_millis(50));

        let duration_us = duration_us_between(started_instant, closed_instant);

        assert!((1_000..=2_000).contains(&duration_us));
        assert!(duration_us < 10_000);
    }

    #[test]
    fn live_finished_wall_clock_is_not_synthesized_from_duration() {
        let recorder = LiveRecorder::builder("svc").run_id("rid").build().unwrap();
        let subscriber = tracing_subscriber::registry().with(recorder.layer());
        tracing::subscriber::with_default(subscriber, || {
            let span = tracing::info_span!(
                "request",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/a"
            );
            let span_id = span.id().expect("span is enabled").into_u64();
            {
                let mut state = lock_state(&recorder.state);
                let open = state.open.get_mut(&span_id).expect("open span is tracked");
                open.started_at_unix_ms = open.started_at_unix_ms.saturating_sub(60_000);
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
            drop(span);

            let snapshot = recorder.snapshot_run().unwrap();
            assert_eq!(snapshot.run().requests.len(), 1);
            let request = &snapshot.run().requests[0];
            let synthetic_finished_at_unix_ms = request
                .started_at_unix_ms
                .saturating_add(request.latency_us.div_ceil(1_000));

            assert!(request.finished_at_unix_ms >= request.started_at_unix_ms);
            assert!(request.latency_us > 0);
            assert_ne!(request.finished_at_unix_ms, synthetic_finished_at_unix_ms);
        });
    }

    #[test]
    fn stage_span_collected() {
        with_recorder(|recorder| {
            // Keep the request open so the stage interval is contained in the request interval.
            let request = tracing::info_span!(
                "request",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/a"
            )
            .entered();

            let span = tracing::info_span!(
                "stage",
                tt.kind = "stage",
                tt.request_id = "r1",
                tt.stage = "db"
            );
            drop(span);

            drop(request);

            let run = recorder.snapshot_run().unwrap();
            assert_eq!(run.run().stages.len(), 1);
        });
    }

    #[test]
    fn queue_span_collected() {
        with_recorder(|recorder| {
            let request = tracing::info_span!(
                "request",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/a"
            )
            .entered();
            let span = tracing::info_span!(
                "queue",
                tt.kind = "queue",
                tt.request_id = "r1",
                tt.queue = "permits"
            );
            drop(span);
            drop(request);
            let run = recorder.snapshot_run().unwrap();
            assert_eq!(run.run().queues.len(), 1);
        });
    }

    #[test]
    fn on_record_updates_field() {
        with_recorder(|recorder| {
            let span = tracing::info_span!(
                "request",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/a",
                tt.outcome = tracing::field::Empty
            );
            span.record("tt.outcome", "timeout");
            drop(span);
            let run = recorder.snapshot_run().unwrap();
            assert_eq!(run.run().requests[0].outcome, "timeout");
        });
    }

    #[test]
    fn unrelated_span_ignored() {
        with_recorder(|recorder| {
            let span = tracing::info_span!("other", user_id = 42_u64);
            drop(span);
            let run = recorder.snapshot_run().unwrap();
            assert!(run.run().requests.is_empty());
            assert!(run.run().stages.is_empty());
            assert!(run.run().queues.is_empty());
        });
    }

    #[test]
    fn snapshot_run_is_non_consuming_and_shutdown_consumes_owned_handle() {
        with_recorder(|recorder| {
            let span = tracing::info_span!(
                "request",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/checkout"
            );
            drop(span);
            let snapshot = recorder.snapshot_run().unwrap();
            assert_eq!(snapshot.run().requests.len(), 1);
            assert_eq!(snapshot.run().requests[0].request_id, "r1");
            assert_eq!(snapshot.run().requests[0].route, "/checkout");
        });

        let recorder = LiveRecorder::builder("svc").build().unwrap();
        let subscriber = tracing_subscriber::registry().with(recorder.layer());
        tracing::subscriber::with_default(subscriber, || {
            let span = tracing::info_span!(
                "request",
                tt.kind = "request",
                tt.request_id = "r2",
                tt.route = "/checkout"
            );
            drop(span);
        });
        let run = recorder.shutdown().unwrap();
        assert_eq!(run.run().requests.len(), 1);
        assert_eq!(run.run().requests[0].request_id, "r2");
    }

    #[test]
    fn builder_metadata_applies_to_imported_run() {
        let recorder = LiveRecorder::builder("checkout-service")
            .service_version("1.2.3")
            .run_id("run-42")
            .strict(false)
            .build()
            .unwrap();
        let subscriber = tracing_subscriber::registry().with(recorder.layer());
        tracing::subscriber::with_default(subscriber, || {
            let span = tracing::info_span!(
                "request",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/checkout"
            );
            drop(span);
        });

        let run = recorder.snapshot_run().unwrap();
        assert_eq!(run.run().metadata.service_name, "checkout-service");
        assert_eq!(run.run().metadata.service_version.as_deref(), Some("1.2.3"));
        assert_eq!(run.run().metadata.run_id, "run-42");
    }

    #[test]
    fn strict_mode_errors_on_malformed_request() {
        let recorder = LiveRecorder::builder("svc").strict(true).build().unwrap();
        let subscriber = tracing_subscriber::registry().with(recorder.layer());
        tracing::subscriber::with_default(subscriber, || {
            let span = tracing::info_span!("request", tt.kind = "request", tt.request_id = "r1");
            drop(span);
        });

        assert!(recorder.snapshot_run().is_err());
    }

    #[test]
    fn tt_kind_recorded_later_is_captured() {
        with_recorder(|recorder| {
            let span = tracing::info_span!(
                "request",
                tt.kind = tracing::field::Empty,
                tt.request_id = "r1",
                tt.route = "/late-kind"
            );
            span.record("tt.kind", "request");
            drop(span);

            let run = recorder.snapshot_run().unwrap();
            assert_eq!(run.run().requests.len(), 1);
            assert_eq!(run.run().requests[0].route, "/late-kind");
        });
    }

    #[test]
    fn non_tailtriage_fields_do_not_make_span_candidate() {
        with_recorder(|recorder| {
            let span = tracing::info_span!(
                "request",
                http.method = "GET",
                user_id = 42_u64,
                error = tracing::field::Empty
            );
            drop(span);

            let run = recorder.snapshot_run().unwrap();
            assert!(run.run().requests.is_empty());
            assert!(run.run().stages.is_empty());
            assert!(run.run().queues.is_empty());
            assert!(run.warnings().is_empty());
        });
    }

    #[test]
    fn debug_or_invalid_tt_kind_does_not_become_valid_kind() {
        with_recorder(|recorder| {
            let debug_kind = tracing::info_span!(
                "debug-kind",
                tt.kind = ?Some("request"),
                tt.request_id = "r-debug",
                tt.route = "/debug"
            );
            drop(debug_kind);

            let numeric_kind = tracing::info_span!(
                "numeric-kind",
                tt.kind = 7_u64,
                tt.request_id = "r-num",
                tt.route = "/numeric"
            );
            drop(numeric_kind);

            let run = recorder.snapshot_run().unwrap();
            assert!(run.run().requests.is_empty());
            assert!(run.run().stages.is_empty());
            assert!(run.run().queues.is_empty());
        });
    }

    #[test]
    fn display_formatted_tt_kind_request_is_imported() {
        with_recorder(|recorder| {
            struct RequestKind;
            impl fmt::Display for RequestKind {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    f.write_str("request")
                }
            }

            let kind = RequestKind;
            let request = tracing::info_span!(
                "request",
                tt.kind = %kind,
                tt.request_id = "r-display",
                tt.route = "/display-kind"
            );
            drop(request);

            let imported = recorder.snapshot_run().unwrap();
            let run = imported.run();
            assert_eq!(run.requests.len(), 1);
            assert_eq!(run.requests[0].request_id, "r-display");
            assert_eq!(run.requests[0].route, "/display-kind");
            assert!(!imported
                .warnings()
                .iter()
                .any(|w| w.message().contains("invalid tt.kind")));
        });
    }

    #[test]
    fn debug_formatted_string_tt_kind_is_rejected_with_invalid_kind_warning() {
        with_recorder(|recorder| {
            let kind = "request";
            let request = tracing::info_span!(
                "request",
                tt.kind = ?kind,
                tt.request_id = "r-debug-string",
                tt.route = "/debug-string-kind"
            );
            drop(request);

            let imported = recorder.snapshot_run().unwrap();
            let run = imported.run();
            assert!(run.requests.is_empty());
            assert!(imported
                .warnings()
                .iter()
                .any(|w| w.message().contains("invalid tt.kind")));
            assert!(run
                .metadata
                .lifecycle_warnings
                .iter()
                .any(|msg| msg.contains("invalid tt.kind")));
        });
    }

    #[test]
    fn shutdown_output_is_analyzable_and_has_no_runtime_snapshots() {
        with_recorder(|recorder| {
            let request = tracing::info_span!(
                "request",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/checkout"
            )
            .entered();
            let queue = tracing::info_span!(
                "queue",
                tt.kind = "queue",
                tt.request_id = "r1",
                tt.queue = "db-pool"
            );
            let stage = tracing::info_span!(
                "stage",
                tt.kind = "stage",
                tt.request_id = "r1",
                tt.stage = "db.query"
            );
            drop(queue);
            drop(stage);
            drop(request);

            let imported = recorder.snapshot_run().unwrap();
            let run = imported.run();
            assert_eq!(run.requests.len(), 1);
            assert_eq!(run.queues.len(), 1);
            assert_eq!(run.stages.len(), 1);
            assert!(run.runtime_snapshots.is_empty());
            let report = analyze_run(run, AnalyzeOptions::default());
            assert_eq!(report.request_count, 1);
        });
    }

    #[test]
    fn completed_candidate_cap_emits_warning_and_sets_limits_hit_non_strict() {
        let recorder = LiveRecorder::builder("svc")
            .max_completed_candidate_spans(1)
            .build()
            .unwrap();
        let subscriber = tracing_subscriber::registry().with(recorder.layer());
        tracing::subscriber::with_default(subscriber, || {
            let span1 = tracing::info_span!(
                "request",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/a"
            );
            drop(span1);
            let span2 = tracing::info_span!(
                "request",
                tt.kind = "request",
                tt.request_id = "r2",
                tt.route = "/b"
            );
            drop(span2);
        });
        let imported = recorder.snapshot_run().unwrap();
        assert_eq!(imported.run().requests.len(), 1);
        assert!(imported
            .warnings()
            .iter()
            .any(|w| w.message().contains("max_completed_candidate_spans")));
        assert!(imported
            .run()
            .metadata
            .lifecycle_warnings
            .iter()
            .any(|w| w.contains("max_completed_candidate_spans")));
        assert!(imported.run().truncation.limits_hit);
        assert_eq!(imported.run().truncation.dropped_requests, 0);
        assert_eq!(imported.run().truncation.dropped_stages, 0);
        assert_eq!(imported.run().truncation.dropped_queues, 0);
        assert_eq!(imported.run().requests[0].request_id, "r1");
    }

    #[test]
    fn completed_candidate_cap_preserves_request_over_children_non_strict() {
        let recorder = LiveRecorder::builder("svc")
            .max_completed_candidate_spans(2)
            .build()
            .unwrap();
        let subscriber = tracing_subscriber::registry().with(recorder.layer());
        tracing::subscriber::with_default(subscriber, || {
            let request = tracing::info_span!(
                "request",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/checkout"
            );
            let request_guard = request.enter();
            drop(tracing::info_span!(
                "queue",
                tt.kind = "queue",
                tt.request_id = "r1",
                tt.queue = "db"
            ));
            drop(tracing::info_span!(
                "stage",
                tt.kind = "stage",
                tt.request_id = "r1",
                tt.stage = "db.query"
            ));
            drop(request_guard);
            drop(request);
        });
        let imported = recorder.snapshot_run().unwrap();
        assert_eq!(imported.run().requests.len(), 1);
        assert!(imported.run().stages.len() + imported.run().queues.len() <= 1);
        assert!(imported.run().truncation.limits_hit);
        let warning_text = imported
            .warnings()
            .iter()
            .map(|w| w.message().to_string())
            .collect::<Vec<_>>()
            .join(" | ");
        assert!(warning_text.contains("max_completed_candidate_spans"));
        assert!(
            warning_text.contains("dropped 1 completed child candidate span")
                || warning_text.contains("evicted 1 completed child candidate span")
        );
    }

    #[test]
    fn raw_cap_does_not_evict_retained_request_child_for_later_unretained_request() {
        let recorder = LiveRecorder::builder("svc")
            .capture_limits_override(CaptureLimitsOverride {
                max_requests: Some(1),
                max_stages: Some(10),
                max_queues: Some(10),
                ..Default::default()
            })
            .max_completed_candidate_spans(2)
            .build()
            .unwrap();
        let subscriber = tracing_subscriber::registry().with(recorder.layer());
        tracing::subscriber::with_default(subscriber, || {
            let request = tracing::info_span!(
                "request",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/r1"
            );
            let request_guard = request.enter();
            drop(tracing::info_span!(
                "stage",
                tt.kind = "stage",
                tt.request_id = "r1",
                tt.stage = "db.query"
            ));
            drop(request_guard);
            drop(request);

            drop(tracing::info_span!(
                "request",
                tt.kind = "request",
                tt.request_id = "r2",
                tt.route = "/r2"
            ));
        });

        let imported = recorder.snapshot_run().unwrap();
        let run = imported.run();
        assert_eq!(run.requests.len(), 1);
        assert_eq!(run.requests[0].request_id, "r1");
        assert_eq!(run.requests[0].route, "/r1");
        assert!(run.stages.is_empty());
        assert!(run.truncation.limits_hit);
        assert_eq!(run.truncation.dropped_requests, 1);

        let warning_text = imported
            .warnings()
            .iter()
            .map(|w| w.message().to_string())
            .collect::<Vec<_>>()
            .join(" | ");
        assert!(warning_text.contains("evicted 1 completed child candidate span"));
        assert!(!warning_text.contains("dropped 1 completed request candidate span"));
    }

    #[test]
    fn raw_cap_still_preserves_request_root_when_within_semantic_request_limit() {
        let recorder = LiveRecorder::builder("svc")
            .capture_limits_override(CaptureLimitsOverride {
                max_requests: Some(2),
                max_stages: Some(10),
                max_queues: Some(10),
                ..Default::default()
            })
            .max_completed_candidate_spans(2)
            .build()
            .unwrap();
        let subscriber = tracing_subscriber::registry().with(recorder.layer());
        tracing::subscriber::with_default(subscriber, || {
            let request = tracing::info_span!(
                "request",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/r1"
            );
            let request_guard = request.enter();
            drop(tracing::info_span!(
                "stage",
                tt.kind = "stage",
                tt.request_id = "r1",
                tt.stage = "db.query"
            ));
            drop(request_guard);
            drop(request);

            drop(tracing::info_span!(
                "request",
                tt.kind = "request",
                tt.request_id = "r2",
                tt.route = "/r2"
            ));
        });

        let imported = recorder.snapshot_run().unwrap();
        let run = imported.run();
        assert_eq!(run.requests.len(), 2);
        assert_eq!(run.requests[0].request_id, "r1");
        assert_eq!(run.requests[1].request_id, "r2");
        assert_eq!(run.stages.len() + run.queues.len(), 0);
        assert!(run.truncation.limits_hit);

        let warning_text = imported
            .warnings()
            .iter()
            .map(|w| w.message().to_string())
            .collect::<Vec<_>>()
            .join(" | ");
        assert!(warning_text.contains("evicted 1 completed child candidate span"));
        assert!(warning_text.contains("max_completed_candidate_spans=2"));
        assert!(!warning_text.contains("dropped 1 completed request candidate span"));
    }

    #[test]
    fn strict_mode_errors_when_completed_candidate_cap_drops_spans() {
        let recorder = LiveRecorder::builder("svc")
            .strict(true)
            .max_completed_candidate_spans(1)
            .build()
            .unwrap();
        let subscriber = tracing_subscriber::registry().with(recorder.layer());
        tracing::subscriber::with_default(subscriber, || {
            let span1 = tracing::info_span!(
                "request",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/a"
            );
            drop(span1);
            let span2 = tracing::info_span!(
                "request",
                tt.kind = "request",
                tt.request_id = "r2",
                tt.route = "/b"
            );
            drop(span2);
        });
        let err = recorder.snapshot_run().unwrap_err();
        match err {
            ImportError::StrictViolation(message) => {
                assert!(message.contains("max_completed_candidate_spans"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn strict_mode_errors_when_completed_candidate_cap_evicts_child_for_request() {
        let recorder = LiveRecorder::builder("svc")
            .strict(true)
            .max_completed_candidate_spans(2)
            .build()
            .unwrap();
        let subscriber = tracing_subscriber::registry().with(recorder.layer());
        tracing::subscriber::with_default(subscriber, || {
            let request = tracing::info_span!(
                "request",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/checkout"
            );
            let request_guard = request.enter();
            drop(tracing::info_span!(
                "queue",
                tt.kind = "queue",
                tt.request_id = "r1",
                tt.queue = "db"
            ));
            drop(tracing::info_span!(
                "stage",
                tt.kind = "stage",
                tt.request_id = "r1",
                tt.stage = "db.query"
            ));
            drop(request_guard);
            drop(request);
        });
        let err = recorder.snapshot_run().unwrap_err();
        match err {
            ImportError::StrictViolation(message) => {
                assert!(message.contains("max_completed_candidate_spans"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn completed_candidate_conversion_order_is_request_stage_queue() {
        let mut state = RecorderState::default();
        state.completed_stages.push(SpanRecord::new("stage", 1, 2));
        state.completed_queues.push(SpanRecord::new("queue", 1, 2));
        state
            .completed_requests
            .push(SpanRecord::new("request", 1, 2));
        let ordered = completed_span_records(&state);
        assert_eq!(ordered[0].name(), "request");
        assert_eq!(ordered[1].name(), "stage");
        assert_eq!(ordered[2].name(), "queue");
    }

    #[test]
    fn raw_completed_candidate_cap_is_separate_from_semantic_capture_limits() {
        let recorder = LiveRecorder::builder("svc")
            .max_completed_candidate_spans(10)
            .capture_limits(CaptureLimits {
                max_requests: 1,
                ..CaptureMode::Light.core_defaults()
            })
            .build()
            .unwrap();
        let subscriber = tracing_subscriber::registry().with(recorder.layer());
        tracing::subscriber::with_default(subscriber, || {
            drop(tracing::info_span!(
                "request-1",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/a"
            ));
            drop(tracing::info_span!(
                "request-2",
                tt.kind = "request",
                tt.request_id = "r2",
                tt.route = "/b"
            ));
        });
        let imported = recorder.snapshot_run().unwrap();
        assert_eq!(imported.run().requests.len(), 1);
        assert_eq!(imported.run().truncation.dropped_requests, 1);
        assert!(!imported
            .warnings()
            .iter()
            .any(|w| w.message().contains("max_completed_candidate_spans")));
    }

    #[test]
    fn raw_completed_candidate_cap_drops_incoming_request_when_full_of_requests() {
        let recorder = LiveRecorder::builder("svc")
            .max_completed_candidate_spans(3)
            .capture_limits(CaptureLimits {
                max_requests: 3,
                ..CaptureMode::Light.core_defaults()
            })
            .build()
            .unwrap();
        let subscriber = tracing_subscriber::registry().with(recorder.layer());
        tracing::subscriber::with_default(subscriber, || {
            drop(tracing::info_span!(
                "request-1",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/a"
            ));
            drop(tracing::info_span!(
                "request-1-duplicate",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/a-duplicate"
            ));
            drop(tracing::info_span!(
                "request-2",
                tt.kind = "request",
                tt.request_id = "r2",
                tt.route = "/b"
            ));
            drop(tracing::info_span!(
                "request-3",
                tt.kind = "request",
                tt.request_id = "r3",
                tt.route = "/c"
            ));
        });

        let imported = recorder.snapshot_run().unwrap();
        let request_ids = imported
            .run()
            .requests
            .iter()
            .map(|request| request.request_id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(request_ids, vec!["r2"]);
        assert_eq!(imported.run().requests.len(), 1);
        assert!(imported
            .warnings()
            .iter()
            .any(|w| { w.message().contains("duplicate_completed_request_id") }));
        assert!(imported.warnings().iter().any(|w| {
            w.message()
                .contains("dropped 1 completed request candidate span(s)")
                && w.message().contains("max_completed_candidate_spans=3")
        }));
    }

    #[test]
    fn request_arriving_at_full_child_containing_cap_evicts_child_regardless_of_identity() {
        let recorder = LiveRecorder::builder("svc")
            .max_completed_candidate_spans(3)
            .capture_limits(CaptureLimits {
                max_requests: 3,
                max_stages: 10,
                ..CaptureMode::Light.core_defaults()
            })
            .build()
            .unwrap();
        let subscriber = tracing_subscriber::registry().with(recorder.layer());
        tracing::subscriber::with_default(subscriber, || {
            drop(tracing::info_span!(
                "request-1",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/a"
            ));
            drop(tracing::info_span!(
                "request-1-duplicate",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/a-duplicate"
            ));
            drop(tracing::info_span!(
                "stage-r1",
                tt.kind = "stage",
                tt.request_id = "r1",
                tt.stage = "db"
            ));
            drop(tracing::info_span!(
                "request-2",
                tt.kind = "request",
                tt.request_id = "r2",
                tt.route = "/b"
            ));
        });

        let imported = recorder.snapshot_run().unwrap();
        let run = imported.run();
        let request_ids = run
            .requests
            .iter()
            .map(|request| request.request_id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(request_ids, vec!["r2"]);
        assert_eq!(run.requests.len(), 1);
        assert!(run.stages.is_empty());
        assert!(imported.warnings().iter().any(|w| {
            w.message()
                .contains("evicted 1 completed child candidate span(s)")
                && w.message().contains("max_completed_candidate_spans=3")
        }));
    }

    #[test]
    fn strict_mode_errors_when_raw_cap_evicts_child_before_core_excludes_ambiguous_requests() {
        let recorder = LiveRecorder::builder("svc")
            .strict(true)
            .max_completed_candidate_spans(3)
            .capture_limits(CaptureLimits {
                max_requests: 3,
                max_stages: 10,
                ..CaptureMode::Light.core_defaults()
            })
            .build()
            .unwrap();
        let subscriber = tracing_subscriber::registry().with(recorder.layer());
        tracing::subscriber::with_default(subscriber, || {
            drop(tracing::info_span!(
                "request-1",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/a"
            ));
            drop(tracing::info_span!(
                "request-1-duplicate",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/a-duplicate"
            ));
            drop(tracing::info_span!(
                "stage-r1",
                tt.kind = "stage",
                tt.request_id = "r1",
                tt.stage = "db"
            ));
            drop(tracing::info_span!(
                "request-2",
                tt.kind = "request",
                tt.request_id = "r2",
                tt.route = "/b"
            ));
        });

        let err = recorder
            .snapshot_run()
            .expect_err("strict should reject retained duplicate request eviction");
        match err {
            ImportError::StrictViolation(message) => {
                assert!(message.contains("max_completed_candidate_spans"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn full_request_only_raw_cap_drops_incoming_request_regardless_of_identity() {
        let recorder = LiveRecorder::builder("svc")
            .max_completed_candidate_spans(2)
            .capture_limits(CaptureLimits {
                max_requests: 2,
                ..CaptureMode::Light.core_defaults()
            })
            .build()
            .unwrap();
        let subscriber = tracing_subscriber::registry().with(recorder.layer());
        tracing::subscriber::with_default(subscriber, || {
            drop(tracing::info_span!(
                "request-1",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/a"
            ));
            drop(tracing::info_span!(
                "request-1-duplicate",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/a-duplicate"
            ));
            drop(tracing::info_span!(
                "request-2",
                tt.kind = "request",
                tt.request_id = "r2",
                tt.route = "/b"
            ));
        });

        let imported = recorder.snapshot_run().unwrap();
        let request_ids = imported
            .run()
            .requests
            .iter()
            .map(|request| request.request_id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        assert!(request_ids.is_empty());
        assert_eq!(imported.run().requests.len(), 0);
        assert!(imported.retained_sources().is_empty());
        assert_eq!(imported.run().truncation.dropped_requests, 0);
        assert!(imported.warnings().iter().any(|w| {
            w.message()
                .contains("dropped 1 completed request candidate span(s)")
                && w.message().contains("max_completed_candidate_spans=2")
        }));
    }

    #[test]
    fn retained_sources_follow_live_recorder_section_order_with_exact_identity() {
        let recorder = LiveRecorder::builder("svc").build().unwrap();
        {
            let mut state = recorder.state.lock().unwrap();
            state.completed_stages.push(
                SpanRecord::new("stage-a", 110, 120)
                    .id("stage-id")
                    .parent_id("req-id")
                    .field(TT_KIND, "stage")
                    .field("tt.request_id", "r1")
                    .field("tt.stage", "db")
                    .field("custom", "stage-custom"),
            );
            state.completed_queues.push(
                SpanRecord::new("queue-a", 105, 109)
                    .id("queue-id")
                    .parent_id("req-id")
                    .field(TT_KIND, "queue")
                    .field("tt.request_id", "r1")
                    .field("tt.queue", "permits")
                    .field("custom", "queue-custom"),
            );
            state.completed_requests.push(
                SpanRecord::new("request-a", 100, 130)
                    .id("req-id")
                    .parent_id("root-id")
                    .field(TT_KIND, "request")
                    .field("tt.request_id", "r1")
                    .field("tt.route", "/a")
                    .field("custom", "request-custom"),
            );
        }

        let imported = recorder.snapshot_run().unwrap();
        let retained = imported.retained_sources();
        assert_eq!(
            retained.iter().map(SpanRecord::name).collect::<Vec<_>>(),
            vec!["request-a", "stage-a", "queue-a"]
        );
        assert_eq!(retained[0].id_ref(), Some("req-id"));
        assert_eq!(retained[0].parent_id_ref(), Some("root-id"));
        assert_eq!(
            retained[0].fields().get("custom"),
            Some(&FieldValue::String("request-custom".to_owned()))
        );
        assert_eq!(
            retained[0].fields().get(TT_KIND),
            Some(&FieldValue::String("request".to_owned()))
        );
        assert_eq!(retained[1].id_ref(), Some("stage-id"));
        assert_eq!(retained[1].parent_id_ref(), Some("req-id"));
        assert_eq!(
            retained[1].fields().get("custom"),
            Some(&FieldValue::String("stage-custom".to_owned()))
        );
        assert_eq!(
            retained[1].fields().get("tt.stage"),
            Some(&FieldValue::String("db".to_owned()))
        );
        assert_eq!(retained[2].id_ref(), Some("queue-id"));
        assert_eq!(retained[2].parent_id_ref(), Some("req-id"));
        assert_eq!(
            retained[2].fields().get("custom"),
            Some(&FieldValue::String("queue-custom".to_owned()))
        );
        assert_eq!(
            retained[2].fields().get("tt.queue"),
            Some(&FieldValue::String("permits".to_owned()))
        );
    }

    #[test]
    fn snapshots_are_non_consuming_and_shutdown_retained_sources_are_deterministic() {
        let build = || {
            let recorder = LiveRecorder::builder("svc").run_id("rid").build().unwrap();
            recorder.state.lock().unwrap().completed_requests.push(
                SpanRecord::new("request", 100, 120)
                    .id("req-id")
                    .field(TT_KIND, "request")
                    .field("tt.request_id", "r1")
                    .field("tt.route", "/a")
                    .field("tt.outcome", "ok")
                    .field("custom", "kept"),
            );
            recorder
        };

        let recorder = build();
        let first = recorder.snapshot_run().unwrap();
        let second = recorder.snapshot_run().unwrap();
        assert_eq!(first.run(), second.run());
        assert_eq!(first.warnings(), second.warnings());
        assert_eq!(first.retained_sources(), second.retained_sources());

        let shutdown = build().shutdown().unwrap();
        assert_eq!(first.run(), shutdown.run());
        assert_eq!(first.warnings(), shutdown.warnings());
        assert_eq!(first.retained_sources(), shutdown.retained_sources());
    }

    #[test]
    fn semantic_request_limit_applies_after_raw_recorder_retention() {
        let recorder = LiveRecorder::builder("svc")
            .max_completed_candidate_spans(2)
            .capture_limits(CaptureLimits {
                max_requests: 1,
                ..CaptureMode::Light.core_defaults()
            })
            .build()
            .unwrap();
        let subscriber = tracing_subscriber::registry().with(recorder.layer());
        tracing::subscriber::with_default(subscriber, || {
            drop(tracing::info_span!(
                "request-1",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/a"
            ));
            drop(tracing::info_span!(
                "request-2",
                tt.kind = "request",
                tt.request_id = "r2",
                tt.route = "/b"
            ));
        });

        let imported = recorder.snapshot_run().unwrap();
        assert_eq!(imported.run().requests.len(), 1);
        assert_eq!(imported.run().requests[0].request_id, "r1");
        assert_eq!(
            imported
                .retained_sources()
                .iter()
                .map(SpanRecord::name)
                .collect::<Vec<_>>(),
            vec!["request-1"]
        );
        assert_eq!(imported.run().truncation.dropped_requests, 1);
        assert_eq!(imported.run().truncation.dropped_stages, 0);
        assert_eq!(imported.run().truncation.dropped_queues, 0);
        assert!(!imported
            .warnings()
            .iter()
            .any(|w| w.message().contains("max_completed_candidate_spans")));
    }

    #[test]
    fn strict_mode_errors_when_max_open_spans_drops_candidate_spans() {
        let recorder = LiveRecorder::builder("svc")
            .strict(true)
            .max_open_spans(1)
            .build()
            .unwrap();
        let subscriber = tracing_subscriber::registry().with(recorder.layer());
        tracing::subscriber::with_default(subscriber, || {
            let span1 = tracing::info_span!(
                "request",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/a"
            );
            let span2 = tracing::info_span!(
                "request",
                tt.kind = "request",
                tt.request_id = "r2",
                tt.route = "/b"
            );
            drop(span1);
            drop(span2);
        });
        let err = recorder
            .snapshot_run()
            .expect_err("strict should reject retention drops");
        match err {
            ImportError::StrictViolation(message) => {
                assert!(message.contains("dropped 1 open candidate span"));
                assert!(message.contains("max_open_spans=1"));
                assert!(message.contains("reduce capture scope"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn strict_mode_combines_recorder_drop_and_conversion_strict_violations() {
        let recorder = LiveRecorder::builder("svc")
            .strict(true)
            .capture_limits(tailtriage_core::CaptureLimits {
                max_requests: 1,
                max_stages: 1,
                max_queues: 1,
                ..tailtriage_core::CaptureMode::Light.core_defaults()
            })
            .build()
            .unwrap();
        let subscriber = tracing_subscriber::registry().with(recorder.layer());
        tracing::subscriber::with_default(subscriber, || {
            let malformed =
                tracing::info_span!("request-bad", tt.kind = "request", tt.request_id = "r-bad");
            drop(malformed);
            let valid = tracing::info_span!(
                "request-good",
                tt.kind = "request",
                tt.request_id = "r-good",
                tt.route = "/ok"
            );
            drop(valid);
        });

        let err = recorder.snapshot_run().expect_err(
            "strict mode should fail when recorder retention drops and strict conversion both occur",
        );
        match err {
            ImportError::StrictViolation(message) => {
                assert!(message.contains("incomplete required fields"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn incomplete_request_does_not_consume_completed_candidate_cap_in_non_strict_mode() {
        let recorder = LiveRecorder::builder("svc")
            .max_completed_candidate_spans(1)
            .build()
            .unwrap();
        let subscriber = tracing_subscriber::registry().with(recorder.layer());
        tracing::subscriber::with_default(subscriber, || {
            drop(tracing::info_span!(
                "request-malformed",
                tt.kind = "request",
                tt.request_id = "r-bad"
            ));
            drop(tracing::info_span!(
                "request-valid",
                tt.kind = "request",
                tt.request_id = "r-good",
                tt.route = "/ok"
            ));
        });
        let imported = recorder.snapshot_run().unwrap();
        assert_eq!(imported.run().requests.len(), 1);
        assert_eq!(imported.run().requests[0].request_id, "r-good");
        assert!(imported
            .warnings()
            .iter()
            .any(|w| w.message().contains("incomplete required fields")));
        assert!(!imported
            .warnings()
            .iter()
            .any(|w| w.message().contains("max_completed_candidate_spans")));
    }

    #[test]
    fn incomplete_stage_does_not_consume_completed_candidate_cap_in_non_strict_mode() {
        let recorder = LiveRecorder::builder("svc")
            .max_completed_candidate_spans(2)
            .build()
            .unwrap();
        let subscriber = tracing_subscriber::registry().with(recorder.layer());
        tracing::subscriber::with_default(subscriber, || {
            let request = tracing::info_span!(
                "request-valid",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/ok"
            );
            let _request_guard = request.enter();
            drop(tracing::info_span!(
                "stage-missing-field",
                tt.kind = "stage",
                tt.request_id = "r1"
            ));
            drop(tracing::info_span!(
                "stage-valid",
                tt.kind = "stage",
                tt.request_id = "r1",
                tt.stage = "db"
            ));
        });
        let imported = recorder.snapshot_run().unwrap();
        assert_eq!(imported.run().requests.len(), 1);
        assert_eq!(imported.run().stages.len(), 1);
        assert_eq!(imported.run().stages[0].stage, "db");
        assert!(imported
            .warnings()
            .iter()
            .any(|w| w.message().contains("incomplete required fields")));
        assert!(imported
            .warnings()
            .iter()
            .any(|w| w.message().contains("missing required field tt.stage")));
        assert!(!imported
            .warnings()
            .iter()
            .any(|w| w.message().contains("max_completed_candidate_spans")));
    }

    #[test]
    fn incomplete_queue_does_not_consume_completed_candidate_cap_in_non_strict_mode() {
        let recorder = LiveRecorder::builder("svc")
            .max_completed_candidate_spans(2)
            .build()
            .unwrap();
        let subscriber = tracing_subscriber::registry().with(recorder.layer());
        tracing::subscriber::with_default(subscriber, || {
            let request = tracing::info_span!(
                "request-valid",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/ok"
            );
            let _request_guard = request.enter();
            drop(tracing::info_span!(
                "queue-missing-field",
                tt.kind = "queue",
                tt.request_id = "r1"
            ));
            drop(tracing::info_span!(
                "queue-valid",
                tt.kind = "queue",
                tt.request_id = "r1",
                tt.queue = "db-pool"
            ));
        });
        let imported = recorder.snapshot_run().unwrap();
        assert_eq!(imported.run().requests.len(), 1);
        assert_eq!(imported.run().queues.len(), 1);
        assert_eq!(imported.run().queues[0].queue, "db-pool");
        assert!(imported
            .warnings()
            .iter()
            .any(|w| w.message().contains("incomplete required fields")));
        assert!(imported
            .warnings()
            .iter()
            .any(|w| w.message().contains("missing required field tt.queue")));
        assert!(!imported
            .warnings()
            .iter()
            .any(|w| w.message().contains("max_completed_candidate_spans")));
    }

    #[test]
    fn invalid_numeric_required_fields_do_not_consume_completed_candidate_cap() {
        let recorder = LiveRecorder::builder("svc")
            .max_completed_candidate_spans(1)
            .build()
            .unwrap();
        let subscriber = tracing_subscriber::registry().with(recorder.layer());
        tracing::subscriber::with_default(subscriber, || {
            drop(tracing::info_span!(
                "request-invalid",
                tt.kind = "request",
                tt.request_id = "r-bad",
                tt.route = 7_u64
            ));
            drop(tracing::info_span!(
                "request-valid",
                tt.kind = "request",
                tt.request_id = "r-good",
                tt.route = "/ok"
            ));
        });
        let imported = recorder.snapshot_run().unwrap();
        assert_eq!(imported.run().requests.len(), 1);
        assert_eq!(imported.run().requests[0].request_id, "r-good");
        assert!(imported
            .warnings()
            .iter()
            .any(|w| w.message().contains("invalid required field tt.route")));
        assert!(!imported
            .warnings()
            .iter()
            .any(|w| w.message().contains("max_completed_candidate_spans")));
    }

    #[test]
    fn whitespace_only_required_field_reports_incomplete_candidate_not_invalid_run_event() {
        let recorder = LiveRecorder::builder("svc")
            .max_completed_candidate_spans(2)
            .build()
            .unwrap();
        let subscriber = tracing_subscriber::registry().with(recorder.layer());
        tracing::subscriber::with_default(subscriber, || {
            drop(tracing::info_span!(
                "request-whitespace",
                tt.kind = "request",
                tt.request_id = " ",
                tt.route = "/bad"
            ));
            drop(tracing::info_span!(
                "request-good",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/ok"
            ));
        });
        let imported = recorder.snapshot_run().unwrap();
        assert_eq!(imported.run().requests.len(), 1);
        assert!(imported.warnings().iter().any(|w| {
            w.message().contains("incomplete required fields")
                && w.message()
                    .contains("required string cannot be empty or whitespace")
        }));
        assert!(!imported
            .warnings()
            .iter()
            .any(|w| w.message().contains("InvalidRunEvent")));
    }

    #[test]
    fn invalid_numeric_stage_does_not_consume_completed_candidate_cap() {
        let recorder = LiveRecorder::builder("svc")
            .max_completed_candidate_spans(2)
            .build()
            .unwrap();
        let subscriber = tracing_subscriber::registry().with(recorder.layer());
        tracing::subscriber::with_default(subscriber, || {
            let request = tracing::info_span!(
                "request-valid",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/ok"
            );
            let _request_guard = request.enter();
            drop(tracing::info_span!(
                "stage-invalid",
                tt.kind = "stage",
                tt.request_id = "r1",
                tt.stage = 7_u64
            ));
            drop(tracing::info_span!(
                "stage-valid",
                tt.kind = "stage",
                tt.request_id = "r1",
                tt.stage = "db"
            ));
        });
        let imported = recorder.snapshot_run().unwrap();
        assert_eq!(imported.run().requests.len(), 1);
        assert_eq!(imported.run().stages.len(), 1);
        assert_eq!(imported.run().stages[0].stage, "db");
        assert!(imported
            .warnings()
            .iter()
            .any(|w| w.message().contains("invalid required field tt.stage")));
        assert!(!imported
            .warnings()
            .iter()
            .any(|w| w.message().contains("max_completed_candidate_spans")));
    }

    #[test]
    fn invalid_numeric_queue_does_not_consume_completed_candidate_cap() {
        let recorder = LiveRecorder::builder("svc")
            .max_completed_candidate_spans(2)
            .build()
            .unwrap();
        let subscriber = tracing_subscriber::registry().with(recorder.layer());
        tracing::subscriber::with_default(subscriber, || {
            let request = tracing::info_span!(
                "request-valid",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/ok"
            );
            let _request_guard = request.enter();
            drop(tracing::info_span!(
                "queue-invalid",
                tt.kind = "queue",
                tt.request_id = "r1",
                tt.queue = 7_u64
            ));
            drop(tracing::info_span!(
                "queue-valid",
                tt.kind = "queue",
                tt.request_id = "r1",
                tt.queue = "db-pool"
            ));
        });
        let imported = recorder.snapshot_run().unwrap();
        assert_eq!(imported.run().requests.len(), 1);
        assert_eq!(imported.run().queues.len(), 1);
        assert_eq!(imported.run().queues[0].queue, "db-pool");
        assert!(imported
            .warnings()
            .iter()
            .any(|w| w.message().contains("invalid required field tt.queue")));
        assert!(!imported
            .warnings()
            .iter()
            .any(|w| w.message().contains("max_completed_candidate_spans")));
    }

    #[test]
    fn source_valid_orphan_stage_consumes_semantic_stage_retention_before_core_exclusion() {
        let recorder = LiveRecorder::builder("svc")
            .capture_limits(CaptureLimits {
                max_requests: 1,
                max_stages: 1,
                ..CaptureMode::Light.core_defaults()
            })
            .build()
            .unwrap();
        let subscriber = tracing_subscriber::registry().with(recorder.layer());
        tracing::subscriber::with_default(subscriber, || {
            drop(tracing::info_span!(
                "stage-orphan",
                tt.kind = "stage",
                tt.request_id = "missing",
                tt.stage = "db"
            ));
            let request = tracing::info_span!(
                "request-valid",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/ok"
            );
            let _request_guard = request.enter();
            drop(tracing::info_span!(
                "stage-valid",
                tt.kind = "stage",
                tt.request_id = "r1",
                tt.stage = "db"
            ));
        });
        let imported = recorder.snapshot_run().unwrap();
        assert!(imported.run().stages.is_empty());
        assert_eq!(imported.run().truncation.dropped_stages, 1);
    }

    #[test]
    fn source_valid_orphan_queue_consumes_semantic_queue_retention_before_core_exclusion() {
        let recorder = LiveRecorder::builder("svc")
            .capture_limits(CaptureLimits {
                max_requests: 1,
                max_queues: 1,
                ..CaptureMode::Light.core_defaults()
            })
            .build()
            .unwrap();
        let subscriber = tracing_subscriber::registry().with(recorder.layer());
        tracing::subscriber::with_default(subscriber, || {
            drop(tracing::info_span!(
                "queue-orphan",
                tt.kind = "queue",
                tt.request_id = "missing",
                tt.queue = "db-pool"
            ));
            let request = tracing::info_span!(
                "request-valid",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/ok"
            );
            let _request_guard = request.enter();
            drop(tracing::info_span!(
                "queue-valid",
                tt.kind = "queue",
                tt.request_id = "r1",
                tt.queue = "db-pool"
            ));
        });
        let imported = recorder.snapshot_run().unwrap();
        assert!(imported.run().queues.is_empty());
        assert_eq!(imported.run().truncation.dropped_queues, 1);
    }

    #[test]
    fn strict_mode_fails_for_malformed_request_span() {
        let recorder = LiveRecorder::builder("svc").strict(true).build().unwrap();
        let subscriber = tracing_subscriber::registry().with(recorder.layer());
        tracing::subscriber::with_default(subscriber, || {
            drop(tracing::info_span!(
                "request-malformed",
                tt.kind = "request",
                tt.request_id = "r1"
            ));
        });
        let err = recorder.snapshot_run().unwrap_err();
        assert!(matches!(err, ImportError::StrictViolation(_)));
    }

    #[test]
    fn strict_mode_fails_for_invalid_numeric_request_route() {
        let recorder = LiveRecorder::builder("svc").strict(true).build().unwrap();
        let subscriber = tracing_subscriber::registry().with(recorder.layer());
        tracing::subscriber::with_default(subscriber, || {
            drop(tracing::info_span!(
                "request-invalid-route",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = 1_u64
            ));
        });
        let err = recorder.snapshot_run().unwrap_err();
        match err {
            ImportError::StrictViolation(message) => {
                assert!(message.contains("incomplete required fields"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn strict_mode_fails_for_malformed_stage_span() {
        let recorder = LiveRecorder::builder("svc").strict(true).build().unwrap();
        let subscriber = tracing_subscriber::registry().with(recorder.layer());
        tracing::subscriber::with_default(subscriber, || {
            let request = tracing::info_span!(
                "request-valid",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/ok"
            );
            let _request_guard = request.enter();
            drop(tracing::info_span!(
                "stage-malformed",
                tt.kind = "stage",
                tt.request_id = "r1"
            ));
        });
        let err = recorder.snapshot_run().unwrap_err();
        assert!(matches!(err, ImportError::StrictViolation(_)));
    }

    #[test]
    fn strict_mode_fails_for_invalid_numeric_stage_field() {
        let recorder = LiveRecorder::builder("svc").strict(true).build().unwrap();
        let subscriber = tracing_subscriber::registry().with(recorder.layer());
        tracing::subscriber::with_default(subscriber, || {
            let request = tracing::info_span!(
                "request-valid",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/ok"
            );
            let _request_guard = request.enter();
            drop(tracing::info_span!(
                "stage-invalid",
                tt.kind = "stage",
                tt.request_id = "r1",
                tt.stage = 2_u64
            ));
        });
        let err = recorder.snapshot_run().unwrap_err();
        match err {
            ImportError::StrictViolation(message) => {
                assert!(message.contains("incomplete required fields"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn strict_mode_fails_for_malformed_queue_span() {
        let recorder = LiveRecorder::builder("svc").strict(true).build().unwrap();
        let subscriber = tracing_subscriber::registry().with(recorder.layer());
        tracing::subscriber::with_default(subscriber, || {
            let request = tracing::info_span!(
                "request-valid",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/ok"
            );
            let _request_guard = request.enter();
            drop(tracing::info_span!(
                "queue-malformed",
                tt.kind = "queue",
                tt.request_id = "r1"
            ));
        });
        let err = recorder.snapshot_run().unwrap_err();
        assert!(matches!(err, ImportError::StrictViolation(_)));
    }

    #[test]
    fn strict_mode_fails_for_invalid_numeric_queue_field() {
        let recorder = LiveRecorder::builder("svc").strict(true).build().unwrap();
        let subscriber = tracing_subscriber::registry().with(recorder.layer());
        tracing::subscriber::with_default(subscriber, || {
            let request = tracing::info_span!(
                "request-valid",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/ok"
            );
            let _request_guard = request.enter();
            drop(tracing::info_span!(
                "queue-invalid",
                tt.kind = "queue",
                tt.request_id = "r1",
                tt.queue = 3_u64
            ));
        });
        let err = recorder.snapshot_run().unwrap_err();
        match err {
            ImportError::StrictViolation(message) => {
                assert!(message.contains("incomplete required fields"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn strict_mode_fails_for_orphan_stage_span() {
        let recorder = LiveRecorder::builder("svc").strict(true).build().unwrap();
        let subscriber = tracing_subscriber::registry().with(recorder.layer());
        tracing::subscriber::with_default(subscriber, || {
            drop(tracing::info_span!(
                "stage-orphan",
                tt.kind = "stage",
                tt.request_id = "missing",
                tt.stage = "db"
            ));
        });
        let err = recorder.snapshot_run().unwrap_err();
        assert!(matches!(err, ImportError::StrictViolation(_)));
    }

    #[test]
    fn strict_mode_fails_for_orphan_queue_span() {
        let recorder = LiveRecorder::builder("svc").strict(true).build().unwrap();
        let subscriber = tracing_subscriber::registry().with(recorder.layer());
        tracing::subscriber::with_default(subscriber, || {
            drop(tracing::info_span!(
                "queue-orphan",
                tt.kind = "queue",
                tt.request_id = "missing",
                tt.queue = "db-pool"
            ));
        });
        let err = recorder.snapshot_run().unwrap_err();
        assert!(matches!(err, ImportError::StrictViolation(_)));
    }

    #[test]
    fn open_span_saturation_emits_warning_and_sets_limits_hit() {
        let recorder = LiveRecorder::builder("svc")
            .max_open_spans(1)
            .build()
            .unwrap();
        let subscriber = tracing_subscriber::registry().with(recorder.layer());
        tracing::subscriber::with_default(subscriber, || {
            let span1 = tracing::info_span!(
                "request",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/a"
            );
            let span2 = tracing::info_span!(
                "request",
                tt.kind = "request",
                tt.request_id = "r2",
                tt.route = "/b"
            );
            drop(span1);
            drop(span2);
        });
        let imported = recorder.snapshot_run().unwrap();
        assert_eq!(imported.run().requests.len(), 1);
        assert!(imported
            .warnings()
            .iter()
            .any(|w| w.message().contains("dropped 1 candidate spans")));
        assert!(imported
            .run()
            .metadata
            .lifecycle_warnings
            .iter()
            .any(|w| w.contains("dropped 1 candidate spans")));
        assert!(imported.run().truncation.limits_hit);
    }

    #[test]
    fn non_strict_mode_reports_drop_warnings_and_truncation() {
        let recorder = LiveRecorder::builder("svc")
            .max_open_spans(1)
            .capture_limits(tailtriage_core::CaptureLimits {
                max_requests: 1,
                max_stages: 1,
                max_queues: 1,
                ..tailtriage_core::CaptureMode::Light.core_defaults()
            })
            .build()
            .unwrap();
        let subscriber = tracing_subscriber::registry().with(recorder.layer());
        tracing::subscriber::with_default(subscriber, || {
            let _open_1 = tracing::info_span!(
                "request-open-1",
                tt.kind = "request",
                tt.request_id = "r-open-1",
                tt.route = "/open-1"
            )
            .entered();
            let _open_2 = tracing::info_span!(
                "request-open-2",
                tt.kind = "request",
                tt.request_id = "r-open-2",
                tt.route = "/open-2"
            )
            .entered();
        });

        let subscriber = tracing_subscriber::registry().with(recorder.layer());
        tracing::subscriber::with_default(subscriber, || {
            let span1 = tracing::info_span!(
                "request-closed-1",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/a"
            );
            drop(span1);
            let span2 = tracing::info_span!(
                "request-closed-2",
                tt.kind = "request",
                tt.request_id = "r2",
                tt.route = "/b"
            );
            drop(span2);
        });

        let imported = recorder.snapshot_run().unwrap();
        assert!(imported
            .warnings()
            .iter()
            .any(|w| w.message().contains("dropped") && w.message().contains("max_open_spans")));
        assert!(imported
            .run()
            .metadata
            .lifecycle_warnings
            .iter()
            .any(|w| w.contains("max_open_spans")));
        assert!(imported.run().truncation.dropped_requests >= 1);
        assert!(imported.run().truncation.limits_hit);
    }

    #[test]
    fn unrelated_spans_do_not_consume_open_limit() {
        let recorder = LiveRecorder::builder("svc")
            .max_open_spans(1)
            .build()
            .unwrap();
        let subscriber = tracing_subscriber::registry().with(recorder.layer());
        tracing::subscriber::with_default(subscriber, || {
            let unrelated = tracing::info_span!("ordinary", foo = 1_u64);
            let request = tracing::info_span!(
                "request",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/a",
                tt.outcome = "ok"
            );
            drop(request);
            drop(unrelated);
        });
        let imported = recorder.snapshot_run().unwrap();
        assert_eq!(imported.run().requests.len(), 1);
        assert!(imported.warnings().is_empty());
    }

    #[test]
    fn live_recorder_builder_rejects_blank_service_name() {
        let err = LiveRecorder::builder("   ").build().unwrap_err();
        assert!(matches!(err, ImportError::EmptyServiceName));
    }

    #[test]
    fn tracing_intake_session_builder_rejects_blank_service_name() {
        let err = TracingSession::builder("   ").build().unwrap_err();
        assert!(matches!(err, ImportError::EmptyServiceName));
    }

    #[test]
    fn closed_candidate_missing_tt_kind_warns_non_strict() {
        with_recorder(|recorder| {
            let span = tracing::info_span!(
                "http.request",
                tt.kind = tracing::field::Empty,
                tt.request_id = "r1",
                tt.route = "/checkout"
            );
            drop(span);
            let imported = recorder.snapshot_run().unwrap();
            assert!(imported.run().requests.is_empty());
            assert!(imported.run().stages.is_empty());
            assert!(imported.run().queues.is_empty());
            assert_eq!(imported.warnings().len(), 1);
            let msg = imported.warnings()[0].message();
            assert!(msg.contains("invalid tt.kind"));
            assert!(msg.contains("missing=1"));
            assert!(msg.contains("unknown=0"));
            assert!(msg.contains("malformed=0"));
            assert!(msg.contains("reason=missing"));
            assert!(msg.contains("http.request") || msg.contains("r1"));
            assert!(imported
                .run()
                .metadata
                .lifecycle_warnings
                .iter()
                .any(|w| w == msg));
        });
    }

    #[test]
    fn closed_candidate_unknown_tt_kind_warns_non_strict() {
        with_recorder(|recorder| {
            let span = tracing::info_span!(
                "http.request",
                tt.kind = "bogus",
                tt.request_id = "r-unknown",
                tt.route = "/checkout"
            );
            drop(span);
            let imported = recorder.snapshot_run().unwrap();
            assert!(imported.run().requests.is_empty());
            assert!(imported.run().stages.is_empty());
            assert!(imported.run().queues.is_empty());
            assert_eq!(imported.warnings().len(), 1);
            let msg = imported.warnings()[0].message();
            assert!(msg.contains("invalid tt.kind"));
            assert!(msg.contains("missing=0"));
            assert!(msg.contains("unknown=1"));
            assert!(msg.contains("malformed=0"));
            assert!(msg.contains("reason=unknown"));
            assert!(msg.contains("r-unknown"));
        });
    }

    #[test]
    fn closed_candidate_malformed_tt_kind_warns_non_strict() {
        with_recorder(|recorder| {
            let span = tracing::info_span!(
                "http.request",
                tt.kind = 42_u64,
                tt.request_id = "r-malformed",
                tt.route = "/checkout"
            );
            drop(span);
            let imported = recorder.snapshot_run().unwrap();
            assert!(imported.run().requests.is_empty());
            assert!(imported.run().stages.is_empty());
            assert!(imported.run().queues.is_empty());
            assert_eq!(imported.warnings().len(), 1);
            let msg = imported.warnings()[0].message();
            assert!(msg.contains("invalid tt.kind"));
            assert!(msg.contains("missing=0"));
            assert!(msg.contains("unknown=0"));
            assert!(msg.contains("malformed=1"));
            assert!(msg.contains("reason=malformed"));
            assert!(msg.contains("r-malformed"));
        });
    }

    #[test]
    fn invalid_kind_warning_aggregates_missing_unknown_and_malformed_counts() {
        with_recorder(|recorder| {
            drop(tracing::info_span!(
                "missing.kind",
                tt.kind = tracing::field::Empty,
                tt.request_id = "r-missing"
            ));
            drop(tracing::info_span!(
                "unknown.kind",
                tt.kind = "bogus",
                tt.request_id = "r-unknown"
            ));
            drop(tracing::info_span!(
                "malformed.kind",
                tt.kind = 7_u64,
                tt.request_id = "r-malformed"
            ));
            let imported = recorder.snapshot_run().unwrap();
            assert!(imported.run().requests.is_empty());
            assert!(imported.run().stages.is_empty());
            assert!(imported.run().queues.is_empty());
            assert_eq!(imported.warnings().len(), 1);
            let msg = imported.warnings()[0].message();
            assert!(msg.contains("invalid tt.kind"));
            assert!(msg.contains("missing=1"));
            assert!(msg.contains("unknown=1"));
            assert!(msg.contains("malformed=1"));
            assert!(msg.contains("reason=missing"));
            assert!(msg.contains("reason=unknown"));
            assert!(msg.contains("reason=malformed"));
        });
    }

    #[test]
    fn closed_candidate_missing_tt_kind_errors_strict() {
        let recorder = LiveRecorder::builder("svc").strict(true).build().unwrap();
        let subscriber = tracing_subscriber::registry().with(recorder.layer());
        tracing::subscriber::with_default(subscriber, || {
            let span = tracing::info_span!(
                "http.request",
                tt.kind = tracing::field::Empty,
                tt.request_id = "r1",
                tt.route = "/checkout"
            );
            drop(span);
        });
        let err = recorder.snapshot_run().unwrap_err();
        match err {
            ImportError::StrictViolation(message) => {
                assert!(
                    message.contains("missing tt.kind") || message.contains("closed candidate")
                );
                assert!(!message.contains("0 open candidate span(s)"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn closed_candidate_missing_tt_kind_shutdown_errors_strict() {
        let recorder = LiveRecorder::builder("svc").strict(true).build().unwrap();
        let subscriber = tracing_subscriber::registry().with(recorder.layer());
        tracing::subscriber::with_default(subscriber, || {
            let span = tracing::info_span!(
                "http.request",
                tt.kind = tracing::field::Empty,
                tt.request_id = "r1",
                tt.route = "/checkout"
            );
            drop(span);
        });
        let err = recorder.shutdown().unwrap_err();
        match err {
            ImportError::StrictViolation(message) => {
                assert!(
                    message.contains("missing tt.kind") || message.contains("closed candidate")
                );
                assert!(!message.contains("0 open candidate span(s)"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn strict_mode_reports_open_and_closed_missing_kind_causes_together() {
        let recorder = LiveRecorder::builder("svc").strict(true).build().unwrap();
        let subscriber = tracing_subscriber::registry().with(recorder.layer());
        tracing::subscriber::with_default(subscriber, || {
            let _open = tracing::info_span!(
                "request",
                tt.kind = "request",
                tt.request_id = "r-open",
                tt.route = "/open"
            )
            .entered();
            let closed_missing_kind = tracing::info_span!(
                "stage.db",
                tt.kind = tracing::field::Empty,
                tt.request_id = "r-closed"
            );
            drop(closed_missing_kind);

            let err = recorder.snapshot_run().unwrap_err();
            match err {
                ImportError::StrictViolation(message) => {
                    assert!(message.contains("open candidate span(s)"));
                    assert!(
                        message.contains("missing tt.kind") || message.contains("closed candidate")
                    );
                }
                other => panic!("unexpected error: {other:?}"),
            }
        });
    }

    #[test]
    fn unrelated_closed_span_still_ignored_without_warning() {
        with_recorder(|recorder| {
            let span = tracing::info_span!("ordinary", user_id = 7_u64);
            drop(span);
            let imported = recorder.snapshot_run().unwrap();
            assert!(imported.run().requests.is_empty());
            assert!(imported.run().stages.is_empty());
            assert!(imported.run().queues.is_empty());
            assert!(imported.warnings().is_empty());
        });
    }
    #[test]
    fn strict_mode_rejects_open_candidate_spans_without_fabricating_completions() {
        let recorder = LiveRecorder::builder("svc").strict(true).build().unwrap();
        let subscriber = tracing_subscriber::registry().with(recorder.layer());
        let err = tracing::subscriber::with_default(subscriber, || {
            let _open_guard = tracing::info_span!(
                "request",
                tt.kind = "request",
                tt.request_id = "r-open",
                tt.route = "/open"
            )
            .entered();
            recorder.snapshot_run().expect_err("strict should reject")
        });
        match err {
            ImportError::StrictViolation(message) => {
                assert!(message.contains("open candidate span(s)"));
                assert!(message.contains("not converted into fabricated completions"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn open_candidate_span_warns_on_snapshot_and_shutdown_non_strict() {
        with_recorder(|recorder| {
            let _open = tracing::info_span!(
                "request",
                tt.kind = "request",
                tt.request_id = "r-open",
                tt.route = "/open"
            )
            .entered();
            let snapshot = recorder.snapshot_run().unwrap();
            assert!(snapshot.run().requests.is_empty());
            assert!(snapshot.warnings().iter().any(|w| w
                .message()
                .contains("open candidate span(s) at snapshot/shutdown")));
            assert!(snapshot
                .run()
                .metadata
                .lifecycle_warnings
                .iter()
                .any(|w| w.contains("open candidate span(s) at snapshot/shutdown")));
            let shutdown = recorder.snapshot_run().unwrap();
            assert!(shutdown.warnings().iter().any(|w| w
                .message()
                .contains("open candidate span(s) at snapshot/shutdown")));
        });
    }

    #[test]
    fn open_candidate_span_errors_in_strict_mode() {
        let recorder = LiveRecorder::builder("svc").strict(true).build().unwrap();
        let subscriber = tracing_subscriber::registry().with(recorder.layer());
        tracing::subscriber::with_default(subscriber, || {
            let _open =
                tracing::info_span!("request", tt.kind = "request", tt.request_id = "r1").entered();
            let err = recorder.snapshot_run().unwrap_err();
            assert!(matches!(err, ImportError::StrictViolation(_)));
        });
    }

    #[test]
    fn unrelated_open_span_does_not_warn() {
        with_recorder(|recorder| {
            let _open = tracing::info_span!("other", user = 1_u64).entered();
            let snapshot = recorder.snapshot_run().unwrap();
            assert!(snapshot.warnings().is_empty());
        });
    }

    #[test]
    fn open_candidate_with_empty_tt_kind_still_warns() {
        with_recorder(|recorder| {
            let _open = tracing::info_span!(
                "request",
                tt.kind = tracing::field::Empty,
                tt.request_id = "r-empty"
            )
            .entered();
            let snapshot = recorder.snapshot_run().unwrap();
            assert!(snapshot.warnings().iter().any(|w| w
                .message()
                .contains("open candidate span(s) at snapshot/shutdown")));
        });
    }

    #[test]
    fn direct_completed_span_jsonl_writer_preserves_exact_retained_sources() {
        let dir = tempfile::tempdir().unwrap();
        let first_path = dir.path().join("first.jsonl");
        let second_path = dir.path().join("second.jsonl");
        let sources = vec![
            SpanRecord::new("request-source", 1_700_000_000_001, 1_700_000_000_051)
                .id("req-span")
                .parent_id("root-span")
                .started_at_run_us(10)
                .finished_at_run_us(50_010)
                .duration_us(50_000)
                .field(TT_KIND, "request")
                .field("tt.request_id", "r1")
                .field("tt.route", "/exact")
                .field("tt.outcome", "ok")
                .field("custom.string", "value")
                .field("custom.bool", true)
                .field("custom.u64", 7_u64),
            SpanRecord::new("stage-source", 1_700_000_000_010, 1_700_000_000_020)
                .id("stage-span")
                .parent_id("req-span")
                .started_at_run_us(9_000)
                .finished_at_run_us(19_000)
                .duration_us(10_000)
                .field(TT_KIND, "stage")
                .field("tt.request_id", "r1")
                .field("tt.stage", "db")
                .field("custom.i64", -3_i64)
                .field("custom.f64", 1.25_f64),
            SpanRecord::new("queue-source", 1_700_000_000_021, 1_700_000_000_025)
                .id("queue-span")
                .parent_id("req-span")
                .duration_us(4_000)
                .field(TT_KIND, "queue")
                .field("tt.request_id", "r1")
                .field("tt.queue", "permits")
                .field("custom.null", FieldValue::Null),
        ];

        write_completed_span_jsonl_from_retained_sources(&sources, &first_path).unwrap();
        write_completed_span_jsonl_from_retained_sources(&sources, &second_path).unwrap();

        let raw = std::fs::read_to_string(&first_path).unwrap();
        let lines = raw.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), sources.len());
        for line in &lines {
            let value: serde_json::Value = serde_json::from_str(line).unwrap();
            assert_eq!(value["format"], "tailtriage.tracing-span.v1");
        }
        assert_eq!(decode_completed_span_jsonl(&first_path), sources);
        assert_eq!(
            source_projection(&decode_completed_span_jsonl(&first_path)),
            source_projection(&sources)
        );
        assert_eq!(
            std::fs::read(&first_path).unwrap(),
            std::fs::read(&second_path).unwrap()
        );
    }

    #[test]
    fn direct_completed_span_jsonl_writer_excludes_core_excluded_sources() {
        let dir = tempfile::tempdir().unwrap();
        let spans_path = dir.path().join("spans.jsonl");
        let sources = vec![
            SpanRecord::new("duplicate-request-a", 100, 150)
                .id("dup-a")
                .field(TT_KIND, "request")
                .field("tt.request_id", "dup")
                .field("tt.route", "/dup-a"),
            SpanRecord::new("duplicate-request-b", 101, 151)
                .id("dup-b")
                .field(TT_KIND, "request")
                .field("tt.request_id", "dup")
                .field("tt.route", "/dup-b"),
            SpanRecord::new("ambiguous-child", 110, 120)
                .id("ambiguous")
                .parent_id("dup-a")
                .field(TT_KIND, "stage")
                .field("tt.request_id", "dup")
                .field("tt.stage", "ambiguous"),
            SpanRecord::new("orphan-child", 110, 120)
                .id("orphan")
                .field(TT_KIND, "queue")
                .field("tt.request_id", "missing")
                .field("tt.queue", "orphan"),
            SpanRecord::new("child-of-excluded-parent", 111, 121)
                .id("excluded-child")
                .parent_id("dup-a")
                .field(TT_KIND, "queue")
                .field("tt.request_id", "dup")
                .field("tt.queue", "excluded-parent"),
            SpanRecord::new("valid-request", 200, 300)
                .id("valid-req")
                .started_at_run_us(0)
                .finished_at_run_us(100_000)
                .field(TT_KIND, "request")
                .field("tt.request_id", "ok")
                .field("tt.route", "/ok"),
            SpanRecord::new("outside-child", 301, 310)
                .id("outside")
                .parent_id("valid-req")
                .started_at_run_us(101_000)
                .finished_at_run_us(110_000)
                .field(TT_KIND, "stage")
                .field("tt.request_id", "ok")
                .field("tt.stage", "outside"),
            SpanRecord::new("valid-child", 210, 220)
                .id("valid-child")
                .parent_id("valid-req")
                .started_at_run_us(10_000)
                .finished_at_run_us(20_000)
                .field(TT_KIND, "stage")
                .field("tt.request_id", "ok")
                .field("tt.stage", "inside"),
        ];
        let imported = run_from_span_records(sources, ImportOptions::new("svc")).unwrap();

        write_completed_span_jsonl_from_retained_sources(imported.retained_sources(), &spans_path)
            .unwrap();

        assert_eq!(
            source_projection(&decode_completed_span_jsonl(&spans_path)),
            vec![
                ("valid-request".to_owned(), "valid-req".to_owned()),
                ("valid-child".to_owned(), "valid-child".to_owned()),
            ]
        );
    }

    fn request(name: &str, id: &str, start: u64, finish: u64) -> SpanRecord {
        SpanRecord::new(name, start, finish)
            .id(format!("{id}-span"))
            .field(TT_KIND, "request")
            .field("tt.request_id", id)
            .field("tt.route", format!("/{id}"))
            .field("tt.outcome", "ok")
    }

    fn stage(name: &str, id: &str, start: u64, finish: u64) -> SpanRecord {
        SpanRecord::new(name, start, finish)
            .id(format!("{name}-span"))
            .parent_id(format!("{id}-span"))
            .field(TT_KIND, "stage")
            .field("tt.request_id", id)
            .field("tt.stage", name)
            .field("tt.success", true)
    }

    fn queue(name: &str, id: &str, start: u64, finish: u64) -> SpanRecord {
        SpanRecord::new(name, start, finish)
            .id(format!("{name}-span"))
            .parent_id(format!("{id}-span"))
            .field(TT_KIND, "queue")
            .field("tt.request_id", id)
            .field("tt.queue", name)
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn canonical_completed_span_jsonl_replay_matches_representable_evidence_projection() {
        #[derive(Clone, Copy)]
        struct ExpectedAccounting {
            dropped_requests: u64,
            dropped_stages: u64,
            dropped_queues: u64,
            limits_hit: bool,
        }

        struct Case {
            name: &'static str,
            spans: Vec<SpanRecord>,
            options: ImportOptions,
            strict_replay: bool,
            expected_accounting: ExpectedAccounting,
        }

        let no_drops = ExpectedAccounting {
            dropped_requests: 0,
            dropped_stages: 0,
            dropped_queues: 0,
            limits_hit: false,
        };

        let cases = vec![
            Case {
                name: "precise custom identity and fields",
                spans: vec![
                    request("custom-http", "precise", 1_000, 1_050)
                        .id("req-custom")
                        .parent_id("root")
                        .started_at_run_us(10)
                        .finished_at_run_us(50_010)
                        .duration_us(50_000)
                        .field("custom.string", "kept")
                        .field("custom.u64", 42_u64),
                    stage("custom-db", "precise", 1_010, 1_030)
                        .id("stage-custom")
                        .parent_id("req-custom")
                        .started_at_run_us(10_010)
                        .finished_at_run_us(30_010)
                        .duration_us(20_000)
                        .field("custom.bool", false),
                    queue("custom-permits", "precise", 1_031, 1_040)
                        .id("queue-custom")
                        .parent_id("req-custom")
                        .started_at_run_us(31_010)
                        .finished_at_run_us(40_010)
                        .duration_us(9_000)
                        .field("tt.depth_at_start", 8_u64)
                        .field("custom.null", FieldValue::Null),
                ],
                options: ImportOptions::new("svc"),
                strict_replay: true,
                expected_accounting: no_drops,
            },
            Case {
                name: "duration only",
                spans: vec![
                    request("duration-request", "duration", 2_000, 2_050).duration_us(123_456),
                    stage("duration-stage", "duration", 2_010, 2_020).duration_us(10_000),
                    queue("duration-queue", "duration", 2_021, 2_025).duration_us(4_000),
                ],
                options: ImportOptions::new("svc"),
                strict_replay: true,
                expected_accounting: no_drops,
            },
            Case {
                name: "repairable optional offsets",
                spans: vec![
                    request("repair-request", "repair", 3_000, 3_050)
                        .started_at_run_us(50)
                        .finished_at_run_us(10)
                        .duration_us(50_000),
                    stage("repair-stage", "repair", 3_010, 3_020)
                        .started_at_run_us(20)
                        .finished_at_run_us(15)
                        .duration_us(10_000),
                    queue("repair-queue", "repair", 3_021, 3_030)
                        .started_at_run_us(30)
                        .finished_at_run_us(25)
                        .duration_us(9_000),
                ],
                options: ImportOptions::new("svc"),
                strict_replay: false,
                expected_accounting: no_drops,
            },
            Case {
                name: "duplicates ambiguous children and valid survivor",
                spans: vec![
                    request("dup-a", "dup", 4_000, 4_050),
                    request("dup-b", "dup", 4_001, 4_051),
                    stage("ambiguous-stage", "dup", 4_010, 4_020),
                    request("survivor", "survivor", 4_100, 4_150),
                    queue("survivor-queue", "survivor", 4_110, 4_120),
                ],
                options: ImportOptions::new("svc"),
                strict_replay: false,
                expected_accounting: no_drops,
            },
            Case {
                name: "orphan child with valid survivor",
                spans: vec![
                    stage("orphan-stage", "missing", 5_010, 5_020),
                    queue("orphan-queue", "missing", 5_021, 5_025),
                    request("orphan-survivor", "orphan-survivor", 5_100, 5_150),
                ],
                options: ImportOptions::new("svc"),
                strict_replay: false,
                expected_accounting: no_drops,
            },
            Case {
                name: "source parser inverted parent leaves child orphan",
                spans: vec![
                    SpanRecord::new("invalid-parent", 6_050, 6_000)
                        .id("invalid-parent-span")
                        .field(TT_KIND, "request")
                        .field("tt.request_id", "bad-parent")
                        .field("tt.route", "/bad-parent"),
                    stage(
                        "child-of-parser-rejected-parent",
                        "bad-parent",
                        6_010,
                        6_020,
                    ),
                    request(
                        "valid-with-parser-rejected-peer",
                        "valid-peer",
                        6_100,
                        6_150,
                    ),
                ],
                options: ImportOptions::new("svc"),
                strict_replay: false,
                expected_accounting: no_drops,
            },
            Case {
                name: "contained parent with precise outside child",
                spans: vec![
                    request("containment-parent", "contained", 7_000, 7_050)
                        .started_at_run_us(0)
                        .finished_at_run_us(50_000),
                    stage("outside-stage", "contained", 7_060, 7_070)
                        .started_at_run_us(60_000)
                        .finished_at_run_us(70_000),
                    queue("inside-queue", "contained", 7_010, 7_020)
                        .started_at_run_us(10_000)
                        .finished_at_run_us(20_000),
                ],
                options: ImportOptions::new("svc"),
                strict_replay: false,
                expected_accounting: no_drops,
            },
            Case {
                name: "semantic limits",
                spans: vec![
                    request("limit-request-kept", "limit-kept", 8_000, 8_050),
                    request("limit-request-dropped", "limit-dropped", 8_100, 8_150),
                    stage("limit-stage-kept", "limit-kept", 8_010, 8_020),
                    stage("limit-stage-dropped", "limit-kept", 8_021, 8_030),
                    queue("limit-queue-kept", "limit-kept", 8_031, 8_035),
                    queue("limit-queue-dropped", "limit-kept", 8_036, 8_040),
                ],
                options: ImportOptions::new("svc").capture_limits_override(
                    tailtriage_core::CaptureLimitsOverride {
                        max_requests: Some(1),
                        max_stages: Some(1),
                        max_queues: Some(1),
                        ..tailtriage_core::CaptureLimitsOverride::default()
                    },
                ),
                strict_replay: false,
                expected_accounting: ExpectedAccounting {
                    dropped_requests: 1,
                    dropped_stages: 1,
                    dropped_queues: 1,
                    limits_hit: true,
                },
            },
        ];

        for case in cases {
            let dir = tempfile::tempdir().unwrap();
            let spans_path = dir.path().join("spans.jsonl");
            let direct = run_from_span_records(case.spans.clone(), case.options.clone())
                .unwrap_or_else(|err| panic!("{} direct conversion failed: {err}", case.name));
            let direct_projection = representable_evidence(direct.run());
            assert_eq!(
                direct.run().truncation.dropped_requests,
                case.expected_accounting.dropped_requests,
                "{} dropped request accounting mismatch",
                case.name
            );
            assert_eq!(
                direct.run().truncation.dropped_stages,
                case.expected_accounting.dropped_stages,
                "{} dropped stage accounting mismatch",
                case.name
            );
            assert_eq!(
                direct.run().truncation.dropped_queues,
                case.expected_accounting.dropped_queues,
                "{} dropped queue accounting mismatch",
                case.name
            );
            assert_eq!(
                direct.run().truncation.limits_hit,
                case.expected_accounting.limits_hit,
                "{} limits_hit accounting mismatch",
                case.name
            );
            write_completed_span_jsonl_from_retained_sources(
                direct.retained_sources(),
                &spans_path,
            )
            .unwrap_or_else(|err| panic!("{} write failed: {err}", case.name));
            let decoded = decode_completed_span_jsonl(&spans_path);
            assert_eq!(
                source_identity(&decoded),
                source_identity(direct.retained_sources()),
                "{} retained source identity write/read mismatch",
                case.name
            );
            let replay = crate::jsonl::import_jsonl_path_with_mode(
                &spans_path,
                ImportOptions::new("svc"),
                crate::jsonl::JsonlParseMode::TailtriageWrapperOnly,
            )
            .unwrap_or_else(|err| panic!("{} permissive replay failed: {err}", case.name));
            assert_eq!(
                representable_evidence(replay.run()),
                direct_projection,
                "{} direct/replay representable projection mismatch",
                case.name
            );
            assert_eq!(
                source_identity(replay.retained_sources()),
                source_identity(direct.retained_sources()),
                "{} replay retained source identity mismatch",
                case.name
            );
            if case.strict_replay {
                let strict = crate::jsonl::import_jsonl_path_with_mode(
                    &spans_path,
                    ImportOptions::new("svc").strict(true),
                    crate::jsonl::JsonlParseMode::TailtriageWrapperOnly,
                )
                .unwrap_or_else(|err| panic!("{} strict replay failed: {err}", case.name));
                assert_eq!(representable_evidence(strict.run()), direct_projection);
            }
        }
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn repaired_optional_precision_writer_emits_original_source_values() {
        let dir = tempfile::tempdir().unwrap();
        let first_path = dir.path().join("spans.jsonl");
        let second_path = dir.path().join("reemitted.jsonl");
        let sources = vec![
            request("repairable-request", "repair", 1_000, 1_020)
                .id("repairable-request")
                .started_at_run_us(50)
                .finished_at_run_us(10)
                .duration_us(20_000),
            stage("repairable-stage", "repair", 1_005, 1_015)
                .id("repairable-stage")
                .parent_id("repairable-request")
                .started_at_run_us(40)
                .finished_at_run_us(20)
                .duration_us(10_000),
            queue("repairable-queue", "repair", 1_006, 1_012)
                .id("repairable-queue")
                .parent_id("repairable-request")
                .started_at_run_us(35)
                .finished_at_run_us(25)
                .duration_us(6_000),
        ];

        let direct_provenance =
            crate::convert_span_records_with_provenance(sources.clone(), ImportOptions::new("svc"))
                .unwrap();
        let direct = &direct_provenance.imported;
        assert_eq!(direct.run().requests[0].started_at_run_us, None);
        assert_eq!(direct.run().requests[0].finished_at_run_us, None);
        assert_eq!(direct.run().requests[0].latency_us, 20_000);
        assert_eq!(direct.run().stages[0].started_at_run_us, None);
        assert_eq!(direct.run().stages[0].finished_at_run_us, None);
        assert_eq!(direct.run().stages[0].latency_us, 10_000);
        assert_eq!(direct.run().queues[0].waited_from_run_us, None);
        assert_eq!(direct.run().queues[0].waited_until_run_us, None);
        assert_eq!(direct.run().queues[0].wait_us, 6_000);
        assert_eq!(direct.retained_sources(), sources.as_slice());
        assert_eq!(
            source_identity(direct.retained_sources()),
            source_identity(&sources)
        );

        let direct_issues = relevant_issue_projection(&direct_provenance.normalized.report);
        assert!(!direct_issues.is_empty());
        assert_eq!(
            direct_issues,
            vec![
                IssueProjection {
                    severity: "error",
                    code: "inverted_interval",
                    section: tailtriage_core::RunSection::Requests,
                    section_input_index: Some(0),
                    field: None,
                },
                IssueProjection {
                    severity: "error",
                    code: "inverted_interval",
                    section: tailtriage_core::RunSection::Stages,
                    section_input_index: Some(0),
                    field: None,
                },
                IssueProjection {
                    severity: "error",
                    code: "inverted_interval",
                    section: tailtriage_core::RunSection::Queues,
                    section_input_index: Some(0),
                    field: None,
                },
            ]
        );

        write_completed_span_jsonl_from_retained_sources(direct.retained_sources(), &first_path)
            .unwrap();
        let decoded = decode_completed_span_jsonl(&first_path);
        assert_eq!(decoded, sources);
        let replay_provenance =
            crate::convert_span_records_with_provenance(decoded.clone(), ImportOptions::new("svc"))
                .unwrap();
        assert_eq!(
            relevant_issue_projection(&replay_provenance.normalized.report),
            direct_issues
        );

        let replay = crate::jsonl::import_jsonl_path_with_mode(
            &first_path,
            ImportOptions::new("svc"),
            crate::jsonl::JsonlParseMode::TailtriageWrapperOnly,
        )
        .unwrap();
        assert_eq!(
            representable_evidence(replay.run()),
            representable_evidence(direct.run())
        );
        assert_eq!(
            source_identity(replay.retained_sources()),
            source_identity(&sources)
        );
        assert_eq!(
            source_identity(replay.retained_sources()),
            source_identity(direct.retained_sources())
        );

        write_completed_span_jsonl_from_retained_sources(replay.retained_sources(), &second_path)
            .unwrap();
        assert_eq!(
            std::fs::read(&first_path).unwrap(),
            std::fs::read(&second_path).unwrap()
        );
        assert_eq!(
            analyze_run(replay.run(), AnalyzeOptions::default()),
            analyze_run(direct.run(), AnalyzeOptions::default())
        );
    }

    #[test]
    fn semantic_unavailable_records_are_not_written_to_completed_jsonl() {
        let semantic_dir = tempfile::tempdir().unwrap();
        let semantic_path = semantic_dir.path().join("semantic.jsonl");
        let semantic_session = TracingSession::builder("svc")
            .completed_span_jsonl_path(&semantic_path)
            .capture_limits(CaptureLimits {
                max_requests: 1,
                max_stages: 1,
                max_queues: 1,
                ..CaptureMode::Light.core_defaults()
            })
            .build()
            .unwrap();
        {
            let mut state = semantic_session.recorder.state.lock().unwrap();
            state.completed_requests.push(
                SpanRecord::new("sem-request-kept", 100, 200)
                    .id("sr1")
                    .field(TT_KIND, "request")
                    .field("tt.request_id", "sr1")
                    .field("tt.route", "/one"),
            );
            state.completed_requests.push(
                SpanRecord::new("sem-request-dropped", 101, 201)
                    .id("sr2")
                    .field(TT_KIND, "request")
                    .field("tt.request_id", "sr2")
                    .field("tt.route", "/two"),
            );
            state.completed_stages.push(
                SpanRecord::new("sem-stage-kept", 110, 120)
                    .id("ss1")
                    .field(TT_KIND, "stage")
                    .field("tt.request_id", "sr1")
                    .field("tt.stage", "db"),
            );
            state.completed_stages.push(
                SpanRecord::new("sem-stage-dropped", 121, 130)
                    .id("ss2")
                    .field(TT_KIND, "stage")
                    .field("tt.request_id", "sr1")
                    .field("tt.stage", "cache"),
            );
            state.completed_queues.push(
                SpanRecord::new("sem-queue-kept", 130, 140)
                    .id("sq1")
                    .field(TT_KIND, "queue")
                    .field("tt.request_id", "sr1")
                    .field("tt.queue", "permits"),
            );
            state.completed_queues.push(
                SpanRecord::new("sem-queue-dropped", 141, 150)
                    .id("sq2")
                    .field(TT_KIND, "queue")
                    .field("tt.request_id", "sr1")
                    .field("tt.queue", "work"),
            );
        }
        let semantic_snapshot = semantic_session.snapshot_run().unwrap();
        assert_eq!(semantic_snapshot.run().truncation.dropped_requests, 1);
        assert_eq!(semantic_snapshot.run().truncation.dropped_stages, 1);
        assert_eq!(semantic_snapshot.run().truncation.dropped_queues, 1);
        futures_executor::block_on(semantic_session.shutdown()).unwrap();
        assert_eq!(
            source_projection(&decode_completed_span_jsonl(&semantic_path)),
            vec![
                ("sem-request-kept".into(), "sr1".into()),
                ("sem-stage-kept".into(), "ss1".into()),
                ("sem-queue-kept".into(), "sq1".into())
            ]
        );
    }

    #[test]
    fn raw_unavailable_records_are_not_written_to_completed_jsonl() {
        let raw_dir = tempfile::tempdir().unwrap();
        let raw_path = raw_dir.path().join("raw.jsonl");
        let raw_session = TracingSession::builder("svc")
            .completed_span_jsonl_path(&raw_path)
            .max_completed_candidate_spans(1)
            .build()
            .unwrap();
        let subscriber = tracing_subscriber::registry().with(raw_session.layer());
        tracing::subscriber::with_default(subscriber, || {
            drop(tracing::info_span!(
                "raw-kept",
                tt.kind = "request",
                tt.request_id = "raw-kept",
                tt.route = "/kept"
            ));
            drop(tracing::info_span!(
                "raw-dropped",
                tt.kind = "stage",
                tt.request_id = "raw-kept",
                tt.stage = "dropped"
            ));
        });
        let raw_snapshot = raw_session.snapshot_run().unwrap();
        assert!(raw_snapshot.warnings().iter().any(|w| w
            .message()
            .contains("dropped 1 completed child candidate span(s)")
            && w.message().contains("max_completed_candidate_spans=1")));
        assert_eq!(raw_snapshot.run().truncation.dropped_requests, 0);
        assert_eq!(raw_snapshot.run().truncation.dropped_stages, 0);
        assert_eq!(raw_snapshot.run().truncation.dropped_queues, 0);
        let raw_shutdown = futures_executor::block_on(raw_session.shutdown()).unwrap();
        assert_eq!(
            representable_evidence(raw_shutdown.run()),
            representable_evidence(raw_snapshot.run())
        );
        let decoded = decode_completed_span_jsonl(&raw_path);
        let raw_names = decoded
            .iter()
            .map(|span| span.name().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(raw_names, vec!["raw-kept".to_owned()]);
        assert!(!raw_names.iter().any(|name| name == "raw-dropped"));
        let replay = crate::jsonl::import_jsonl_path_with_mode(
            &raw_path,
            ImportOptions::new("svc"),
            crate::jsonl::JsonlParseMode::TailtriageWrapperOnly,
        )
        .unwrap();
        assert_eq!(
            representable_evidence(replay.run()),
            representable_evidence(raw_shutdown.run())
        );
        assert_eq!(replay.run().truncation.dropped_requests, 0);
        assert_eq!(replay.run().truncation.dropped_stages, 0);
        assert_eq!(replay.run().truncation.dropped_queues, 0);
        assert!(!replay.warnings().iter().any(|warning| warning
            .message()
            .contains("max_completed_candidate_spans=1")));
        // Replay sees only retained source records, so it does not reproduce raw-drop
        // warnings or raw-recorder pressure counters from omitted sources.
    }

    fn write_representative_live_session_jsonl(path: &Path) -> ImportedRun {
        let session = TracingSession::builder("svc")
            .completed_span_jsonl_path(path)
            .build()
            .unwrap();
        {
            let mut state = session.recorder.state.lock().unwrap();
            state.completed_stages.push(
                SpanRecord::new("live-stage-first", 10, 20)
                    .started_at_run_us(10_000)
                    .finished_at_run_us(20_000)
                    .field(TT_KIND, "stage")
                    .field("tt.request_id", "live-1")
                    .field("tt.stage", "db"),
            );
            state.completed_queues.push(
                SpanRecord::new("live-queue-second", 21, 25)
                    .started_at_run_us(21_000)
                    .finished_at_run_us(25_000)
                    .field(TT_KIND, "queue")
                    .field("tt.request_id", "live-1")
                    .field("tt.queue", "permits"),
            );
            state.completed_requests.push(
                SpanRecord::new("live-request-third", 1, 50)
                    .started_at_run_us(1_000)
                    .finished_at_run_us(50_000)
                    .field(TT_KIND, "request")
                    .field("tt.request_id", "live-1")
                    .field("tt.route", "/live")
                    .field("custom", "identity"),
            );
        }
        futures_executor::block_on(session.shutdown()).unwrap()
    }

    #[test]
    fn live_session_completed_jsonl_is_section_grouped_and_byte_deterministic() {
        let first_dir = tempfile::tempdir().unwrap();
        let second_dir = tempfile::tempdir().unwrap();
        let first_path = first_dir.path().join("spans.jsonl");
        let second_path = second_dir.path().join("spans.jsonl");

        let first = write_representative_live_session_jsonl(&first_path);
        let second = write_representative_live_session_jsonl(&second_path);
        assert_eq!(
            representable_evidence(first.run()),
            representable_evidence(second.run())
        );
        assert_eq!(
            source_identity(first.retained_sources()),
            source_identity(second.retained_sources())
        );
        assert_eq!(
            std::fs::read(&first_path).unwrap(),
            std::fs::read(&second_path).unwrap()
        );

        let decoded = decode_completed_span_jsonl(&first_path);
        assert_eq!(
            decoded.iter().map(SpanRecord::name).collect::<Vec<_>>(),
            vec![
                "live-request-third",
                "live-stage-first",
                "live-queue-second"
            ]
        );
        assert_eq!(
            source_identity(&decoded),
            source_identity(first.retained_sources())
        );

        let replay = crate::jsonl::import_jsonl_path_with_mode(
            &first_path,
            ImportOptions::new("svc"),
            crate::jsonl::JsonlParseMode::TailtriageWrapperOnly,
        )
        .unwrap();
        assert_eq!(
            representable_evidence(replay.run()),
            representable_evidence(first.run())
        );
    }

    fn assert_stable_wrapper_reemits_byte_identically(name: &str, spans: Vec<SpanRecord>) {
        let dir = tempfile::tempdir().unwrap();
        let first_path = dir.path().join(format!("{name}-first.jsonl"));
        let second_path = dir.path().join(format!("{name}-second.jsonl"));
        let direct = run_from_span_records(spans, ImportOptions::new("svc")).unwrap();
        write_completed_span_jsonl_from_retained_sources(direct.retained_sources(), &first_path)
            .unwrap();
        let replay = crate::jsonl::import_jsonl_path_with_mode(
            &first_path,
            ImportOptions::new("svc"),
            crate::jsonl::JsonlParseMode::TailtriageWrapperOnly,
        )
        .unwrap();
        write_completed_span_jsonl_from_retained_sources(replay.retained_sources(), &second_path)
            .unwrap();
        assert_eq!(
            source_identity(replay.retained_sources()),
            source_identity(direct.retained_sources())
        );
        assert_eq!(
            std::fs::read(&first_path).unwrap(),
            std::fs::read(&second_path).unwrap()
        );
    }

    #[test]
    fn stable_wrapper_import_reemits_precise_and_repairable_sources_byte_identically() {
        assert_stable_wrapper_reemits_byte_identically(
            "precise",
            vec![
                request("precise-reemit-request", "reemit", 10, 30)
                    .id("precise-req")
                    .parent_id("root")
                    .started_at_run_us(1)
                    .finished_at_run_us(20_001)
                    .duration_us(20_000)
                    .field("custom", "kept"),
                stage("precise-reemit-stage", "reemit", 12, 20)
                    .id("precise-stage")
                    .parent_id("precise-req"),
                queue("precise-reemit-queue", "reemit", 21, 25)
                    .id("precise-queue")
                    .parent_id("precise-req"),
            ],
        );
        assert_stable_wrapper_reemits_byte_identically(
            "repair",
            vec![
                request("repair-reemit-request", "repair-reemit", 40, 80)
                    .started_at_run_us(50)
                    .finished_at_run_us(10)
                    .duration_us(40_000),
                stage("repair-reemit-stage", "repair-reemit", 45, 55)
                    .started_at_run_us(20)
                    .finished_at_run_us(15)
                    .duration_us(10_000),
                queue("repair-reemit-queue", "repair-reemit", 56, 60)
                    .started_at_run_us(30)
                    .finished_at_run_us(25)
                    .duration_us(4_000),
            ],
        );
    }

    #[cfg(feature = "tokio")]
    #[tokio::test(flavor = "current_thread")]
    async fn tokio_session_retained_sources_replay_request_stage_queue_but_not_runtime_snapshots() {
        let dir = tempfile::tempdir().unwrap();
        let spans_path = dir.path().join("tokio-spans.jsonl");
        let session = crate::TracingSession::builder("svc")
            .manual_runtime_snapshots()
            .build()
            .unwrap();
        let subscriber = tracing_subscriber::registry().with(session.layer());
        tracing::subscriber::with_default(subscriber, || {
            tracing::info_span!(
                "tokio-request",
                tt.kind = "request",
                tt.request_id = "tokio-r1",
                tt.route = "/tokio"
            )
            .in_scope(|| {
                tracing::info_span!(
                    "tokio-stage",
                    tt.kind = "stage",
                    tt.request_id = "tokio-r1",
                    tt.stage = "db"
                )
                .in_scope(|| {});
                tracing::info_span!(
                    "tokio-queue",
                    tt.kind = "queue",
                    tt.request_id = "tokio-r1",
                    tt.queue = "permits",
                    tt.depth_at_start = 1_u64
                )
                .in_scope(|| {});
            });
        });
        session
            .record_runtime_snapshot(tailtriage_core::RuntimeSnapshot {
                at_unix_ms: tailtriage_core::unix_time_ms(),
                at_run_us: Some(1_000),
                alive_tasks: Some(2),
                global_queue_depth: Some(3),
                local_queue_depth: Some(4),
                blocking_queue_depth: Some(5),
                remote_schedule_count: Some(6),
            })
            .expect("record runtime snapshot");

        let imported = session.shutdown().await.unwrap();
        assert_eq!(
            imported
                .retained_sources()
                .iter()
                .map(SpanRecord::name)
                .collect::<Vec<_>>(),
            vec!["tokio-request", "tokio-stage", "tokio-queue"]
        );
        assert_eq!(imported.run().runtime_snapshots.len(), 1);
        write_completed_span_jsonl_from_retained_sources(imported.retained_sources(), &spans_path)
            .unwrap();
        let replay = crate::jsonl::import_jsonl_path_with_mode(
            &spans_path,
            ImportOptions::new("svc"),
            crate::jsonl::JsonlParseMode::TailtriageWrapperOnly,
        )
        .unwrap();
        assert_eq!(
            representable_evidence(replay.run()),
            representable_evidence(imported.run())
        );
        assert_eq!(
            source_identity(replay.retained_sources()),
            source_identity(imported.retained_sources())
        );
        assert!(replay.run().runtime_snapshots.is_empty());
        assert_ne!(replay.run(), imported.run());
        // Completed-span JSONL intentionally replays retained request/stage/queue
        // evidence only; runtime snapshots remain Run-only metadata.
    }

    #[test]
    fn completed_jsonl_missing_retained_sources_error_is_deterministic_without_output() {
        let dir = tempfile::tempdir().unwrap();
        let spans_path = dir.path().join("spans.jsonl");
        let imported = run_from_span_records(
            vec![SpanRecord::new("request", 100, 200)
                .field(TT_KIND, "request")
                .field("tt.request_id", "r1")
                .field("tt.route", "/a")],
            ImportOptions::new("svc"),
        )
        .unwrap();
        let err = validate_completed_span_jsonl_retained_sources(imported.run(), &[], &spans_path)
            .unwrap_err();
        match err {
            ImportError::Io {
                operation,
                context,
                reason,
            } => {
                assert_eq!(operation, "prepare completed span jsonl retained sources");
                assert_eq!(context, spans_path.display().to_string());
                assert_eq!(reason, "internal invariant violation: completed-span JSONL output requires retained original source spans");
            }
            other => panic!("unexpected error: {other}"),
        }
        assert!(!spans_path.exists());
        assert!(completed_span_jsonl_temp_path(&spans_path)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .contains("tailtriage-tmp"));
    }

    #[test]
    fn intake_session_wrapper_jsonl_and_truncate_behavior() {
        let dir = tempfile::tempdir().unwrap();
        let spans_path = dir.path().join("spans.jsonl");
        std::fs::write(
            &spans_path,
            r#"{"format":"tailtriage.tracing-span.v1","span":{"name":"old","started_at_unix_ms":1,"finished_at_unix_ms":2,"fields":{"tt.kind":"request","tt.request_id":"old","tt.route":"/old"}}}"#,
        )
        .unwrap();
        let session = TracingSession::builder("svc")
            .completed_span_jsonl_path(&spans_path)
            .build()
            .unwrap();
        let subscriber = tracing_subscriber::registry().with(session.layer());
        tracing::subscriber::with_default(subscriber, || {
            let span = tracing::info_span!(
                "request",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/a"
            );
            drop(span);
        });
        let imported = futures_executor::block_on(session.shutdown()).unwrap();
        assert_eq!(imported.run().requests.len(), 1);
        let raw = std::fs::read_to_string(&spans_path).unwrap();
        assert!(!raw.contains("\"old\""));
        let lines: Vec<_> = raw.lines().filter(|l| !l.trim().is_empty()).collect();
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn intake_session_emits_wrapper_shape_and_round_trips_wrapper_only() {
        let dir = tempfile::tempdir().unwrap();
        let spans_path = dir.path().join("spans.jsonl");
        let session = TracingSession::builder("svc")
            .completed_span_jsonl_path(&spans_path)
            .build()
            .unwrap();
        let subscriber = tracing_subscriber::registry().with(session.layer());
        tracing::subscriber::with_default(subscriber, || {
            let span = tracing::info_span!(
                "request",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/a"
            );
            drop(span);
        });
        assert!(session.snapshot_run().is_ok());
        assert!(!spans_path.exists());
        let _ = futures_executor::block_on(session.shutdown()).unwrap();
        let raw = std::fs::read_to_string(&spans_path).unwrap();
        let lines: Vec<_> = raw.lines().filter(|l| !l.trim().is_empty()).collect();
        assert_eq!(lines.len(), 1);
        let value: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(value["format"], "tailtriage.tracing-span.v1");
        assert!(value["span"].is_object());
        assert_eq!(value["span"]["name"], "request");
        assert!(value["span"]["started_at_unix_ms"].is_number());
        assert!(value["span"]["finished_at_unix_ms"].is_number());
        assert!(value["span"]["duration_us"].is_number());
        assert_eq!(value["span"]["fields"]["tt.kind"], "request");
        assert_eq!(value["span"]["fields"]["tt.request_id"], "r1");
        assert_eq!(value["span"]["fields"]["tt.route"], "/a");

        let imported = crate::jsonl::import_jsonl_path_with_mode(
            &spans_path,
            ImportOptions::new("svc"),
            crate::jsonl::JsonlParseMode::TailtriageWrapperOnly,
        )
        .unwrap();
        assert_eq!(imported.run().requests.len(), 1);
        assert_eq!(imported.run().requests[0].request_id, "r1");
        assert_eq!(imported.run().requests[0].route, "/a");
    }

    #[test]
    fn session_shutdown_writes_retained_original_sources_directly() {
        let dir = tempfile::tempdir().unwrap();
        let spans_path = dir.path().join("spans.jsonl");
        let run_path = dir.path().join("run.json");
        let session = TracingSession::builder("svc")
            .completed_span_jsonl_path(&spans_path)
            .run_json_path(&run_path)
            .build()
            .unwrap();
        let subscriber = tracing_subscriber::registry().with(session.layer());
        tracing::subscriber::with_default(subscriber, || {
            drop(tracing::info_span!(
                "request-source-name",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/a",
                tt.outcome = "ok",
                custom_source = "written"
            ));
        });

        let snapshot = session.snapshot_run().unwrap();
        assert_eq!(snapshot.retained_sources().len(), 1);
        assert_eq!(snapshot.retained_sources()[0].name(), "request-source-name");
        assert_eq!(
            snapshot.retained_sources()[0].fields().get("custom_source"),
            Some(&FieldValue::String("written".to_owned()))
        );

        let imported = futures_executor::block_on(session.shutdown()).unwrap();
        assert_eq!(imported.retained_sources(), snapshot.retained_sources());
        assert_eq!(imported.run(), snapshot.run());
        assert_eq!(imported.warnings(), snapshot.warnings());

        let written = crate::jsonl::import_jsonl_path_with_mode(
            &spans_path,
            ImportOptions::new("svc"),
            crate::jsonl::JsonlParseMode::TailtriageWrapperOnly,
        )
        .unwrap();
        assert_eq!(written.run().requests, imported.run().requests);
        assert_eq!(written.run().stages, imported.run().stages);
        assert_eq!(written.run().queues, imported.run().queues);
        assert_eq!(written.run().truncation, imported.run().truncation);
        assert_eq!(written.retained_sources().len(), 1);
        assert_eq!(written.retained_sources()[0].name(), "request-source-name");
        assert_eq!(
            written.retained_sources()[0].fields().get("custom_source"),
            Some(&FieldValue::String("written".to_owned()))
        );
        let run_json: tailtriage_core::Run =
            serde_json::from_slice(&std::fs::read(&run_path).unwrap()).unwrap();
        assert_eq!(&run_json, imported.run());
    }

    #[test]
    fn completed_jsonl_matches_retained_run_counts() {
        let dir = tempfile::tempdir().unwrap();
        let spans_path = dir.path().join("spans.jsonl");
        let session = TracingSession::builder("svc")
            .completed_span_jsonl_path(&spans_path)
            .capture_limits(tailtriage_core::CaptureLimits {
                max_requests: 1,
                max_stages: 1,
                max_queues: 1,
                ..tailtriage_core::CaptureMode::Light.core_defaults()
            })
            .build()
            .unwrap();
        let subscriber = tracing_subscriber::registry().with(session.layer());
        tracing::subscriber::with_default(subscriber, || {
            drop(tracing::info_span!(
                "request",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/a"
            ));
            drop(tracing::info_span!(
                "request",
                tt.kind = "request",
                tt.request_id = "r2",
                tt.route = "/b"
            ));
        });
        let snapshot = session.snapshot_run().unwrap();
        assert_eq!(snapshot.run().requests.len(), 1);
        assert_eq!(snapshot.run().truncation.dropped_requests, 1);
        futures_executor::block_on(session.shutdown()).unwrap();
        let imported = crate::jsonl::import_jsonl_path_with_mode(
            &spans_path,
            ImportOptions::new("svc"),
            crate::jsonl::JsonlParseMode::TailtriageWrapperOnly,
        )
        .unwrap();
        assert_eq!(imported.run().requests.len(), 1);
        assert_eq!(imported.run().stages.len(), 0);
        assert_eq!(imported.run().queues.len(), 0);
    }

    fn assert_no_temp_artifacts(dir: &Path) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let file_name = entry.file_name();
            let file_name = file_name.to_string_lossy();
            assert!(
                !file_name.contains("tailtriage-tmp"),
                "unexpected completed-span JSONL temp artifact: {}",
                entry.path().display()
            );
            assert!(
                !file_name.contains(".tmp-"),
                "unexpected Run JSON temp artifact: {}",
                entry.path().display()
            );
        }
    }

    #[test]
    fn intake_session_write_failure_returns_io_on_shutdown() {
        let dir = tempfile::tempdir().unwrap();
        let parent_file = dir.path().join("missing");
        std::fs::write(&parent_file, "not-a-directory").unwrap();
        let bad_path = parent_file.join("spans.jsonl");
        let session = TracingSession::builder("svc")
            .completed_span_jsonl_path(&bad_path)
            .strict(true)
            .build()
            .unwrap();
        let subscriber = tracing_subscriber::registry().with(session.layer());
        tracing::subscriber::with_default(subscriber, || {
            drop(tracing::info_span!(
                "request",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/ok"
            ));
        });
        let err = futures_executor::block_on(session.shutdown()).unwrap_err();
        assert!(matches!(err, ImportError::Io { .. }));
        assert!(err
            .to_string()
            .contains("create completed span jsonl parent directory"));
    }

    #[test]
    fn intake_session_non_strict_write_failure_returns_io_on_shutdown() {
        let dir = tempfile::tempdir().unwrap();
        let parent_file = dir.path().join("missing");
        std::fs::write(&parent_file, "not-a-directory").unwrap();
        let bad_path = parent_file.join("spans.jsonl");
        let session = TracingSession::builder("svc")
            .completed_span_jsonl_path(&bad_path)
            .build()
            .unwrap();
        let subscriber = tracing_subscriber::registry().with(session.layer());
        tracing::subscriber::with_default(subscriber, || {
            drop(tracing::info_span!(
                "request",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/ok"
            ));
        });

        let err = futures_executor::block_on(session.shutdown()).unwrap_err();
        assert!(matches!(err, ImportError::Io { .. }));
        assert!(err
            .to_string()
            .contains("create completed span jsonl parent directory"));
    }

    #[test]
    fn intake_session_run_json_path_writes_valid_run_json() {
        let dir = tempfile::tempdir().unwrap();
        let run_path = dir.path().join("run.json");
        let session = TracingSession::builder("svc")
            .run_json_path(&run_path)
            .build()
            .unwrap();
        let subscriber = tracing_subscriber::registry().with(session.layer());
        tracing::subscriber::with_default(subscriber, || {
            drop(tracing::info_span!(
                "request",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/a"
            ));
        });
        futures_executor::block_on(session.shutdown()).unwrap();
        assert!(run_path.exists());
        let run: tailtriage_core::Run =
            serde_json::from_slice(&std::fs::read(&run_path).unwrap()).unwrap();
        assert_eq!(run.requests.len(), 1);
    }

    #[test]
    fn session_shutdown_succeeds_when_request_span_handle_is_dropped_before_shutdown() {
        let dir = tempfile::tempdir().unwrap();
        let run_path = dir.path().join("run.json");
        let session = TracingSession::builder("svc")
            .run_json_path(&run_path)
            .build()
            .unwrap();
        let subscriber = tracing_subscriber::registry().with(session.layer());
        tracing::subscriber::with_default(subscriber, || {
            let _request_guard = tracing::info_span!(
                "request",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/a"
            )
            .entered();
        });

        futures_executor::block_on(session.shutdown()).unwrap();
        assert!(run_path.exists());
    }

    #[test]
    fn session_shutdown_rejects_open_request_span_for_persisted_output() {
        let dir = tempfile::tempdir().unwrap();
        let run_path = dir.path().join("run.json");
        let session = TracingSession::builder("svc")
            .run_json_path(&run_path)
            .build()
            .unwrap();
        let subscriber = tracing_subscriber::registry().with(session.layer());
        let mut open_span = None;
        tracing::subscriber::with_default(subscriber, || {
            let span = tracing::info_span!(
                "request",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/a"
            );
            {
                let _request_guard = span.enter();
            }
            open_span = Some(span);
        });

        let span = open_span.expect("span handle retained across shutdown");
        let err = futures_executor::block_on(session.shutdown()).unwrap_err();
        assert!(matches!(
            err,
            ImportError::ZeroRequestArtifactWithWarnings { .. }
                | ImportError::ZeroRequestArtifact { .. }
        ));
        let message = err.to_string();
        assert!(
            message.contains("tracing import produced zero request events")
                || message.contains("open candidate span(s)")
        );
        assert!(!run_path.exists());
        drop(span);
    }

    #[test]
    fn intake_session_run_json_path_creates_nested_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let run_path = dir.path().join("nested/out/run.json");
        let session = TracingSession::builder("svc")
            .run_json_path(&run_path)
            .build()
            .unwrap();
        let subscriber = tracing_subscriber::registry().with(session.layer());
        tracing::subscriber::with_default(subscriber, || {
            drop(tracing::info_span!(
                "request",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/a"
            ));
        });
        futures_executor::block_on(session.shutdown()).unwrap();
        assert!(run_path.exists());
    }

    #[test]
    fn intake_session_run_json_path_rejects_zero_requests_without_creating_file() {
        let dir = tempfile::tempdir().unwrap();
        let run_path = dir.path().join("run.json");
        let session = TracingSession::builder("svc")
            .run_json_path(&run_path)
            .build()
            .unwrap();
        let err = futures_executor::block_on(session.shutdown()).unwrap_err();
        assert!(matches!(err, ImportError::ZeroRequestArtifact { .. }));
        assert!(!run_path.exists());
    }

    #[test]
    fn intake_session_zero_request_persisted_error_includes_intake_warnings() {
        let dir = tempfile::tempdir().unwrap();
        let run_path = dir.path().join("run.json");
        let session = TracingSession::builder("svc")
            .run_json_path(&run_path)
            .build()
            .unwrap();
        let subscriber = tracing_subscriber::registry().with(session.layer());
        tracing::subscriber::with_default(subscriber, || {
            drop(tracing::info_span!(
                "bad-kind",
                tt.kind = "wat",
                tt.request_id = "r1",
                tt.route = "/ignored"
            ));
        });

        let err = futures_executor::block_on(session.shutdown()).unwrap_err();
        assert!(matches!(
            err,
            ImportError::ZeroRequestArtifactWithWarnings { .. }
        ));
        let message = err.to_string();
        assert!(message.contains("tracing import produced zero request events"));
        assert!(message.contains("warnings observed during tracing intake:"));
        assert!(message.contains("invalid tt.kind"));
        assert!(!run_path.exists());
    }

    #[test]
    fn shutdown_with_completed_span_jsonl_only_and_zero_requests_writes_no_artifact() {
        let dir = tempfile::tempdir().unwrap();
        let spans_path = dir.path().join("spans.jsonl");
        let session = TracingSession::builder("svc")
            .completed_span_jsonl_path(&spans_path)
            .build()
            .unwrap();
        let err = futures_executor::block_on(session.shutdown()).unwrap_err();
        assert!(matches!(err, ImportError::ZeroRequestArtifact { .. }));
        assert!(!spans_path.exists());
    }

    #[test]
    fn shutdown_with_no_persisted_paths_and_zero_requests_still_returns_imported_run() {
        let session = TracingSession::builder("svc").build().unwrap();
        let imported = futures_executor::block_on(session.shutdown()).unwrap();
        assert_eq!(imported.run().requests.len(), 0);
    }

    #[test]
    fn intake_session_run_json_path_rejects_zero_requests_without_overwriting_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let run_path = dir.path().join("run.json");
        std::fs::write(&run_path, "keep-me").unwrap();
        let session = TracingSession::builder("svc")
            .run_json_path(&run_path)
            .build()
            .unwrap();
        let err = futures_executor::block_on(session.shutdown()).unwrap_err();
        assert!(matches!(err, ImportError::ZeroRequestArtifact { .. }));
        assert_eq!(std::fs::read_to_string(&run_path).unwrap(), "keep-me");
    }

    #[test]
    fn shutdown_with_both_outputs_and_zero_requests_writes_no_final_or_temp_artifacts() {
        let dir = tempfile::tempdir().unwrap();
        let spans_path = dir.path().join("spans.jsonl");
        let run_path = dir.path().join("run.json");
        let session = TracingSession::builder("svc")
            .completed_span_jsonl_path(&spans_path)
            .run_json_path(&run_path)
            .build()
            .unwrap();
        let err = futures_executor::block_on(session.shutdown()).unwrap_err();
        assert!(matches!(err, ImportError::ZeroRequestArtifact { .. }));
        assert!(!spans_path.exists());
        assert!(!run_path.exists());
        assert_no_temp_artifacts(dir.path());
    }

    #[test]
    fn completed_jsonl_failure_prevents_run_json_transaction() {
        let dir = tempfile::tempdir().unwrap();
        let parent_file = dir.path().join("not-a-directory");
        std::fs::write(&parent_file, "not-a-directory").unwrap();
        let spans_path = parent_file.join("spans.jsonl");
        let run_path = dir.path().join("run.json");
        std::fs::write(&run_path, "run-json-must-not-change").unwrap();

        let session = TracingSession::builder("svc")
            .completed_span_jsonl_path(&spans_path)
            .run_json_path(&run_path)
            .build()
            .unwrap();
        let subscriber = tracing_subscriber::registry().with(session.layer());
        tracing::subscriber::with_default(subscriber, || {
            drop(tracing::info_span!(
                "jsonl-failure-request",
                tt.kind = "request",
                tt.request_id = "jsonl-failure-r1",
                tt.route = "/jsonl-failure"
            ));
        });

        let err = futures_executor::block_on(session.shutdown()).unwrap_err();
        match err {
            ImportError::Io {
                operation,
                context,
                reason: _,
            } => {
                assert_eq!(operation, "create completed span jsonl parent directory");
                assert_eq!(context, parent_file.display().to_string());
            }
            other => panic!("unexpected error: {other}"),
        }
        assert_eq!(
            std::fs::read_to_string(&run_path).unwrap(),
            "run-json-must-not-change"
        );
        assert!(!spans_path.exists());
        assert_no_temp_artifacts(dir.path());
    }

    #[test]
    fn completed_jsonl_remains_finalized_when_run_json_write_fails() {
        let dir = tempfile::tempdir().unwrap();
        let spans_path = dir.path().join("spans.jsonl");
        let run_path = dir.path().join("run-json-target");
        std::fs::create_dir(&run_path).unwrap();

        let session = TracingSession::builder("svc")
            .completed_span_jsonl_path(&spans_path)
            .run_json_path(&run_path)
            .build()
            .unwrap();
        let subscriber = tracing_subscriber::registry().with(session.layer());
        tracing::subscriber::with_default(subscriber, || {
            drop(tracing::info_span!(
                "run-json-failure-request",
                tt.kind = "request",
                tt.request_id = "run-json-failure-r1",
                tt.route = "/run-json-failure",
                custom_failure_field = "jsonl-survives"
            ));
        });

        let err = futures_executor::block_on(session.shutdown()).unwrap_err();
        match err {
            ImportError::RunJsonWrite { path, reason: _ } => {
                assert_eq!(path, run_path.display().to_string());
            }
            other => panic!("unexpected error: {other}"),
        }
        assert!(spans_path.is_file());
        assert!(run_path.is_dir());
        assert_no_temp_artifacts(dir.path());

        let decoded = decode_completed_span_jsonl(&spans_path);
        assert_eq!(decoded.len(), 1);
        let span = &decoded[0];
        assert_eq!(span.name(), "run-json-failure-request");
        assert_eq!(
            span.fields().get("tt.request_id"),
            Some(&FieldValue::String("run-json-failure-r1".to_owned()))
        );
        assert_eq!(
            span.fields().get("tt.route"),
            Some(&FieldValue::String("/run-json-failure".to_owned()))
        );
        assert_eq!(
            span.fields().get("custom_failure_field"),
            Some(&FieldValue::String("jsonl-survives".to_owned()))
        );
    }

    #[test]
    fn intake_session_persisted_success_keeps_warnings_and_writes_outputs() {
        let dir = tempfile::tempdir().unwrap();
        let spans_path = dir.path().join("spans.jsonl");
        let run_path = dir.path().join("run.json");
        let session = TracingSession::builder("svc")
            .completed_span_jsonl_path(&spans_path)
            .run_json_path(&run_path)
            .build()
            .unwrap();
        let subscriber = tracing_subscriber::registry().with(session.layer());
        tracing::subscriber::with_default(subscriber, || {
            drop(tracing::info_span!(
                "req",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/ok"
            ));
            drop(tracing::info_span!(
                "bad-kind",
                tt.kind = "wat",
                tt.request_id = "r1",
                tt.route = "/ignored"
            ));
        });

        let imported = futures_executor::block_on(session.shutdown()).unwrap();
        assert_eq!(imported.run().requests.len(), 1);
        assert!(imported
            .warnings()
            .iter()
            .any(|w| w.message().contains("invalid tt.kind")));
        assert!(spans_path.exists());
        assert!(run_path.exists());
    }

    #[test]
    fn completed_span_jsonl_success_writes_final_wrapper_file() {
        let dir = tempfile::tempdir().unwrap();
        let spans_path = dir.path().join("spans.jsonl");
        let session = TracingSession::builder("svc")
            .completed_span_jsonl_path(&spans_path)
            .build()
            .unwrap();
        let subscriber = tracing_subscriber::registry().with(session.layer());
        tracing::subscriber::with_default(subscriber, || {
            drop(tracing::info_span!(
                "request",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/a"
            ));
        });

        futures_executor::block_on(session.shutdown()).unwrap();
        assert!(spans_path.exists());
        let raw = std::fs::read_to_string(&spans_path).unwrap();
        let lines: Vec<_> = raw.lines().filter(|line| !line.trim().is_empty()).collect();
        assert_eq!(lines.len(), 1);
        let value: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(value["format"], "tailtriage.tracing-span.v1");
        assert!(value["span"].is_object());
    }

    #[test]
    fn completed_span_jsonl_path_creates_nested_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let spans_path = dir.path().join("nested/out/spans.jsonl");
        let session = TracingSession::builder("svc")
            .completed_span_jsonl_path(&spans_path)
            .build()
            .unwrap();
        let subscriber = tracing_subscriber::registry().with(session.layer());
        tracing::subscriber::with_default(subscriber, || {
            drop(tracing::info_span!(
                "request",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/a"
            ));
        });
        futures_executor::block_on(session.shutdown()).unwrap();
        assert!(spans_path.exists());
    }

    #[test]
    fn create_output_parent_dir_skips_filename_only_path() {
        create_output_parent_dir(Path::new("run.json"), "create run json parent directory")
            .unwrap();
    }

    #[test]
    fn unrelated_spans_are_ignored_by_completed_span_jsonl_writer() {
        let dir = tempfile::tempdir().unwrap();
        let spans_path = dir.path().join("spans.jsonl");
        let session = TracingSession::builder("svc")
            .completed_span_jsonl_path(&spans_path)
            .build()
            .unwrap();
        let subscriber = tracing_subscriber::registry().with(session.layer());
        tracing::subscriber::with_default(subscriber, || {
            drop(tracing::info_span!("unrelated", user = 1_u64));
            drop(tracing::info_span!(
                "request",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/a"
            ));
        });
        futures_executor::block_on(session.shutdown()).unwrap();
        let raw = std::fs::read_to_string(&spans_path).unwrap();
        let lines: Vec<_> = raw.lines().filter(|l| !l.trim().is_empty()).collect();
        assert_eq!(lines.len(), 1);
        let line = lines[0];
        assert!(line.contains("\"tt.request_id\":\"r1\""));
        assert!(!line.contains("unrelated"));
    }

    #[test]
    fn completed_jsonl_excludes_malformed_and_orphan_stage_queue_spans() {
        let dir = tempfile::tempdir().unwrap();
        let spans_path = dir.path().join("spans.jsonl");
        let session = TracingSession::builder("svc")
            .completed_span_jsonl_path(&spans_path)
            .build()
            .unwrap();
        let subscriber = tracing_subscriber::registry().with(session.layer());
        tracing::subscriber::with_default(subscriber, || {
            drop(tracing::info_span!(
                "bad-kind",
                tt.kind = tracing::field::Empty,
                tt.request_id = "bad-1",
                tt.route = "/bad"
            ));
            drop(tracing::info_span!(
                "orphan-stage",
                tt.kind = "stage",
                tt.request_id = "missing-req",
                tt.stage = "db",
                tt.success = true
            ));
            drop(tracing::info_span!(
                "orphan-queue",
                tt.kind = "queue",
                tt.request_id = "missing-req",
                tt.queue = "permits"
            ));
            drop(tracing::info_span!(
                "request",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/ok",
                tt.outcome = "ok"
            ));
        });
        let _ = futures_executor::block_on(session.shutdown()).unwrap();
        let imported = crate::jsonl::import_jsonl_path_with_mode(
            &spans_path,
            ImportOptions::new("svc"),
            crate::jsonl::JsonlParseMode::TailtriageWrapperOnly,
        )
        .unwrap();
        assert_eq!(imported.run().requests.len(), 1);
        assert_eq!(imported.run().stages.len(), 0);
        assert_eq!(imported.run().queues.len(), 0);
    }

    #[test]
    fn intake_session_captures_request_stage_queue() {
        let session = TracingSession::builder("svc").build().unwrap();
        let subscriber = tracing_subscriber::registry().with(session.layer());
        tracing::subscriber::with_default(subscriber, || {
            let req = tracing::info_span!(
                "request",
                tt.kind = "request",
                tt.request_id = "req-1",
                tt.route = "/checkout",
                tt.outcome = "ok"
            );
            let req_guard = req.enter();
            drop(tracing::info_span!(
                "stage",
                tt.kind = "stage",
                tt.request_id = "req-1",
                tt.stage = "db",
                tt.success = true
            ));
            drop(tracing::info_span!(
                "queue",
                tt.kind = "queue",
                tt.request_id = "req-1",
                tt.queue = "admission",
                tt.depth_at_start = 7_u64
            ));
            drop(req_guard);
        });
        let snapshot = session.snapshot_run().unwrap();
        let run = snapshot.run();
        assert_eq!(run.requests.len(), 1);
        assert_eq!(run.stages.len(), 1);
        assert_eq!(run.queues.len(), 1);
        assert!(run.runtime_snapshots.is_empty());
        assert_eq!(run.requests[0].route, "/checkout");
        assert_eq!(run.stages[0].stage, "db");
        assert_eq!(run.queues[0].queue, "admission");
        assert_eq!(run.queues[0].depth_at_start, Some(7));
    }
}
