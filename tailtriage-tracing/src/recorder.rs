use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, Mutex};

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
}

/// Builder for [`TracingRecorder`].
#[derive(Debug, Clone)]
pub struct TracingRecorderBuilder {
    options: ImportOptions,
}

/// `tracing_subscriber` layer that feeds completed spans into a [`TracingRecorder`].
#[derive(Debug, Clone)]
pub struct TailtriageLayer {
    state: Arc<Mutex<RecorderState>>,
}

#[derive(Debug, Default)]
struct RecorderState {
    open: BTreeMap<String, OpenSpan>,
    completed: Vec<SpanRecord>,
}

#[derive(Debug)]
struct OpenSpan {
    id: Option<String>,
    parent_id: Option<String>,
    name: String,
    fields: BTreeMap<String, FieldValue>,
    started_at_unix_ms: u64,
}

impl TracingRecorder {
    /// Creates a builder with required service name metadata.
    pub fn builder(service_name: impl Into<String>) -> TracingRecorderBuilder {
        TracingRecorderBuilder {
            options: ImportOptions::new(service_name),
        }
    }

    /// Returns a cloneable layer that captures `on_new_span`, `on_record`, and `on_close`.
    #[must_use]
    pub fn layer(&self) -> TailtriageLayer {
        TailtriageLayer {
            state: Arc::clone(&self.state),
        }
    }

    /// Converts currently completed spans into an imported run.
    ///
    /// # Errors
    ///
    /// Returns [`ImportError`] when strict conversion fails or the recorder mutex is poisoned.
    pub fn snapshot_run(&self) -> Result<ImportedRun, ImportError> {
        let spans = {
            let state = self.state.lock().map_err(|e| ImportError::InvalidField {
                field: "recorder",
                reason: format!("recorder mutex poisoned: {e}"),
            })?;
            state.completed.clone()
        };
        run_from_span_records(spans, self.options.clone())
    }

    /// Converts currently completed spans into an imported run.
    ///
    /// This is currently equivalent to [`Self::snapshot_run`].
    ///
    /// # Errors
    ///
    /// Returns [`ImportError`] when strict conversion fails or the recorder mutex is poisoned.
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
        }
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
        let open_span = OpenSpan {
            id: Some(id.into_u64().to_string()),
            parent_id,
            name: attrs.metadata().name().to_owned(),
            fields: visitor.fields,
            started_at_unix_ms: tailtriage_core::unix_time_ms(),
        };
        if let Ok(mut state) = self.state.lock() {
            state.open.insert(id.into_u64().to_string(), open_span);
        }
    }

    fn on_record(&self, span_id: &Id, values: &tracing::span::Record<'_>, _ctx: Context<'_, S>) {
        let mut visitor = FieldVisitor::default();
        values.record(&mut visitor);
        if let Ok(mut state) = self.state.lock() {
            let key = span_id.into_u64().to_string();
            if let Some(span) = state.open.get_mut(&key) {
                span.fields.extend(visitor.fields);
            }
        }
    }

    fn on_close(&self, id: Id, _ctx: Context<'_, S>) {
        if let Ok(mut state) = self.state.lock() {
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
                if let Some(span_id) = open.id {
                    record = record.id(span_id);
                }
                if let Some(parent_id) = open.parent_id {
                    record = record.parent_id(parent_id);
                }
                for (k, v) in open.fields {
                    record = record.field(k, v);
                }
                state.completed.push(record);
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
    use super::*;
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
}
