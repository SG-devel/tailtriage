use std::{
    collections::BTreeMap,
    fmt,
    sync::{Arc, Mutex},
};

use tracing::{field::Visit, span, Subscriber};
use tracing_subscriber::{layer::Context, registry::LookupSpan, Layer};

use crate::{
    run_from_span_records, FieldValue, ImportError, ImportOptions, ImportedRun, SpanRecord,
};

#[derive(Debug, Clone)]
struct OpenSpan {
    id: String,
    parent_id: Option<String>,
    name: String,
    started_at_unix_ms: u64,
    fields: BTreeMap<String, FieldValue>,
}

#[derive(Debug, Default)]
struct RecorderState {
    open_spans: BTreeMap<String, OpenSpan>,
    completed_spans: Vec<SpanRecord>,
}

/// Builds a [`TracingRecorder`].
#[derive(Debug, Clone)]
pub struct TracingRecorderBuilder {
    options: ImportOptions,
}

impl TracingRecorderBuilder {
    /// Sets service version metadata.
    #[must_use]
    pub fn service_version(mut self, service_version: impl Into<String>) -> Self {
        self.options = self.options.service_version(service_version);
        self
    }

    /// Sets explicit run id metadata.
    #[must_use]
    pub fn run_id(mut self, run_id: impl Into<String>) -> Self {
        self.options = self.options.run_id(run_id);
        self
    }

    /// Enables or disables strict import behavior.
    #[must_use]
    pub fn strict(mut self, strict: bool) -> Self {
        self.options = self.options.strict(strict);
        self
    }

    /// Creates a recorder.
    #[must_use]
    pub fn build(self) -> TracingRecorder {
        TracingRecorder {
            options: self.options,
            state: Arc::new(Mutex::new(RecorderState::default())),
        }
    }
}

/// Live in-memory recorder for completed tracing spans with `tt.*` fields.
#[derive(Debug, Clone)]
pub struct TracingRecorder {
    options: ImportOptions,
    state: Arc<Mutex<RecorderState>>,
}

impl TracingRecorder {
    /// Creates a recorder builder.
    pub fn builder(service_name: impl Into<String>) -> TracingRecorderBuilder {
        TracingRecorderBuilder {
            options: ImportOptions::new(service_name),
        }
    }

    /// Returns a cloneable tracing layer that records span lifecycle updates.
    #[must_use]
    pub fn layer(&self) -> TailtriageLayer {
        TailtriageLayer {
            state: Arc::clone(&self.state),
        }
    }

    /// Converts completed captured spans into an imported run snapshot.
    ///
    /// # Errors
    ///
    /// Returns [`ImportError`] when strict conversion rejects malformed `tt.*` spans.
    ///
    /// # Panics
    ///
    /// Panics if the internal recorder mutex is poisoned.
    pub fn snapshot_run(&self) -> Result<ImportedRun, ImportError> {
        let completed = {
            let state = self.state.lock().expect("recorder mutex poisoned");
            state.completed_spans.clone()
        };
        run_from_span_records(completed, self.options.clone())
    }

    /// Converts completed captured spans into an imported run on shutdown.
    ///
    /// # Errors
    ///
    /// Returns [`ImportError`] when strict conversion rejects malformed `tt.*` spans.
    pub fn shutdown(&self) -> Result<ImportedRun, ImportError> {
        self.snapshot_run()
    }
}

/// Tracing layer that captures completed spans into a [`TracingRecorder`].
#[derive(Clone)]
pub struct TailtriageLayer {
    state: Arc<Mutex<RecorderState>>,
}

impl fmt::Debug for TailtriageLayer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TailtriageLayer").finish_non_exhaustive()
    }
}

impl<S> Layer<S> for TailtriageLayer
where
    S: Subscriber + for<'span> LookupSpan<'span>,
{
    fn on_new_span(&self, attrs: &span::Attributes<'_>, id: &span::Id, ctx: Context<'_, S>) {
        let Some(span_ref) = ctx.span(id) else {
            return;
        };

        let mut visitor = FieldVisitor::default();
        attrs.record(&mut visitor);
        let metadata = span_ref.metadata();

        let open_span = OpenSpan {
            id: id.into_u64().to_string(),
            parent_id: attrs
                .parent()
                .map(|p| p.into_u64().to_string())
                .or_else(|| {
                    span_ref
                        .parent()
                        .map(|parent| parent.id().into_u64().to_string())
                }),
            name: metadata.name().to_owned(),
            started_at_unix_ms: tailtriage_core::unix_time_ms(),
            fields: visitor.fields,
        };

        let mut state = self.state.lock().expect("recorder mutex poisoned");
        state.open_spans.insert(open_span.id.clone(), open_span);
    }

    fn on_record(&self, id: &span::Id, values: &span::Record<'_>, _ctx: Context<'_, S>) {
        let mut visitor = FieldVisitor::default();
        values.record(&mut visitor);

        let mut state = self.state.lock().expect("recorder mutex poisoned");
        if let Some(open_span) = state.open_spans.get_mut(&id.into_u64().to_string()) {
            open_span.fields.extend(visitor.fields);
        }
    }

    fn on_close(&self, id: span::Id, _ctx: Context<'_, S>) {
        let mut state = self.state.lock().expect("recorder mutex poisoned");
        let Some(open_span) = state.open_spans.remove(&id.into_u64().to_string()) else {
            return;
        };

        if !open_span.fields.contains_key("tt.kind") {
            return;
        }

        let mut record = SpanRecord::new(
            open_span.name,
            open_span.started_at_unix_ms,
            tailtriage_core::unix_time_ms(),
        )
        .id(open_span.id);

        if let Some(parent_id) = open_span.parent_id {
            record = record.parent_id(parent_id);
        }
        for (k, v) in open_span.fields {
            record = record.field(k, v);
        }
        state.completed_spans.push(record);
    }
}

