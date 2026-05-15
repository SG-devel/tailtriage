//! `tailtriage-tracing` provides tracing intake bridge types for `tailtriage` triage workflows.
//!
//! This crate defines semantic convention constants and typed span records that will be
//! converted into [`tailtriage_core::Run`] in later slices.
//!
//! It does **not** currently implement JSONL parsing, live tracing layers, CLI integration,
//! or OpenTelemetry/OTLP ingestion.
//!
//! # Example
//! ```
//! use tailtriage_tracing::{FieldValue, SpanRecord, TT_KIND, TT_REQUEST_ID, TT_ROUTE};
//!
//! let span = SpanRecord::new("http.request", 1_700_000_000_000, 1_700_000_000_120)
//!     .field(TT_KIND, FieldValue::String("request".into()))
//!     .field(TT_REQUEST_ID, FieldValue::String("req-42".into()))
//!     .field(TT_ROUTE, FieldValue::String("GET /items/:id".into()));
//!
//! assert_eq!(span.name(), "http.request");
//! ```

mod convention;
mod error;
mod types;

pub use convention::*;
pub use error::ImportError;
pub use types::{FieldValue, ImportOptions, ImportWarning, ImportedRun, SpanKind, SpanRecord};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_match_expected_keys() {
        assert_eq!(TT_KIND, "tt.kind");
        assert_eq!(TT_REQUEST_ID, "tt.request_id");
        assert_eq!(TT_ROUTE, "tt.route");
        assert_eq!(TT_STAGE, "tt.stage");
        assert_eq!(TT_QUEUE, "tt.queue");
        assert_eq!(TT_DEPTH_AT_START, "tt.depth_at_start");
        assert_eq!(TT_OUTCOME, "tt.outcome");
        assert_eq!(TT_SUCCESS, "tt.success");
    }

    #[test]
    fn span_record_builder_stores_fields() {
        let span = SpanRecord::new("work", 10, 20)
            .field(TT_KIND, FieldValue::String("stage".to_string()))
            .field(TT_SUCCESS, FieldValue::Bool(true));

        assert_eq!(span.name(), "work");
        assert_eq!(span.started_at_unix_ms(), 10);
        assert_eq!(span.finished_at_unix_ms(), 20);
        assert_eq!(
            span.fields().get(TT_KIND),
            Some(&FieldValue::String("stage".to_string()))
        );
        assert_eq!(span.fields().get(TT_SUCCESS), Some(&FieldValue::Bool(true)));
    }

    #[test]
    fn import_options_builder_sets_values() {
        let options = ImportOptions::new("checkout")
            .service_version("1.2.3")
            .run_id("run-123")
            .strict(true);

        assert_eq!(options.service_name(), "checkout");
        assert_eq!(options.service_version_ref(), Some("1.2.3"));
        assert_eq!(options.run_id_ref(), Some("run-123"));
        assert!(options.strict_mode());
    }

    #[test]
    fn import_warning_display_is_message() {
        let warning = ImportWarning::new("missing optional field: tt.route");
        assert_eq!(warning.to_string(), "missing optional field: tt.route");
    }
}
