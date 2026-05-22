use std::collections::BTreeMap;
use std::fmt;
use std::fmt::Write as _;
use std::fs::OpenOptions;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tailtriage_core::{CaptureLimits, CaptureLimitsOverride, CaptureMode, LocalJsonSink, RunSink};

use tracing::field::{Field, Visit};
use tracing::{Id, Subscriber};
use tracing_subscriber::layer::{Context, Layer};

use crate::{
    ensure_persistable_run_has_requests, run_from_span_records, FieldValue, ImportError,
    ImportOptions, ImportedRun, SpanRecord, TT_KIND,
};

/// In-memory recorder for completed tracing spans with `tt.*` fields.
#[derive(Debug, Clone)]
pub struct TracingRecorder {
    state: Arc<Mutex<RecorderState>>,
    options: ImportOptions,
    capture_limits: CaptureLimits,
    limits: RecorderLimits,
}
/// High-level tracing intake bridge for completed `tt.*` spans.
///
/// A session attaches to an existing `tracing_subscriber` registry via [`Self::layer`],
/// captures completed `tt.*` spans, and converts them into standard `tailtriage_core::Run`
/// artifacts through [`Self::snapshot_run`] or [`Self::shutdown`].
///
/// When configured, the session emits stable completed-span JSONL records in the
/// wrapper form `{"format":"tailtriage.tracing-span.v1","span":{...}}` and can
/// optionally write a Run JSON file on shutdown.
///
/// This API is intentionally a tracing intake bridge; it does not implement OTel/OTLP.
/// Tracing-only evidence does not fabricate runtime-pressure snapshots, and suspects
/// in resulting diagnosis reports remain triage leads rather than root-cause proof.
#[derive(Debug, Clone)]
pub struct TracingIntakeSession {
    recorder: TracingRecorder,
    run_json_path: Option<PathBuf>,
}
/// Builder for [`TracingIntakeSession`].
#[derive(Debug, Clone)]
pub struct TracingIntakeSessionBuilder {
    recorder_builder: TracingRecorderBuilder,
    completed_span_jsonl_path: Option<PathBuf>,
    run_json_path: Option<PathBuf>,
}

/// Builder for [`TracingRecorder`].
#[derive(Debug, Clone)]
pub struct TracingRecorderBuilder {
    options: ImportOptions,
    limits: RecorderLimits,
}

/// `tracing_subscriber` layer that feeds completed spans into a [`TracingRecorder`].
#[derive(Debug, Clone)]
pub struct TailtriageLayer {
    state: Arc<Mutex<RecorderState>>,
    capture_limits: CaptureLimits,
    limits: RecorderLimits,
}
/// Default maximum number of concurrently tracked open candidate spans.
pub const DEFAULT_MAX_OPEN_SPANS: usize = 8_192;
/// Configurable in-memory limits for live tracing recorder retention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct RecorderLimits {
    /// Maximum number of concurrently tracked open candidate spans.
    pub max_open_spans: usize,
}
impl Default for RecorderLimits {
    fn default() -> Self {
        Self {
            max_open_spans: DEFAULT_MAX_OPEN_SPANS,
        }
    }
}

#[derive(Debug, Default)]
struct RecorderState {
    open: BTreeMap<u64, OpenSpan>,
    completed: Vec<SpanRecord>,
    retained_requests: usize,
    retained_stages: usize,
    retained_queues: usize,
    dropped_open_spans: u64,
    dropped_completed_requests: u64,
    dropped_completed_stages: u64,
    dropped_completed_queues: u64,
    closed_missing_kind_spans: u64,
    closed_unknown_kind_spans: u64,
    closed_malformed_kind_spans: u64,
    closed_kind_samples: Vec<ClosedKindIssueSample>,
    writer_failure: Option<String>,
    completed_span_writer_path: Option<PathBuf>,
    completed_span_writer: Option<BufWriter<std::fs::File>>,
}

