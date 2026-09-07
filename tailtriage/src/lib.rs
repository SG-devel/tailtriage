#![doc = include_str!("../README.md")]
#![warn(missing_docs)]
#![cfg_attr(docsrs, feature(doc_cfg))]

/// Explicit re-export of the supported `tailtriage-core` API, always available at the crate root.
///
/// Internal sibling integration hooks are deliberately excluded.
pub use tailtriage_core::{
    decode_run_json_path, inspect_run, normalize_run_permissive, summarize_run_validation,
    summarize_run_validation_lifecycle, system_time_to_unix_ms, unix_time_ms, validate_run_strict,
    BuildError, CaptureLimits, CaptureLimitsOverride, CaptureMode, DiscardSink,
    EffectiveCoreConfig, EffectiveTokioSamplerConfig, InFlightSnapshot, InflightGuard,
    LocalJsonSink, MemorySink, NormalizedRun, Outcome, OwnedRequestCompletion, OwnedRequestHandle,
    OwnedStartedRequest, QueueEvent, QueueTimer, RequestCompletion, RequestEvent, RequestHandle,
    RequestOptions, Run, RunBuilder, RunBuilderError, RunBuilderEventError, RunBuilderOptions,
    RunEndReason, RunEventDisposition, RunEventDispositionKind, RunJsonDecodeError, RunMetadata,
    RunSection, RunSink, RunValidationError, RunValidationIssue, RunValidationIssueCode,
    RunValidationLocation, RunValidationReport, RunValidationSeverity, RuntimeSnapshot,
    ShutdownError, SinkError, StageEvent, StageTimer, StartedRequest, Tailtriage,
    TailtriageBuilder, TruncationSummary, UnfinishedRequestSample, UnfinishedRequests,
    RUN_RELATIVE_DURATION_TOLERANCE_US, SCHEMA_VERSION,
};

#[cfg(feature = "axum")]
#[cfg_attr(docsrs, doc(cfg(feature = "axum")))]
/// Optional Axum integration namespace (`tailtriage::axum`).
///
/// Enable with the `axum` feature.
pub use tailtriage_axum as axum;

#[cfg(feature = "controller")]
#[cfg_attr(docsrs, doc(cfg(feature = "controller")))]
/// Controller integration namespace (`tailtriage::controller`).
///
/// Enabled by default via the `controller` feature.
pub use tailtriage_controller as controller;
#[cfg(feature = "tokio")]
#[cfg_attr(docsrs, doc(cfg(feature = "tokio")))]
/// Tokio runtime sampler namespace (`tailtriage::tokio`).
///
/// Enabled by default via the `tokio` feature.
pub use tailtriage_tokio as tokio;

#[cfg(feature = "tracing")]
#[cfg_attr(docsrs, doc(cfg(feature = "tracing")))]
/// Tracing intake bridge namespace (`tailtriage::tracing`).
///
/// Enable with the `tracing` feature. `tracing-live` and `tracing-tokio`
/// expose progressively richer live and Tokio-coupled session APIs.
pub use tailtriage_tracing as tracing;

#[cfg(test)]
mod tests {
    // TT-TEST: P01 primary
    #[test]
    fn core_reexport_exposes_supported_cross_section() {
        use crate::{
            validate_run_strict, EffectiveTokioSamplerConfig, OwnedRequestHandle, QueueTimer, Run,
            RunBuilder, RunSink, RunValidationReport, ShutdownError, SinkError, Tailtriage,
            TailtriageBuilder, SCHEMA_VERSION,
        };

        type CoreTypes<'a> = (
            TailtriageBuilder,
            OwnedRequestHandle,
            ShutdownError,
            Run,
            RunBuilder,
            &'a dyn RunSink,
            SinkError,
            RunValidationReport,
            QueueTimer<'a>,
            EffectiveTokioSamplerConfig,
        );

        let _builder = Tailtriage::builder("default-smoke").sink(crate::DiscardSink);
        let _: Option<CoreTypes<'_>> = None;
        std::hint::black_box(
            validate_run_strict as fn(&Run) -> Result<(), crate::RunValidationError>,
        );
        std::hint::black_box(crate::unix_time_ms as fn() -> u64);
        std::hint::black_box(SCHEMA_VERSION);
    }

    // TT-TEST: P01 primary
    #[cfg(feature = "tokio")]
    #[test]
    fn tokio_namespace_reexport_compiles() {
        let _builder = crate::tokio::RuntimeSampler::builder(std::sync::Arc::new(
            crate::Tailtriage::builder("tokio-smoke")
                .sink(tailtriage_core::DiscardSink)
                .build()
                .expect("build should succeed"),
        ));
    }

    // TT-TEST: P01 primary
    #[cfg(feature = "tokio")]
    #[test]
    fn tokio_helper_trait_reexport_path_compiles() {
        use crate::tokio::TokioRequestHandleExt;

        fn assert_trait<T: TokioRequestHandleExt>() {}
        assert_trait::<crate::RequestHandle<'_>>();
    }

    // TT-TEST: P01 primary
    #[cfg(feature = "controller")]
    #[test]
    fn controller_namespace_reexport_compiles() {
        use crate::controller::{ControllerRequestHandle, TailtriageControllerBuilder};
        use std::time::Duration;
        use tailtriage_core::CaptureMode;

        let sampler = crate::controller::RuntimeSamplerTemplate {
            enabled: true,
            mode_override: Some(CaptureMode::Investigation),
            interval: Some(Duration::from_millis(250)),
            max_runtime_snapshots: Some(123),
        };
        let builder: TailtriageControllerBuilder =
            crate::controller::TailtriageController::builder("default-controller")
                .mode(CaptureMode::Investigation)
                .output("tailtriage-run.json")
                .runtime_sampler(sampler);
        let controller = builder.build().expect("controller should build");
        assert_eq!(
            controller.status().template.mode,
            CaptureMode::Investigation
        );
        assert_eq!(
            controller.status().template.runtime_sampler.interval,
            Some(Duration::from_millis(250))
        );
        let started = controller.begin_request("/checkout");
        let handle: &ControllerRequestHandle = &started.handle;
        let _captured: bool = handle.is_captured();
        let _core: Option<&crate::OwnedRequestHandle> = handle.captured_handle();
        started.completion.finish_ok();
    }

    // TT-TEST: P02 primary
    #[cfg(feature = "axum")]
    #[test]
    fn axum_namespace_reexport_compiles() {
        std::hint::black_box(crate::axum::middleware);
    }

    // TT-TEST: P02 primary
    #[cfg(feature = "tracing")]
    #[test]
    fn tracing_namespace_reexport_compiles() {
        let _options = crate::tracing::ImportOptions::new("default-tracing");
    }
}
