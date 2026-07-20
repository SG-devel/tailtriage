#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

// Core run schema and split request lifecycle instrumentation API for tailtriage.
//
// ```no_run
// use tailtriage_core::{RequestOptions, Tailtriage};
//
// # async fn demo() -> Result<(), Box<dyn std::error::Error>> {
// let tailtriage = Tailtriage::builder("checkout-service")
//     .output("tailtriage-run.json")
//     .build()?;
//
// let started = tailtriage
//     .begin_request_with("/checkout", RequestOptions::new().request_id("req-1").kind("http"));
// let request = started.handle.clone();
//
// // queue(...), stage(...), and inflight(...) instrumentation can happen here.
// // They do not finish the request lifecycle.
// started.completion.finish_ok();
// // You must finish each request exactly once via finish(...), finish_ok(), or finish_result(...).
// // Drop only asserts on unfinished completions in debug builds; it does not auto-record completion.
//
// tailtriage.shutdown()?;
// # Ok(())
// # }
// ```

mod collector;
mod config;
mod events;
mod request_ids;
mod retention;
mod run_builder;
mod sink;
mod time;
mod timers;
mod validation;

pub use collector::{
    OwnedRequestCompletion, OwnedRequestHandle, OwnedStartedRequest, RequestCompletion,
    RequestHandle, RuntimeSamplerRegistrationError, StartedRequest, Tailtriage,
};
pub use config::{
    BuildError, CaptureLimits, CaptureLimitsOverride, CaptureMode, EffectiveCoreConfig,
    RequestOptions, TailtriageBuilder,
};
pub use events::{
    EffectiveTokioSamplerConfig, InFlightSnapshot, Outcome, QueueEvent, RequestEvent, Run,
    RunEndReason, RunMetadata, RuntimeSnapshot, StageEvent, TruncationSummary,
    UnfinishedRequestSample, UnfinishedRequests, SCHEMA_VERSION,
};
pub use run_builder::{RunBuilder, RunBuilderEventError, RunBuilderOptions};
pub use sink::{DiscardSink, LocalJsonSink, MemorySink, RunSink, SinkError};
pub use time::{system_time_to_unix_ms, unix_time_ms};
pub use timers::{InflightGuard, QueueTimer, StageTimer};
pub use validation::{
    inspect_run, normalize_run_permissive, summarize_run_validation, validate_run_strict,
    NormalizedRun, RunEventDisposition, RunEventDispositionKind, RunSection, RunValidationError,
    RunValidationIssue, RunValidationIssueCode, RunValidationLocation, RunValidationReport,
    RunValidationSeverity, RUN_RELATIVE_DURATION_TOLERANCE_US,
};

/// Internal integration hooks for sibling crates in this workspace.
#[doc(hidden)]
pub mod __internal {
    use crate::{EffectiveTokioSamplerConfig, RuntimeSamplerRegistrationError, Tailtriage};

    /// Registers Tokio sampler startup metadata after real sampler preconditions pass.
    ///
    /// This is an intentionally narrow cross-crate boundary for
    /// `tailtriage-tokio` integration. It is hidden from docs and not a
    /// supported end-user API surface.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeSamplerRegistrationError::DuplicateStart`] when a sampler
    /// was already registered for this run.
    pub fn register_tokio_runtime_sampler(
        tailtriage: &Tailtriage,
        config: EffectiveTokioSamplerConfig,
    ) -> Result<(), RuntimeSamplerRegistrationError> {
        tailtriage.register_tokio_runtime_sampler(config)
    }
}

#[cfg(test)]
mod tests;
