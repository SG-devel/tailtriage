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
// // Explicit completion remains preferred when the outcome is known.
// // Dropping an admitted unfinished completion token while capture remains open records one request with outcome `cancelled`.
// // If finalization wins first, a late completion-token Drop is inert.
//
// tailtriage.shutdown()?;
// # Ok(())
// # }
// ```

mod artifact;
mod collector;
mod config;
mod events;
mod retention;
mod run_builder;
mod sink;
mod time;
mod timers;
mod validation;

pub use artifact::{decode_run_json_path, RunJsonDecodeError};
pub use collector::{
    OwnedRequestCompletion, OwnedRequestHandle, OwnedStartedRequest, RequestCompletion,
    RequestHandle, ShutdownError, StartedRequest, Tailtriage,
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
pub use run_builder::{RunBuilder, RunBuilderError, RunBuilderEventError, RunBuilderOptions};
pub use sink::{DiscardSink, LocalJsonSink, MemorySink, RunSink, SinkError};
pub use time::{system_time_to_unix_ms, unix_time_ms};
pub use timers::{InflightGuard, QueueTimer, StageTimer};
pub use validation::{
    inspect_run, normalize_run_permissive, summarize_run_validation,
    summarize_run_validation_lifecycle, validate_run_strict, NormalizedRun, RunEventDisposition,
    RunEventDispositionKind, RunSection, RunValidationError, RunValidationIssue,
    RunValidationIssueCode, RunValidationLocation, RunValidationReport, RunValidationSeverity,
    RUN_RELATIVE_DURATION_TOLERANCE_US,
};

/// Compiler-public integration hooks for sibling crates in this workspace.
///
/// Workspace packages are separate Rust crates, so sibling integrations cannot call core
/// `pub(crate)` internals. These items are public only at the compiler level to provide that
/// narrow bridge. The module is hidden from documentation, excluded from the supported end-user
/// API contract, and may change without the compatibility guarantees of supported public APIs.
/// Ordinary core implementation details must remain private or `pub(crate)`.
#[doc(hidden)]
pub mod __internal {
    use crate::{EffectiveTokioSamplerConfig, RunEndReason, Tailtriage};

    /// Internal cross-crate protocol signal used by the Tokio integration.
    ///
    /// This is not a supported user-facing core error type.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct DuplicateRuntimeSampler;

    /// Sets controller-owned run-end provenance for the controller integration.
    ///
    /// Controller lifecycle owns this mutation. The hook exists only so
    /// `tailtriage-controller` can stamp controller-owned run-end provenance; exposing it as
    /// ordinary application API would let callers forge or mutate that provenance independently
    /// of controller state.
    pub fn set_run_end_reason_if_absent(tailtriage: &Tailtriage, reason: RunEndReason) {
        tailtriage.set_run_end_reason_if_absent(reason);
    }

    /// Escapes control characters for human-readable output in workspace sibling integrations.
    ///
    /// This shares human-output escaping across sibling Tailtriage crates. It is not part of core
    /// capture, Run-schema, validation, persistence, or lifecycle semantics; promoting it would
    /// create an unrelated generic text-utility API commitment. It is also not a general
    /// serialization or Unicode security mechanism.
    #[must_use]
    pub fn escape_control_chars(input: &str) -> String {
        let mut output = String::with_capacity(input.len());
        for ch in input.chars() {
            if ch.is_control() {
                output.extend(ch.escape_default());
            } else {
                output.push(ch);
            }
        }
        output
    }

    /// Registers Tokio sampler startup metadata after real sampler preconditions pass.
    ///
    /// Tokio sampler registration is owned by `tailtriage-tokio`, which invokes this hook only
    /// after its runtime and startup preconditions pass. Making it ordinary public core API would
    /// let applications forge effective Tokio sampler metadata without going through that
    /// integration.
    ///
    /// # Errors
    ///
    /// Returns [`DuplicateRuntimeSampler`] when a sampler was already registered for this run.
    pub fn register_tokio_runtime_sampler(
        tailtriage: &Tailtriage,
        config: EffectiveTokioSamplerConfig,
    ) -> Result<(), DuplicateRuntimeSampler> {
        tailtriage
            .register_tokio_runtime_sampler(config)
            .map_err(|_| DuplicateRuntimeSampler)
    }

    #[cfg(test)]
    mod tests {
        use super::escape_control_chars;

        // TT-TEST: S04 primary
        #[test]
        fn human_text_escaping_preserves_the_internal_integration_contract() {
            let input = "plain\\slash café 東京\n\r\t\u{1b}\u{7}\u{8}\u{7f}\u{85}";
            let escaped = escape_control_chars(input);

            assert_eq!(
                escaped,
                "plain\\slash café 東京\\n\\r\\t\\u{1b}\\u{7}\\u{8}\\u{7f}\\u{85}"
            );
            assert!(!escaped.chars().any(char::is_control));
            assert_eq!(escape_control_chars(&escaped), escaped);
        }
    }
}

#[cfg(test)]
mod tests;
