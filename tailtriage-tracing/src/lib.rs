//! Tracing intake bridge types for `tailtriage`.
//!
//! This crate defines semantic conventions and strongly typed import-facing structures
//! that future functionality will use to convert trace-shaped spans into
//! [`tailtriage_core::Run`].
//!
//! It currently does **not** implement JSONL parsing, a `tracing::Layer`, CLI commands,
//! or OpenTelemetry/OTLP support.
//!
//! # Example
//! ```
//! use tailtriage_tracing::{SpanRecord, TT_KIND, TT_REQUEST_ID, TT_ROUTE};
//!
//! let span = SpanRecord::new("request", 1_700_000_000_000, 1_700_000_000_010)
//!     .field(TT_KIND, "request")
//!     .field(TT_REQUEST_ID, "req-42")
//!     .field(TT_ROUTE, "/checkout");
//!
//! assert_eq!(span.name(), "request");
//! ```

mod convention;
mod error;
mod types;

pub use convention::{
    TT_DEPTH_AT_START, TT_KIND, TT_OUTCOME, TT_QUEUE, TT_REQUEST_ID, TT_ROUTE, TT_STAGE, TT_SUCCESS,
};
pub use error::ImportError;
pub use types::{FieldValue, ImportOptions, ImportWarning, ImportedRun, SpanKind, SpanRecord};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_match_contract() {
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
        let span = SpanRecord::new("queue_wait", 10, 20)
            .field(TT_KIND, "queue")
            .field(TT_QUEUE, "checkout")
            .field(TT_DEPTH_AT_START, 5_u64);
        assert_eq!(
            span.fields().get(TT_KIND),
            Some(&FieldValue::String("queue".to_owned()))
        );
        assert_eq!(
            span.fields().get(TT_DEPTH_AT_START),
            Some(&FieldValue::U64(5))
        );
    }

    #[test]
    fn import_options_builder_sets_values() {
        let opts = ImportOptions::new("checkout-service")
            .service_version("1.2.3")
            .run_id("run-99")
            .strict(true);

        assert_eq!(opts.service_name(), "checkout-service");
        assert_eq!(opts.service_version_value(), Some("1.2.3"));
        assert_eq!(opts.run_id_value(), Some("run-99"));
        assert!(opts.strict_mode());
    }

    #[test]
    fn import_warning_display_uses_message() {
        let warning = ImportWarning::new("missing optional tt.route");
        assert_eq!(warning.to_string(), "missing optional tt.route");
    }
}