#[derive(Debug)]
struct OpenSpan {
    id: Option<String>,
    parent_id: Option<String>,
    name: String,
    fields: BTreeMap<String, FieldValue>,
    started_at_unix_ms: u64,
    started_instant: Instant,
    is_tt_candidate: bool,
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
    dropped_completed_requests: u64,
    dropped_completed_stages: u64,
    dropped_completed_queues: u64,
    open_candidate_count: u64,
    open_samples: Vec<OpenSpanSample>,
    closed_missing_kind_spans: u64,
    closed_unknown_kind_spans: u64,
    closed_malformed_kind_spans: u64,
    closed_kind_samples: Vec<ClosedKindIssueSample>,
    writer_failure: Option<String>,
}
fn lock_state(state: &Arc<Mutex<RecorderState>>) -> std::sync::MutexGuard<'_, RecorderState> {
    match state.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

impl TracingRecorder {
    /// Creates a builder with required service name metadata.
    pub fn builder(service_name: impl Into<String>) -> TracingRecorderBuilder {
        TracingRecorderBuilder {
            options: ImportOptions::new(service_name),
            limits: RecorderLimits::default(),
        }
    }

    /// Returns a cloneable layer that captures `on_new_span`, `on_record`, and `on_close`.
    #[must_use]
    pub fn layer(&self) -> TailtriageLayer {
        TailtriageLayer {
            state: Arc::clone(&self.state),
            capture_limits: self.capture_limits,
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
        self.flush_completed_span_writer()?;
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
                state.completed.clone(),
                SnapshotStats {
                    dropped_open_spans: state.dropped_open_spans,
                    dropped_completed_requests: state.dropped_completed_requests,
                    dropped_completed_stages: state.dropped_completed_stages,
                    dropped_completed_queues: state.dropped_completed_queues,
                    open_candidate_count: count,
                    open_samples: samples,
                    closed_missing_kind_spans: state.closed_missing_kind_spans,
                    closed_unknown_kind_spans: state.closed_unknown_kind_spans,
                    closed_malformed_kind_spans: state.closed_malformed_kind_spans,
                    closed_kind_samples: state.closed_kind_samples.clone(),
                    writer_failure: state.writer_failure.clone(),
                },
            )
        };
        imported_with_drop_warnings(spans, self.options.clone(), &stats, self.limits)
    }

    fn flush_completed_span_writer(&self) -> Result<(), ImportError> {
        let mut state = lock_state(&self.state);
        if let Some(writer) = state.completed_span_writer.as_mut() {
            if let Err(err) = writer.flush() {
                let msg = format_writer_failure(
                    "flush",
                    state.completed_span_writer_path.as_deref(),
                    &err,
                );
                state.writer_failure = Some(msg.clone());
                if self.options.strict_mode() {
                    return Err(ImportError::Io {
                        operation: "flush completed span jsonl writer",
                        context: state.completed_span_writer_path.as_ref().map_or_else(
                            || "completed-span-jsonl".to_owned(),
                            |p| p.display().to_string(),
                        ),
                        reason: err.to_string(),
                    });
                }
            }
        }
        Ok(())
    }

    /// Consumes this recorder handle and returns a final imported run snapshot.
    ///
    /// Span completion is driven by span close/drop, not enter/exit.
    ///
    /// # Errors
    ///
    /// Returns [`ImportError`] when strict conversion fails.
    pub fn shutdown(self) -> Result<ImportedRun, ImportError> {
        self.snapshot_run()
    }
}

