use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, Mutex};

use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::Subscriber;
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::registry::LookupSpan;

use crate::{
    run_from_span_records, FieldValue, ImportError, ImportOptions, ImportedRun, SpanRecord,
};

/// In-memory live recorder for completed tracing spans with `tt.*` fields.
#[derive(Debug, Clone)]
pub struct TracingRecorder {
    state: Arc<Mutex<RecorderState>>,
    options: ImportOptions,
}

/// Builder for configuring a [`TracingRecorder`].
#[derive(Debug, Clone)]
pub struct TracingRecorderBuilder {
    options: ImportOptions,
}

/// `tracing_subscriber::Layer` that collects span lifecycle data for [`TracingRecorder`].
#[derive(Debug, Clone)]
pub struct TailtriageLayer {
    state: Arc<Mutex<RecorderState>>,
}

#[derive(Debug, Default)]
struct RecorderState {
    open_spans: BTreeMap<String, OpenSpan>,
    completed_spans: Vec<SpanRecord>,
}

#[derive(Debug)]
struct OpenSpan {
    name: String,
    parent_id: Option<String>,
    fields: BTreeMap<String, FieldValue>,
    started_at_unix_ms: u64,
}

impl TracingRecorder {
    /// Creates a recorder builder with required service name metadata.
    pub fn builder(service_name: impl Into<String>) -> TracingRecorderBuilder {
        TracingRecorderBuilder {
            options: ImportOptions::new(service_name),
        }
    }

    /// Returns a cloneable tracing layer connected to this recorder.
    #[must_use]
    pub fn layer(&self) -> TailtriageLayer {
        TailtriageLayer {
            state: Arc::clone(&self.state),
        }
    }

    /// Finalizes currently completed spans into an imported run snapshot.
    ///
    /// # Errors
    ///
    /// Returns any strict import validation error from `run_from_span_records`.
    pub fn snapshot_run(&self) -> Result<ImportedRun, ImportError> {
        let records = lock_state(&self.state).completed_spans.clone();
        run_from_span_records(records, self.options.clone())
    }

    /// Finalizes currently completed spans into an imported run during shutdown.
    ///
    /// # Errors
    ///
    /// Returns any strict import validation error from `run_from_span_records`.
    pub fn shutdown(&self) -> Result<ImportedRun, ImportError> {
        self.snapshot_run()
    }
}

impl TracingRecorderBuilder {
    /// Sets optional service version metadata.
    #[must_use]
    pub fn service_version(mut self, service_version: impl Into<String>) -> Self {
        self.options = self.options.service_version(service_version);
        self
    }

    /// Sets optional run id metadata.
    #[must_use]
    pub fn run_id(mut self, run_id: impl Into<String>) -> Self {
        self.options = self.options.run_id(run_id);
        self
    }

    /// Enables or disables strict conversion semantics.
    #[must_use]
    pub fn strict(mut self, strict: bool) -> Self {
        self.options = self.options.strict(strict);
        self
    }

    /// Builds the recorder.
    #[must_use]
    pub fn build(self) -> TracingRecorder {
        TracingRecorder {
            state: Arc::new(Mutex::new(RecorderState::default())),
            options: self.options,
        }
    }
}

impl<S> Layer<S> for TailtriageLayer
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let mut visitor = FieldVisitor::default();
        attrs.record(&mut visitor);

        let parent_id = ctx.span(id).and_then(|span_ref| {
            span_ref
                .parent()
                .map(|parent| parent.id().into_u64().to_string())
        });

        let open = OpenSpan {
            name: attrs.metadata().name().to_owned(),
            parent_id,
            fields: visitor.fields,
            started_at_unix_ms: tailtriage_core::unix_time_ms(),
        };

        lock_state(&self.state)
            .open_spans
            .insert(id.into_u64().to_string(), open);
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, _ctx: Context<'_, S>) {
        let mut visitor = FieldVisitor::default();
        values.record(&mut visitor);
        let mut state = lock_state(&self.state);
        if let Some(span) = state.open_spans.get_mut(&id.into_u64().to_string()) {
            for (k, v) in visitor.fields {
                span.fields.insert(k, v);
            }
        }
    }

    fn on_close(&self, id: Id, _ctx: Context<'_, S>) {
        let finish = tailtriage_core::unix_time_ms();
        let mut state = lock_state(&self.state);
        let Some(span) = state.open_spans.remove(&id.into_u64().to_string()) else {
            return;
        };

        if !matches!(span.fields.get("tt.kind"), Some(FieldValue::String(_))) {
            return;
        }

        let mut record = SpanRecord::new(span.name, span.started_at_unix_ms, finish)
            .id(id.into_u64().to_string());
        if let Some(parent_id) = span.parent_id {
            record = record.parent_id(parent_id);
        }
        for (k, v) in span.fields {
            record = record.field(k, v);
        }
        state.completed_spans.push(record);
    }
}

