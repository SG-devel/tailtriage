use std::{
    collections::BTreeMap,
    fmt,
    sync::{Arc, Mutex},
};

use tracing::{
    field::{Field, Visit},
    span::{Attributes, Id, Record},
    Subscriber,
};
use tracing_subscriber::{
    layer::{Context, Layer},
    registry::LookupSpan,
};

use crate::{
    run_from_span_records, FieldValue, ImportError, ImportOptions, ImportedRun, SpanRecord, TT_KIND,
};

#[derive(Debug, Clone)]
struct OpenSpanData {
    id: Option<String>,
    parent_id: Option<String>,
    name: String,
    started_at_unix_ms: u64,
    fields: BTreeMap<String, FieldValue>,
}

#[derive(Debug, Default)]
struct RecorderState {
    open_spans: BTreeMap<String, OpenSpanData>,
    closed_spans: Vec<SpanRecord>,
}

#[derive(Debug, Clone)]
/// Builder for configuring a live tracing recorder.
pub struct TracingRecorderBuilder {
    options: ImportOptions,
}

impl TracingRecorderBuilder {
    /// Sets service version metadata for imported runs.
    #[must_use]
    pub fn service_version(mut self, service_version: impl Into<String>) -> Self {
        self.options = self.options.service_version(service_version);
        self
    }

    /// Sets a run identifier to use during conversion.
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

    /// Builds a recorder with the current options.
    #[must_use]
    pub fn build(self) -> TracingRecorder {
        TracingRecorder {
            options: self.options,
            state: Arc::new(Mutex::new(RecorderState::default())),
        }
    }
}

#[derive(Debug, Clone)]
/// Live tracing recorder that stores completed `tt.*` spans in memory.
pub struct TracingRecorder {
    options: ImportOptions,
    state: Arc<Mutex<RecorderState>>,
}

impl TracingRecorder {
    /// Creates a recorder builder with required service name.
    pub fn builder(service_name: impl Into<String>) -> TracingRecorderBuilder {
        TracingRecorderBuilder {
            options: ImportOptions::new(service_name),
        }
    }

    /// Returns a clonable `tracing_subscriber::Layer` bound to this recorder.
    #[must_use]
    pub fn layer(&self) -> TailtriageLayer {
        TailtriageLayer {
            state: Arc::clone(&self.state),
        }
    }

    /// Converts currently closed spans into a `tailtriage_core::Run`.
    ///
    /// # Errors
    /// Returns an error when the internal lock is poisoned or strict conversion fails.
    pub fn snapshot_run(&self) -> Result<ImportedRun, ImportError> {
        let state = self.state.lock().map_err(|err| {
            ImportError::StrictViolation(format!("recorder state lock poisoned: {err}"))
        })?;
        run_from_span_records(state.closed_spans.clone(), self.options.clone())
    }

    /// Converts currently closed spans into a run.
    ///
    /// Same behavior as `snapshot_run` in this phase.
    ///
    /// # Errors
    /// Returns an error when the internal lock is poisoned or strict conversion fails.
    pub fn shutdown(&self) -> Result<ImportedRun, ImportError> {
        self.snapshot_run()
    }
}

#[derive(Debug, Clone)]
/// `tracing_subscriber::Layer` that captures spans for tailtriage intake.
pub struct TailtriageLayer {
    state: Arc<Mutex<RecorderState>>,
}