impl TracingIntakeSession {
    /// Creates a tracing intake session builder with required service metadata.
    ///
    /// ```no_run
    /// use tailtriage_tracing::TracingIntakeSession;
    /// use tracing_subscriber::prelude::*;
    ///
    /// let session = TracingIntakeSession::builder("checkout")
    ///     .completed_span_jsonl_path("completed-spans.jsonl")
    ///     .build()
    ///     .expect("session should build");
    ///
    /// let subscriber = tracing_subscriber::registry().with(session.layer());
    /// tracing::subscriber::with_default(subscriber, || {
    ///     let span = tracing::info_span!(
    ///         "request",
    ///         tt.kind = "request",
    ///         tt.request_id = "r1",
    ///         tt.route = "/checkout"
    ///     );
    ///     drop(span);
    /// });
    ///
    /// let _ = session.shutdown().expect("shutdown should succeed");
    /// ```
    pub fn builder(service_name: impl Into<String>) -> TracingIntakeSessionBuilder {
        TracingIntakeSessionBuilder {
            recorder_builder: TracingRecorder::builder(service_name),
            completed_span_jsonl_path: None,
            run_json_path: None,
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
        self.recorder.snapshot_run()
    }
    /// Finalizes intake and optionally writes run JSON when configured.
    ///
    /// # Errors
    ///
    /// Returns an error when conversion fails or when configured run-json output cannot be written.
    pub fn shutdown(self) -> Result<ImportedRun, ImportError> {
        let imported = self.recorder.shutdown()?;
        if let Some(path) = self.run_json_path {
            ensure_persistable_run_has_requests(imported.run())?;
            LocalJsonSink::new(&path)
                .write(imported.run())
                .map_err(|err| ImportError::RunJsonWrite {
                    path: path.display().to_string(),
                    reason: err.to_string(),
                })?;
        }
        Ok(imported)
    }
}
impl TracingIntakeSessionBuilder {
    /// Enables or disables strict mode for conversion warnings.
    #[must_use]
    pub fn strict(mut self, strict: bool) -> Self {
        self.recorder_builder = self.recorder_builder.strict(strict);
        self
    }
    /// Sets capture mode used to resolve live completed-evidence retention limits.
    /// Sets capture mode used to resolve live completed-evidence retention limits.
    #[must_use]
    pub fn mode(mut self, mode: CaptureMode) -> Self {
        self.recorder_builder = self.recorder_builder.mode(mode);
        self
    }
    /// Sets base capture limits used for live completed-evidence retention.
    /// Sets base capture limits used for live completed-evidence retention.
    #[must_use]
    pub fn capture_limits(mut self, capture_limits: CaptureLimits) -> Self {
        self.recorder_builder = self.recorder_builder.capture_limits(capture_limits);
        self
    }
    /// Sets capture-limit overrides applied on top of the selected capture mode.
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
    /// Sets open/completed in-memory retention limits.
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
    /// Enables completed-span JSONL output at the given path.
    ///
    /// Writes completed spans as spans close using the stable wrapper shape.
    /// The file is created or truncated when the session is built.
    #[must_use]
    pub fn completed_span_jsonl_path(mut self, path: impl AsRef<Path>) -> Self {
        self.completed_span_jsonl_path = Some(path.as_ref().to_path_buf());
        self
    }
    /// Enables Run JSON output on shutdown at the given path.
    #[must_use]
    pub fn run_json_path(mut self, path: impl AsRef<Path>) -> Self {
        self.run_json_path = Some(path.as_ref().to_path_buf());
        self
    }
    /// Builds a tracing intake session and opens optional outputs.
    ///
    /// # Errors
    ///
    /// Returns an error when the completed-span JSONL path cannot be opened.
    pub fn build(self) -> Result<TracingIntakeSession, ImportError> {
        let recorder = self.recorder_builder.build();
        if let Some(path) = self.completed_span_jsonl_path {
            let file = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&path)
                .map_err(|err| ImportError::Io {
                    operation: "open completed span jsonl path",
                    context: path.display().to_string(),
                    reason: err.to_string(),
                })?;
            let mut state = lock_state(&recorder.state);
            state.completed_span_writer = Some(BufWriter::new(file));
            state.completed_span_writer_path = Some(path);
        }
        Ok(TracingIntakeSession {
            recorder,
            run_json_path: self.run_json_path,
        })
    }
}

