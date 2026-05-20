use std::collections::BTreeMap;
use std::fmt;
use std::fmt::Write as _;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use tracing::field::{Field, Visit};
use tracing::{Id, Subscriber};
use tracing_subscriber::layer::{Context, Layer};

use crate::{
    run_from_span_records, FieldValue, ImportError, ImportOptions, ImportedRun, SpanRecord, TT_KIND,
};

/// In-memory recorder for completed tracing spans with `tt.*` fields.
#[derive(Debug, Clone)]
pub struct TracingRecorder {
    state: Arc<Mutex<RecorderState>>,
    options: ImportOptions,
    limits: RecorderLimits,
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
    limits: RecorderLimits,
}
/// Default maximum number of concurrently tracked open candidate spans.
pub const DEFAULT_MAX_OPEN_SPANS: usize = 8_192;
/// Default maximum number of retained completed candidate spans.
pub const DEFAULT_MAX_COMPLETED_SPANS: usize = 10_000;
/// Configurable in-memory limits for live tracing recorder retention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct RecorderLimits {
    /// Maximum number of concurrently tracked open candidate spans.
    pub max_open_spans: usize,
    /// Maximum number of retained completed candidate spans.
    pub max_completed_spans: usize,
}
impl Default for RecorderLimits {
    fn default() -> Self {
        Self {
            max_open_spans: DEFAULT_MAX_OPEN_SPANS,
            max_completed_spans: DEFAULT_MAX_COMPLETED_SPANS,
        }
    }
}

#[derive(Debug, Default)]
struct RecorderState {
    open: BTreeMap<u64, OpenSpan>,
    completed: Vec<SpanRecord>,
    dropped_open_spans: u64,
    dropped_completed_spans: u64,
    closed_missing_kind_spans: u64,
    closed_missing_kind_samples: Vec<ClosedMissingKindSample>,
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
struct ClosedMissingKindSample {
    name: String,
    span_id: Option<String>,
    tt_request_id: Option<String>,
}

struct SnapshotStats {
    dropped_open_spans: u64,
    dropped_completed_spans: u64,
    open_candidate_count: u64,
    open_samples: Vec<OpenSpanSample>,
    closed_missing_kind_spans: u64,
    closed_missing_kind_samples: Vec<ClosedMissingKindSample>,
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
            limits: self.limits,
        }
    }

    /// Returns a non-consuming snapshot imported run from currently completed spans.
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
                state.completed.clone(),
                SnapshotStats {
                    dropped_open_spans: state.dropped_open_spans,
                    dropped_completed_spans: state.dropped_completed_spans,
                    open_candidate_count: count,
                    open_samples: samples,
                    closed_missing_kind_spans: state.closed_missing_kind_spans,
                    closed_missing_kind_samples: state.closed_missing_kind_samples.clone(),
                },
            )
        };
        imported_with_drop_warnings(spans, self.options.clone(), &stats, self.limits)
    }

    /// Consumes this recorder handle and converts currently completed spans into a final imported run.
    ///
    /// Span completion is still driven by span close/drop events (`on_close`), not by enter/exit transitions.
    ///
    /// # Errors
    ///
    /// Returns [`ImportError`] when strict conversion fails.
    pub fn shutdown(self) -> Result<ImportedRun, ImportError> {
        self.snapshot_run()
    }
}

impl TracingRecorderBuilder {
    /// Sets service version metadata.
    #[must_use]
    pub fn service_version(mut self, service_version: impl Into<String>) -> Self {
        self.options = self.options.service_version(service_version);
        self
    }

    /// Sets explicit run-id metadata.
    #[must_use]
    pub fn run_id(mut self, run_id: impl Into<String>) -> Self {
        self.options = self.options.run_id(run_id);
        self
    }

    /// Enables or disables strict conversion mode.
    #[must_use]
    pub fn strict(mut self, strict: bool) -> Self {
        self.options = self.options.strict(strict);
        self
    }

