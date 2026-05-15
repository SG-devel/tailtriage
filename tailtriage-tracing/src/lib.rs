#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

//! Tracing intake bridge types for `tailtriage` triage workflows.
//!
//! This crate defines semantic keys and span-shaped intake data that future
//! conversion utilities can map into [`tailtriage_core::Run`].
//!
//! It intentionally does not implement JSONL parsing, live recording, OpenTelemetry,
//! or OTLP in this first slice.
//!
//! # Example
//!
//! ```
//! use tailtriage_tracing::{FieldValue, SpanRecord, TT_KIND, TT_REQUEST_ID, TT_SUCCESS};
//!
//! let record = SpanRecord::new("http.request", 1_700_000_000_000, 1_700_000_000_120)
//!     .field(TT_KIND, FieldValue::String("request".to_string()))
//!     .field(TT_REQUEST_ID, FieldValue::String("req-42".to_string()))
//!     .field(TT_SUCCESS, FieldValue::Bool(true));
//!
//! assert_eq!(record.fields().len(), 3);
//! ```

mod convention;
mod error;
mod types;

pub use convention::{
    TT_DEPTH_AT_START, TT_KIND, TT_OUTCOME, TT_QUEUE, TT_REQUEST_ID, TT_ROUTE, TT_STAGE, TT_SUCCESS,
};
pub use error::ImportError;
pub use types::{FieldValue, ImportOptions, ImportWarning, ImportedRun, SpanKind, SpanRecord};