fn lock_state(state: &Arc<Mutex<RecorderState>>) -> std::sync::MutexGuard<'_, RecorderState> {
    match state.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
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
    use tracing::field::Empty;
    use tracing::info_span;
    use tracing_subscriber::prelude::*;

    use super::*;

    #[test]
    fn request_span_collected() {
        let recorder = TracingRecorder::builder("svc").build();
        let subscriber = tracing_subscriber::registry().with(recorder.layer());
        tracing::subscriber::with_default(subscriber, || {
            let span = info_span!(
                "request",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/x"
            );
            let _guard = span.enter();
        });

        let imported = recorder.snapshot_run().unwrap();
        assert_eq!(imported.run().requests.len(), 1);
    }

    #[test]
    fn stage_span_collected() {
        let recorder = TracingRecorder::builder("svc").build();
        let subscriber = tracing_subscriber::registry().with(recorder.layer());
        tracing::subscriber::with_default(subscriber, || {
            let span = info_span!(
                "stage",
                tt.kind = "stage",
                tt.request_id = "r1",
                tt.stage = "db"
            );
            let _guard = span.enter();
        });

        assert_eq!(recorder.snapshot_run().unwrap().run().stages.len(), 1);
    }

    #[test]
    fn queue_span_collected() {
        let recorder = TracingRecorder::builder("svc").build();
        let subscriber = tracing_subscriber::registry().with(recorder.layer());
        tracing::subscriber::with_default(subscriber, || {
            let span = info_span!(
                "queue",
                tt.kind = "queue",
                tt.request_id = "r1",
                tt.queue = "permits"
            );
            let _guard = span.enter();
        });

        assert_eq!(recorder.snapshot_run().unwrap().run().queues.len(), 1);
    }

    #[test]
    fn on_record_updates_field() {
        let recorder = TracingRecorder::builder("svc").build();
        let subscriber = tracing_subscriber::registry().with(recorder.layer());
        tracing::subscriber::with_default(subscriber, || {
            let span = info_span!(
                "request",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/x",
                tt.outcome = Empty
            );
            span.record("tt.outcome", "error");
            let _guard = span.enter();
        });

        let imported = recorder.snapshot_run().unwrap();
        assert_eq!(imported.run().requests[0].outcome, "error");
    }

    #[test]
    fn unrelated_spans_ignored() {
        let recorder = TracingRecorder::builder("svc").build();
        let subscriber = tracing_subscriber::registry().with(recorder.layer());
        tracing::subscriber::with_default(subscriber, || {
            let _span = info_span!("ordinary", foo = 1_u64);
        });

        let imported = recorder.snapshot_run().unwrap();
        assert!(imported.run().requests.is_empty());
        assert!(imported.run().stages.is_empty());
        assert!(imported.run().queues.is_empty());
    }

    #[test]
    fn converted_run_analyzes_in_memory() {
        let recorder = TracingRecorder::builder("svc").build();
        let subscriber = tracing_subscriber::registry().with(recorder.layer());
        tracing::subscriber::with_default(subscriber, || {
            let span = info_span!(
                "request",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/x"
            );
            let _guard = span.enter();
        });

        let imported = recorder.snapshot_run().unwrap();
        let report = tailtriage_analyzer::analyze_run(
            imported.run(),
            tailtriage_analyzer::AnalyzeOptions::default(),
        );
        assert!(report.request_count >= 1);
    }
}
