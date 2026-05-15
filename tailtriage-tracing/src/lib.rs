#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

//! Tracing intake bridge types for tailtriage triage workflows.
//!
//! This crate provides semantic keys and typed records for importing
//! tracing-shaped span data into [`tailtriage_core::Run`].
//! It intentionally does not provide JSONL parsing, a `tracing` layer,
//! OpenTelemetry integration, or analyzer behavior changes.
//!
//! # Example
//!
//! ```
//! use tailtriage_tracing::{
//!     ImportOptions, SpanRecord, TT_DEPTH_AT_START, TT_KIND, TT_REQUEST_ID, TT_ROUTE, TT_SUCCESS,
//! };
//!
//! let record = SpanRecord::new("http.request", 1_700_000_000_000, 1_700_000_000_120)
//!     .field(TT_KIND, "request")
//!     .field(TT_REQUEST_ID, "req-42")
//!     .field(TT_ROUTE, "/checkout")
//!     .field(TT_SUCCESS, true)
//!     .field(TT_DEPTH_AT_START, 7_u64);
//!
//! let options = ImportOptions::new("checkout-service").strict(false);
//! assert_eq!(record.name(), "http.request");
//! assert_eq!(options.service_name(), "checkout-service");
//! ```

mod convention;
mod error;
mod types;

pub use convention::{
    TT_DEPTH_AT_START, TT_KIND, TT_OUTCOME, TT_QUEUE, TT_REQUEST_ID, TT_ROUTE, TT_STAGE, TT_SUCCESS,
};
pub use error::ImportError;
pub use types::{FieldValue, ImportOptions, ImportWarning, ImportedRun, SpanKind, SpanRecord};
