//! Core run schema and request-context instrumentation API for tailtriage.
//!
//! ```no_run
//! use tailtriage_core::{RequestOptions, Tailtriage};
//!
//! # async fn demo() -> Result<(), Box<dyn std::error::Error>> {
//! let tailtriage = Tailtriage::builder("checkout-service")
//!     .output("tailtriage-run.json")
//!     .build()?;
//!
//! let request = tailtriage
//!     .request_with("/checkout", RequestOptions::new().request_id("req-1"))
//!     .with_kind("http");
//! request.run_ok(async {}).await;
//!
//! tailtriage.shutdown()?;
//! # Ok(())
//! # }
//! ```

mod collector;
mod config;
mod events;
mod sink;
mod time;
mod timers;

pub use collector::{RequestContext, Tailtriage};
pub use config::{BuildError, CaptureLimits, CaptureMode, RequestOptions, TailtriageBuilder};
pub use events::{
    InFlightSnapshot, Outcome, QueueEvent, RequestEvent, Run, RunMetadata, RuntimeSnapshot,
    StageEvent, TruncationSummary,
};
pub use sink::{LocalJsonSink, RunSink, SinkError};
pub use time::{system_time_to_unix_ms, unix_time_ms};
pub use timers::{InflightGuard, QueueTimer, StageTimer};

#[cfg(test)]
mod tests;