impl<S> Layer<S> for TailtriageLayer
where
    S: Subscriber + for<'span> LookupSpan<'span>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let Some(span_ref) = ctx.span(id) else {
            return;
        };

        let mut visitor = FieldVisitor::default();
        attrs.record(&mut visitor);
        if !visitor.fields.contains_key(TT_KIND) {
            return;
        }

        let open = OpenSpanData {
            id: Some(id.into_u64().to_string()),
            parent_id: span_ref
                .parent()
                .map(|parent| parent.id().into_u64().to_string()),
            name: span_ref.name().to_owned(),
            started_at_unix_ms: tailtriage_core::unix_time_ms(),
            fields: visitor.fields,
        };

        if let Ok(mut state) = self.state.lock() {
            state.open_spans.insert(id.into_u64().to_string(), open);
        }
    }

    fn on_record(&self, span: &Id, values: &Record<'_>, _ctx: Context<'_, S>) {
        let mut visitor = FieldVisitor::default();
        values.record(&mut visitor);

        if let Ok(mut state) = self.state.lock() {
            if let Some(open) = state.open_spans.get_mut(&span.into_u64().to_string()) {
                open.fields.extend(visitor.fields);
            }
        }
    }

    fn on_close(&self, id: Id, _ctx: Context<'_, S>) {
        if let Ok(mut state) = self.state.lock() {
            if let Some(open) = state.open_spans.remove(&id.into_u64().to_string()) {
                let mut span = SpanRecord::new(
                    open.name,
                    open.started_at_unix_ms,
                    tailtriage_core::unix_time_ms(),
                );
                if let Some(span_id) = open.id {
                    span = span.id(span_id);
                }
                if let Some(parent_id) = open.parent_id {
                    span = span.parent_id(parent_id);
                }
                for (key, value) in open.fields {
                    span = span.field(key, value);
                }
                state.closed_spans.push(span);
            }
        }
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
    use tracing::subscriber::with_default;
    use tracing_subscriber::{layer::SubscriberExt, Registry};

    use crate::{TracingRecorder, TT_OUTCOME};

    #[test]
    fn collects_request_span() {
        let recorder = TracingRecorder::builder("svc").build();
        let subscriber = Registry::default().with(recorder.layer());

        with_default(subscriber, || {
            let span = tracing::info_span!(
                "http.request",
                "tt.kind" = "request",
                "tt.request_id" = "r1",
                "tt.route" = "/checkout",
                "tt.outcome" = "ok"
            );
            let _entered = span.enter();
        });

        let run = recorder.snapshot_run().expect("snapshot");
        assert_eq!(run.run().requests.len(), 1);
    }

    #[test]
    fn collects_stage_span() {
        let recorder = TracingRecorder::builder("svc").build();
        let subscriber = Registry::default().with(recorder.layer());

        with_default(subscriber, || {
            let span = tracing::info_span!(
                "stage",
                "tt.kind" = "stage",
                "tt.request_id" = "r1",
                "tt.stage" = "db",
                "tt.success" = true
            );
            let _entered = span.enter();
        });

        let run = recorder.snapshot_run().expect("snapshot");
        assert_eq!(run.run().stages.len(), 1);
    }

    #[test]
    fn collects_queue_span() {
        let recorder = TracingRecorder::builder("svc").build();
        let subscriber = Registry::default().with(recorder.layer());

        with_default(subscriber, || {
            let span = tracing::info_span!(
                "queue",
                "tt.kind" = "queue",
                "tt.request_id" = "r1",
                "tt.queue" = "worker",
                "tt.depth_at_start" = 2_u64
            );
            let _entered = span.enter();
        });

        let run = recorder.snapshot_run().expect("snapshot");
        assert_eq!(run.run().queues.len(), 1);
    }

    #[test]
    fn on_record_updates_field() {
        let recorder = TracingRecorder::builder("svc").build();
        let subscriber = Registry::default().with(recorder.layer());

        with_default(subscriber, || {
            let span = tracing::info_span!(
                "request",
                "tt.kind" = "request",
                "tt.request_id" = "r1",
                "tt.route" = "/r",
                "tt.outcome" = tracing::field::Empty
            );
            span.record(TT_OUTCOME, "timeout");
            let _entered = span.enter();
        });

        let run = recorder.snapshot_run().expect("snapshot");
        assert_eq!(run.run().requests[0].outcome, "timeout");
    }

    #[test]
    fn ignores_unrelated_spans() {
        let recorder = TracingRecorder::builder("svc").build();
        let subscriber = Registry::default().with(recorder.layer());

        with_default(subscriber, || {
            let span = tracing::info_span!("ordinary", user_id = 7);
            let _entered = span.enter();
        });

        let run = recorder.snapshot_run().expect("snapshot");
        assert!(run.run().requests.is_empty());
        assert!(run.run().stages.is_empty());
        assert!(run.run().queues.is_empty());
    }
}