impl TracingRecorderBuilder {
    /// Returns resolved capture limits after applying mode/base/override configuration.
    #[must_use]
    pub fn resolved_capture_limits(&self) -> CaptureLimits {
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
    #[must_use]
    pub fn build(self) -> TracingRecorder {
        let resolved = self.options.resolved_capture_limits();
        TracingRecorder {
            state: Arc::new(Mutex::new(RecorderState::default())),
            options: self.options,
            capture_limits: resolved,
            limits: self.limits,
        }
    }
    /// Sets open/completed in-memory retention limits.
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
        let mut state = lock_state(&self.state);
        if state.open.len() >= self.limits.max_open_spans {
            state.dropped_open_spans = state.dropped_open_spans.saturating_add(1);
            return;
        }
        let open_span = OpenSpan {
            id: Some(id.into_u64().to_string()),
            parent_id,
            name: attrs.metadata().name().to_owned(),
            fields: visitor.fields,
            started_at_unix_ms: tailtriage_core::unix_time_ms(),
            started_instant: Instant::now(),
            is_tt_candidate: metadata_candidate || initial_candidate,
        };
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
        let mut state = lock_state(&self.state);
        if let Some(open) = state.open.remove(&id.into_u64()) {
            let kind = classify_kind(&open.fields);
            if let Err(reason) = kind {
                record_invalid_kind_issue(&mut state, &open, reason);
                return;
            }
            let kind = kind.expect("ok kind");
            let mut record = SpanRecord::new(
                open.name,
                open.started_at_unix_ms,
                tailtriage_core::unix_time_ms(),
            );
            let duration_us =
                u64::try_from(open.started_instant.elapsed().as_micros()).unwrap_or(u64::MAX);
            record = record.duration_us(duration_us);
            if let Some(span_id) = open.id {
                record = record.id(span_id);
            }
            if let Some(parent_id) = open.parent_id {
                record = record.parent_id(parent_id);
            }
            for (k, v) in open.fields {
                record = record.field(k, v);
            }
            if let Some(writer) = state.completed_span_writer.as_mut() {
                let wrapped = serde_json::json!({
                    "format": "tailtriage.tracing-span.v1",
                    "span": record,
                });
                match serde_json::to_writer(&mut *writer, &wrapped) {
                    Ok(()) => match writer.write_all(b"\n") {
                        Ok(()) => {}
                        Err(err) => {
                            state.writer_failure = Some(format_writer_failure(
                                "write newline",
                                state.completed_span_writer_path.as_deref(),
                                &err,
                            ));
                        }
                    },
                    Err(err) => {
                        state.writer_failure = Some(format_writer_failure(
                            "write span record",
                            state.completed_span_writer_path.as_deref(),
                            &err,
                        ));
                    }
                }
            }
            let keep = retain_completed_for_kind(&mut state, kind, &self.capture_limits);
            if !keep {
                return;
            }
            state.completed.push(record);
        }
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

fn retain_completed_for_kind(
    state: &mut RecorderState,
    kind: &str,
    capture_limits: &CaptureLimits,
) -> bool {
    match kind {
        "request" if state.retained_requests < capture_limits.max_requests => {
            state.retained_requests += 1;
            true
        }
        "stage" if state.retained_stages < capture_limits.max_stages => {
            state.retained_stages += 1;
            true
        }
        "queue" if state.retained_queues < capture_limits.max_queues => {
            state.retained_queues += 1;
            true
        }
        "request" => {
            state.dropped_completed_requests = state.dropped_completed_requests.saturating_add(1);
            false
        }
        "stage" => {
            state.dropped_completed_stages = state.dropped_completed_stages.saturating_add(1);
            false
        }
        _ => {
            state.dropped_completed_queues = state.dropped_completed_queues.saturating_add(1);
            false
        }
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

fn format_writer_failure(
    operation: &str,
    path: Option<&Path>,
    err: &dyn std::error::Error,
) -> String {
    let path_text = path.map_or_else(|| "<unknown>".to_owned(), |p| p.display().to_string());
    format!("completed-span JSONL writer failed to {operation} at {path_text}: {err}")
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
    if stats.dropped_open_spans > 0 {
        messages.push(format!(
            "live recorder dropped {} open candidate span(s) because max_open_spans={} was reached; raise max_open_spans or reduce capture scope",
            stats.dropped_open_spans, limits.max_open_spans
        ));
    }
    let dropped_completed_total = stats.dropped_completed_requests
        + stats.dropped_completed_stages
        + stats.dropped_completed_queues;
    if dropped_completed_total > 0 {
        messages.push(format!("live recorder dropped completed evidence due to capture limits (requests={}, stages={}, queues={})", stats.dropped_completed_requests, stats.dropped_completed_stages, stats.dropped_completed_queues));
    }
    if let Some(reason) = &stats.writer_failure {
        messages.push(format!(
            "live recorder failed writing completed-span JSONL output: {reason}"
        ));
    }
}

fn append_non_strict_drop_warnings(
    run: &mut tailtriage_core::Run,
    warnings: &mut Vec<crate::ImportWarning>,
    stats: &SnapshotStats,
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
    if stats.dropped_open_spans > 0 {
        let msg = format!(
            "live recorder dropped {} candidate spans because max_open_spans was reached",
            stats.dropped_open_spans
        );
        run.metadata.lifecycle_warnings.push(msg.clone());
        warnings.push(crate::ImportWarning::new(msg));
    }
    let dropped_completed_total = stats.dropped_completed_requests
        + stats.dropped_completed_stages
        + stats.dropped_completed_queues;
    if dropped_completed_total > 0 {
        let msg = format!("live recorder dropped completed evidence due to capture limits (requests={}, stages={}, queues={})", stats.dropped_completed_requests, stats.dropped_completed_stages, stats.dropped_completed_queues);
        run.metadata.lifecycle_warnings.push(msg.clone());
        warnings.push(crate::ImportWarning::new(msg));
        run.truncation.dropped_requests = run
            .truncation
            .dropped_requests
            .saturating_add(stats.dropped_completed_requests);
        run.truncation.dropped_stages = run
            .truncation
            .dropped_stages
            .saturating_add(stats.dropped_completed_stages);
        run.truncation.dropped_queues = run
            .truncation
            .dropped_queues
            .saturating_add(stats.dropped_completed_queues);
    }
    if stats.dropped_open_spans > 0 || dropped_completed_total > 0 {
        run.truncation.limits_hit = true;
    }
    if let Some(reason) = &stats.writer_failure {
        let msg = format!("live recorder failed writing completed-span JSONL output: {reason}");
        run.metadata.lifecycle_warnings.push(msg.clone());
        warnings.push(crate::ImportWarning::new(msg));
    }
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
        && stats.dropped_completed_requests == 0
        && stats.dropped_completed_stages == 0
        && stats.dropped_completed_queues == 0
        && stats.open_candidate_count == 0
        && stats.closed_missing_kind_spans == 0
        && stats.closed_unknown_kind_spans == 0
        && stats.closed_malformed_kind_spans == 0
        && stats.writer_failure.is_none()
    {
        return Ok(imported);
    }

    let (mut run, mut warnings) = imported.into_parts();
    append_non_strict_drop_warnings(&mut run, &mut warnings, stats);
    Ok(ImportedRun::new(run, warnings))
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

    fn with_recorder<T>(f: impl FnOnce(&TracingRecorder) -> T) -> T {
        let recorder = TracingRecorder::builder("svc").run_id("rid").build();
        let subscriber = tracing_subscriber::registry().with(recorder.layer());
        tracing::subscriber::with_default(subscriber, || f(&recorder))
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
    fn stage_span_collected() {
        with_recorder(|recorder| {
            let request = tracing::info_span!(
                "request",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/a"
            );
            drop(request);
            let span = tracing::info_span!(
                "stage",
                tt.kind = "stage",
                tt.request_id = "r1",
                tt.stage = "db"
            );
            drop(span);
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
            );
            drop(request);
            let span = tracing::info_span!(
                "queue",
                tt.kind = "queue",
                tt.request_id = "r1",
                tt.queue = "permits"
            );
            drop(span);
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

        let recorder = TracingRecorder::builder("svc").build();
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
        let recorder = TracingRecorder::builder("checkout-service")
            .service_version("1.2.3")
            .run_id("run-42")
            .strict(false)
            .build();
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
        let recorder = TracingRecorder::builder("svc").strict(true).build();
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
    fn shutdown_output_is_analyzable_and_has_no_runtime_snapshots() {
        with_recorder(|recorder| {
            let request = tracing::info_span!(
                "request",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/checkout"
            );
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
            drop(request);
            drop(queue);
            drop(stage);

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
    fn completed_span_saturation_emits_warning_and_sets_limits_hit() {
        let recorder = TracingRecorder::builder("svc")
            .capture_limits(tailtriage_core::CaptureLimits {
                max_requests: 1,
                max_stages: 1,
                max_queues: 1,
                ..tailtriage_core::CaptureMode::Light.core_defaults()
            })
            .build();
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
        assert!(imported.warnings().iter().any(|w| w
            .message()
            .contains("dropped completed evidence due to capture limits")));
        assert!(imported
            .run()
            .metadata
            .lifecycle_warnings
            .iter()
            .any(|w| w.contains("dropped completed evidence due to capture limits")));
        assert!(imported.run().truncation.limits_hit);
        assert_eq!(imported.run().requests[0].request_id, "r1");
    }

    #[test]
    fn strict_mode_errors_when_completed_retention_drops_completed_spans() {
        let recorder = TracingRecorder::builder("svc")
            .strict(true)
            .capture_limits(tailtriage_core::CaptureLimits {
                max_requests: 1,
                max_stages: 1,
                max_queues: 1,
                ..tailtriage_core::CaptureMode::Light.core_defaults()
            })
            .build();
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
        let err = recorder
            .snapshot_run()
            .expect_err("strict should reject retention drops");
        match err {
            ImportError::StrictViolation(message) => {
                assert!(message.contains("dropped completed evidence due to capture limits"));
                assert!(message.contains("requests=1"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn strict_mode_errors_when_max_open_spans_drops_candidate_spans() {
        let recorder = TracingRecorder::builder("svc")
            .strict(true)
            .max_open_spans(1)
            .build();
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
        let recorder = TracingRecorder::builder("svc")
            .strict(true)
            .capture_limits(tailtriage_core::CaptureLimits {
                max_requests: 1,
                max_stages: 1,
                max_queues: 1,
                ..tailtriage_core::CaptureMode::Light.core_defaults()
            })
            .build();
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
                assert!(message.contains("dropped completed evidence due to capture limits"));
                assert!(message.contains("requests=1"));
                assert!(message.contains("tt.route"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn open_span_saturation_emits_warning_and_sets_limits_hit() {
        let recorder = TracingRecorder::builder("svc").max_open_spans(1).build();
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
        let recorder = TracingRecorder::builder("svc")
            .max_open_spans(1)
            .capture_limits(tailtriage_core::CaptureLimits {
                max_requests: 1,
                max_stages: 1,
                max_queues: 1,
                ..tailtriage_core::CaptureMode::Light.core_defaults()
            })
            .build();
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
        assert!(imported.warnings().iter().any(|w| {
            w.message()
                .contains("dropped completed evidence due to capture limits")
        }));
        assert!(imported
            .run()
            .metadata
            .lifecycle_warnings
            .iter()
            .any(|w| w.contains("max_open_spans")));
        assert!(imported
            .run()
            .metadata
            .lifecycle_warnings
            .iter()
            .any(|w| w.contains("dropped completed evidence due to capture limits")));
        assert!(imported.run().truncation.limits_hit);
    }

    #[test]
    fn unrelated_spans_do_not_consume_open_limit() {
        let recorder = TracingRecorder::builder("svc").max_open_spans(1).build();
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
    fn empty_service_name_builder_errors_on_snapshot() {
        let recorder = TracingRecorder::builder(" ").build();
        let err = recorder.snapshot_run().unwrap_err();
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
        let recorder = TracingRecorder::builder("svc").strict(true).build();
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
        let recorder = TracingRecorder::builder("svc").strict(true).build();
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
        let recorder = TracingRecorder::builder("svc").strict(true).build();
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
        let recorder = TracingRecorder::builder("svc").strict(true).build();
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
        let recorder = TracingRecorder::builder("svc").strict(true).build();
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
    fn intake_session_wrapper_jsonl_and_truncate_behavior() {
        let dir = tempfile::tempdir().unwrap();
        let spans_path = dir.path().join("spans.jsonl");
        std::fs::write(
            &spans_path,
            r#"{"format":"tailtriage.tracing-span.v1","span":{"name":"old","started_at_unix_ms":1,"finished_at_unix_ms":2,"fields":{"tt.kind":"request","tt.request_id":"old","tt.route":"/old"}}}"#,
        )
        .unwrap();
        let session = TracingIntakeSession::builder("svc")
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
        let imported = session.shutdown().unwrap();
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
        let session = TracingIntakeSession::builder("svc")
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
        session.snapshot_run().unwrap();
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
    fn streaming_jsonl_is_independent_of_in_memory_completed_retention() {
        let dir = tempfile::tempdir().unwrap();
        let spans_path = dir.path().join("spans.jsonl");
        let session = TracingIntakeSession::builder("svc")
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
        assert!(snapshot.warnings().iter().any(|w| w
            .message()
            .contains("dropped completed evidence due to capture limits")));
        let raw = std::fs::read_to_string(&spans_path).unwrap();
        let lines: Vec<_> = raw.lines().filter(|l| !l.trim().is_empty()).collect();
        assert_eq!(lines.len(), 2);
        let imported = crate::jsonl::import_jsonl_path_with_mode(
            &spans_path,
            ImportOptions::new("svc"),
            crate::jsonl::JsonlParseMode::TailtriageWrapperOnly,
        )
        .unwrap();
        assert_eq!(imported.run().requests.len(), 2);
        let ids: Vec<_> = imported
            .run()
            .requests
            .iter()
            .map(|r| r.request_id.as_str())
            .collect();
        assert!(ids.contains(&"r1"));
        assert!(ids.contains(&"r2"));
    }

    #[test]
    fn intake_session_build_writer_open_failure_returns_io() {
        let dir = tempfile::tempdir().unwrap();
        let bad_path = dir.path().join("missing").join("spans.jsonl");
        let err = TracingIntakeSession::builder("svc")
            .completed_span_jsonl_path(&bad_path)
            .build()
            .unwrap_err();
        assert!(matches!(err, ImportError::Io { .. }));
        assert!(err.to_string().contains("spans.jsonl"));
    }

    #[test]
    fn intake_session_run_json_path_writes_valid_run_json() {
        let dir = tempfile::tempdir().unwrap();
        let run_path = dir.path().join("run.json");
        let session = TracingIntakeSession::builder("svc")
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
        session.shutdown().unwrap();
        assert!(run_path.exists());
        let run: tailtriage_core::Run =
            serde_json::from_slice(&std::fs::read(&run_path).unwrap()).unwrap();
        assert_eq!(run.requests.len(), 1);
    }

    #[test]
    fn intake_session_run_json_path_rejects_zero_requests_without_creating_file() {
        let dir = tempfile::tempdir().unwrap();
        let run_path = dir.path().join("run.json");
        let session = TracingIntakeSession::builder("svc")
            .run_json_path(&run_path)
            .build()
            .unwrap();
        let err = session.shutdown().unwrap_err();
        assert!(matches!(err, ImportError::ZeroRequestArtifact { .. }));
        assert!(!run_path.exists());
    }

    #[test]
    fn intake_session_run_json_path_rejects_zero_requests_without_overwriting_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let run_path = dir.path().join("run.json");
        std::fs::write(&run_path, "keep-me").unwrap();
        let session = TracingIntakeSession::builder("svc")
            .run_json_path(&run_path)
            .build()
            .unwrap();
        let err = session.shutdown().unwrap_err();
        assert!(matches!(err, ImportError::ZeroRequestArtifact { .. }));
        assert_eq!(std::fs::read_to_string(&run_path).unwrap(), "keep-me");
    }

    #[test]
    fn unrelated_spans_are_ignored_by_completed_span_jsonl_writer() {
        let dir = tempfile::tempdir().unwrap();
        let spans_path = dir.path().join("spans.jsonl");
        let session = TracingIntakeSession::builder("svc")
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
        session.shutdown().unwrap();
        let raw = std::fs::read_to_string(&spans_path).unwrap();
        let lines: Vec<_> = raw.lines().filter(|l| !l.trim().is_empty()).collect();
        assert_eq!(lines.len(), 1);
        let line = lines[0];
        assert!(line.contains("\"tt.request_id\":\"r1\""));
        assert!(!line.contains("unrelated"));
    }

    #[test]
    fn intake_session_captures_request_stage_queue() {
        let session = TracingIntakeSession::builder("svc").build().unwrap();
        let subscriber = tracing_subscriber::registry().with(session.layer());
        tracing::subscriber::with_default(subscriber, || {
            drop(tracing::info_span!(
                "request",
                tt.kind = "request",
                tt.request_id = "req-1",
                tt.route = "/checkout",
                tt.outcome = "ok"
            ));
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
