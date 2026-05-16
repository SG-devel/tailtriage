use std::collections::BTreeMap;
use std::fmt;
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
    open: BTreeMap<String, OpenSpan>,
    completed: Vec<SpanRecord>,
    dropped_open_spans: u64,
    dropped_completed_spans: u64,
}

#[derive(Debug)]
struct OpenSpan {
    id: Option<String>,
    parent_id: Option<String>,
    name: String,
    fields: BTreeMap<String, FieldValue>,
    started_at_unix_ms: u64,
    started_instant: Instant,
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

    /// Converts currently completed spans into an imported run.
    ///
    /// # Errors
    ///
    /// Returns [`ImportError`] when strict conversion fails.
    pub fn snapshot_run(&self) -> Result<ImportedRun, ImportError> {
        let (spans, dropped_open_spans, dropped_completed_spans) = {
            let state = lock_state(&self.state);
            (
                state.completed.clone(),
                state.dropped_open_spans,
                state.dropped_completed_spans,
            )
        };
        imported_with_drop_warnings(
            spans,
            self.options.clone(),
            dropped_open_spans,
            dropped_completed_spans,
        )
    }

    /// Converts currently completed spans into an imported run.
    ///
    /// This is currently equivalent to [`Self::snapshot_run`].
    ///
    /// # Errors
    ///
    /// Returns [`ImportError`] when strict conversion fails.
    pub fn shutdown(&self) -> Result<ImportedRun, ImportError> {
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
        };
        state.open.insert(id.into_u64().to_string(), open_span);
    }

    fn on_record(&self, span_id: &Id, values: &tracing::span::Record<'_>, _ctx: Context<'_, S>) {
        let mut visitor = FieldVisitor::default();
        values.record(&mut visitor);
        let mut state = lock_state(&self.state);
        let key = span_id.into_u64().to_string();
        if let Some(span) = state.open.get_mut(&key) {
            span.fields.extend(visitor.fields);
        }
    }

    fn on_close(&self, id: Id, _ctx: Context<'_, S>) {
        let mut state = lock_state(&self.state);
        let key = id.into_u64().to_string();
        if let Some(open) = state.open.remove(&key) {
            if !open.fields.contains_key(TT_KIND) {
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

fn imported_with_drop_warnings(
    spans: Vec<SpanRecord>,
    options: ImportOptions,
    dropped_open_spans: u64,
    dropped_completed_spans: u64,
) -> Result<ImportedRun, ImportError> {
    let imported = run_from_span_records(spans, options)?;
    if dropped_open_spans == 0 && dropped_completed_spans == 0 {
        return Ok(imported);
    }
    let (mut run, mut warnings) = imported.into_parts();
    if dropped_open_spans > 0 {
        let msg = format!("live recorder dropped {dropped_open_spans} candidate spans because max_open_spans was reached");
        run.metadata.lifecycle_warnings.push(msg.clone());
        warnings.push(crate::ImportWarning::new(msg));
    }
    if dropped_completed_spans > 0 {
        let msg = format!("live recorder dropped {dropped_completed_spans} completed spans because max_completed_spans was reached");
        run.metadata.lifecycle_warnings.push(msg.clone());
        warnings.push(crate::ImportWarning::new(msg));
    }
    run.truncation.limits_hit = true;
    Ok(ImportedRun::new(run, warnings))
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
            let run = recorder.shutdown().unwrap();
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

            let imported = recorder.shutdown().unwrap();
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
    fn unrelated_spans_do_not_consume_open_limit() {
        let recorder = TracingRecorder::builder("svc").max_open_spans(1).build();
        let subscriber = tracing_subscriber::registry().with(recorder.layer());
        tracing::subscriber::with_default(subscriber, || {
            let unrelated = tracing::info_span!("ordinary", foo = 1_u64);
            let request = tracing::info_span!(
                "request",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/a"
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
}