    /// Builds a recorder instance.
    #[must_use]
    pub fn build(self) -> TracingRecorder {
        TracingRecorder {
            state: Arc::new(Mutex::new(RecorderState::default())),
            options: self.options,
            limits: self.limits,
        }
    }
    /// Sets both open/completed in-memory span retention limits.
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
    /// Sets maximum number of retained completed candidate spans.
    #[must_use]
    pub fn max_completed_spans(mut self, max_completed_spans: usize) -> Self {
        self.limits.max_completed_spans = max_completed_spans;
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
            if !open.fields.contains_key(TT_KIND) {
                if open.is_tt_candidate {
                    state.closed_missing_kind_spans =
                        state.closed_missing_kind_spans.saturating_add(1);
                    if state.closed_missing_kind_samples.len() < 3 {
                        state
                            .closed_missing_kind_samples
                            .push(ClosedMissingKindSample {
                                name: open.name.clone(),
                                span_id: open.id.clone(),
                                tt_request_id: scalar_field_string(
                                    open.fields.get("tt.request_id"),
                                ),
                            });
                    }
                }
                return;
            }
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
            if state.completed.len() >= self.limits.max_completed_spans {
                state.dropped_completed_spans = state.dropped_completed_spans.saturating_add(1);
                return;
            }
            state.completed.push(record);
        }
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
    if stats.dropped_open_spans > 0 {
        messages.push(format!(
            "live recorder dropped {} open candidate span(s) because max_open_spans={} was reached; raise max_open_spans or reduce capture scope",
            stats.dropped_open_spans, limits.max_open_spans
        ));
    }
    if stats.dropped_completed_spans > 0 {
        messages.push(format!(
            "live recorder dropped {} completed span(s) because max_completed_spans={} was reached; raise max_completed_spans or reduce capture scope",
            stats.dropped_completed_spans, limits.max_completed_spans
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

    if stats.closed_missing_kind_spans > 0 {
        let mut msg = format!(
            "live recorder closed {} candidate span(s) missing tt.kind; closed candidate spans without tt.kind are not converted",
            stats.closed_missing_kind_spans
        );
        if !stats.closed_missing_kind_samples.is_empty() {
            let sample_text = stats
                .closed_missing_kind_samples
                .iter()
                .map(|sample| {
                    format!(
                        "name={}, id={}, tt.request_id={}",
                        sample.name,
                        sample.span_id.as_deref().unwrap_or("-"),
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
    if stats.dropped_completed_spans > 0 {
        let msg = format!(
            "live recorder dropped {} completed spans because max_completed_spans was reached",
            stats.dropped_completed_spans
        );
        run.metadata.lifecycle_warnings.push(msg.clone());
        warnings.push(crate::ImportWarning::new(msg));
    }
    if stats.dropped_open_spans > 0 || stats.dropped_completed_spans > 0 {
        run.truncation.limits_hit = true;
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
        && stats.dropped_completed_spans == 0
        && stats.open_candidate_count == 0
        && stats.closed_missing_kind_spans == 0
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
    fn shutdown_returns_imported_run() {
        with_recorder(|recorder| {
            let span = tracing::info_span!(
                "request",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/checkout"
            );
            drop(span);
            let run = recorder.clone().shutdown().unwrap();
            assert_eq!(run.run().requests.len(), 1);
            assert_eq!(run.run().requests[0].request_id, "r1");
            assert_eq!(run.run().requests[0].route, "/checkout");
        });
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
            assert!(run
                .warnings()
                .iter()
                .any(|w| w.message().contains("unknown tt.kind 'Some(\"request\")'")));
            assert!(run
                .warnings()
                .iter()
                .any(|w| w.message().contains("tt.kind") && w.message().contains("numeric-kind")));
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

            let imported = recorder.clone().shutdown().unwrap();
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
            .max_completed_spans(1)
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
        assert!(imported
            .warnings()
            .iter()
            .any(|w| w.message().contains("dropped 1 completed spans")));
        assert!(imported
            .run()
            .metadata
            .lifecycle_warnings
            .iter()
            .any(|w| w.contains("dropped 1 completed spans")));
        assert!(imported.run().truncation.limits_hit);
        assert_eq!(imported.run().requests[0].request_id, "r1");
    }

    #[test]
    fn strict_mode_errors_when_max_completed_spans_drops_completed_spans() {
        let recorder = TracingRecorder::builder("svc")
            .strict(true)
            .max_completed_spans(1)
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
                assert!(message.contains("dropped 1 completed span"));
                assert!(message.contains("max_completed_spans=1"));
                assert!(message.contains("reduce capture scope"));
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
            .max_completed_spans(1)
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
                assert!(message.contains("dropped 1 completed span"));
                assert!(message.contains("max_completed_spans=1"));
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
            .max_completed_spans(1)
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
            w.message().contains("dropped") && w.message().contains("max_completed_spans")
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
            .any(|w| w.contains("max_completed_spans")));
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
            assert!(msg.contains("missing tt.kind"));
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
            let shutdown = recorder.clone().shutdown().unwrap();
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
}