#[derive(Default)]
struct FieldVisitor {
    fields: BTreeMap<String, FieldValue>,
}

impl Visit for FieldVisitor {
    fn record_f64(&mut self, field: &tracing::field::Field, value: f64) {
        self.fields
            .insert(field.name().to_owned(), FieldValue::F64(value));
    }

    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        self.fields
            .insert(field.name().to_owned(), FieldValue::I64(value));
    }

    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        self.fields
            .insert(field.name().to_owned(), FieldValue::U64(value));
    }

    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        self.fields
            .insert(field.name().to_owned(), FieldValue::Bool(value));
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.fields.insert(
            field.name().to_owned(),
            FieldValue::String(value.to_owned()),
        );
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn fmt::Debug) {
        self.fields.insert(
            field.name().to_owned(),
            FieldValue::String(format!("{value:?}")),
        );
    }
}

#[cfg(test)]
mod tests {
    use tracing::{field::Empty, Level};
    use tracing_subscriber::{layer::SubscriberExt, Registry};

    use crate::{TracingRecorder, TT_OUTCOME};

    #[test]
    fn collects_request_span() {
        let recorder = TracingRecorder::builder("svc").build();
        let subscriber = Registry::default().with(recorder.layer());
        tracing::subscriber::with_default(subscriber, || {
            let request_span = tracing::info_span!(
                "http.request",
                tt.kind = "request",
                tt.request_id = "req-1",
                tt.route = "/checkout"
            );
            let _guard = request_span.enter();
        });

        let run = recorder.snapshot_run().expect("run");
        assert_eq!(run.run().requests.len(), 1);
    }

    #[test]
    fn collects_stage_span() {
        let recorder = TracingRecorder::builder("svc").build();
        let subscriber = Registry::default().with(recorder.layer());
        tracing::subscriber::with_default(subscriber, || {
            let stage_span = tracing::info_span!(
                "db.stage",
                tt.kind = "stage",
                tt.request_id = "req-1",
                tt.stage = "db"
            );
            let _guard = stage_span.enter();
        });

        let run = recorder.snapshot_run().expect("run");
        assert_eq!(run.run().stages.len(), 1);
    }

    #[test]
    fn collects_queue_span() {
        let recorder = TracingRecorder::builder("svc").build();
        let subscriber = Registry::default().with(recorder.layer());
        tracing::subscriber::with_default(subscriber, || {
            let queue_span = tracing::info_span!(
                "permit.queue",
                tt.kind = "queue",
                tt.request_id = "req-1",
                tt.queue = "permits"
            );
            let _guard = queue_span.enter();
        });

        let run = recorder.snapshot_run().expect("run");
        assert_eq!(run.run().queues.len(), 1);
    }

    #[test]
    fn on_record_updates_fields() {
        let recorder = TracingRecorder::builder("svc").build();
        let subscriber = Registry::default().with(recorder.layer());
        tracing::subscriber::with_default(subscriber, || {
            let request_span = tracing::info_span!(
                "http.request",
                tt.kind = "request",
                tt.request_id = "req-1",
                tt.route = "/checkout",
                tt.outcome = Empty
            );
            request_span.record(TT_OUTCOME, "timeout");
            let _guard = request_span.enter();
        });

        let run = recorder.snapshot_run().expect("run");
        assert_eq!(run.run().requests[0].outcome, "timeout");
    }

    #[test]
    fn unrelated_span_is_ignored() {
        let recorder = TracingRecorder::builder("svc").build();
        let subscriber = Registry::default().with(recorder.layer());
        tracing::subscriber::with_default(subscriber, || {
            let other_span = tracing::span!(Level::INFO, "not-tailtriage", request_id = "req-1");
            let _guard = other_span.enter();
        });

        let run = recorder.snapshot_run().expect("run");
        assert!(run.run().requests.is_empty());
        assert!(run.run().stages.is_empty());
        assert!(run.run().queues.is_empty());
    }
}
