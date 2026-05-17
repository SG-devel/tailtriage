use std::sync::Arc;
use std::time::Duration;

use tailtriage_core::{MemorySink, Tailtriage};
use tailtriage_tokio::{RuntimeSampler, SamplerStartError};

use crate::{ImportError, ImportedRun, RecorderLimits, TailtriageLayer, TracingRecorder};

/// Couples tracing span intake with Tokio runtime sampling in one session.
#[derive(Debug)]
pub struct TracingTokioSession {
    recorder: TracingRecorder,
    runtime_tailtriage: Arc<Tailtriage>,
    runtime_sampler: RuntimeSampler,
}

/// Builder for [`TracingTokioSession`].
#[derive(Debug, Clone)]
pub struct TracingTokioSessionBuilder {
    service_name: String,
    service_version: Option<String>,
    run_id: Option<String>,
    strict: bool,
    limits: RecorderLimits,
    sampler_interval: Option<Duration>,
    max_runtime_snapshots: Option<usize>,
}

/// Error returned when starting a [`TracingTokioSession`] fails.
#[derive(Debug)]
pub enum TracingTokioSessionStartError {
    /// Tracing recorder import configuration failed.
    Import(ImportError),
    /// Tokio runtime sampler failed to start.
    SamplerStart(SamplerStartError),
}

impl core::fmt::Display for TracingTokioSessionStartError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Import(err) => write!(f, "failed to build tracing recorder: {err}"),
            Self::SamplerStart(err) => write!(f, "failed to start Tokio runtime sampler: {err}"),
        }
    }
}

impl std::error::Error for TracingTokioSessionStartError {}

/// Error returned when shutting down a [`TracingTokioSession`] fails.
#[derive(Debug)]
pub enum TracingTokioSessionShutdownError {
    /// Failed to import tracing spans into a run.
    Import(ImportError),
}

impl core::fmt::Display for TracingTokioSessionShutdownError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Import(err) => write!(f, "failed to import tracing run during shutdown: {err}"),
        }
    }
}

impl std::error::Error for TracingTokioSessionShutdownError {}

impl TracingTokioSession {
    /// Creates a session builder for one service.
    #[must_use]
    pub fn builder(service_name: impl Into<String>) -> TracingTokioSessionBuilder {
        TracingTokioSessionBuilder {
            service_name: service_name.into(),
            service_version: None,
            run_id: None,
            strict: false,
            limits: RecorderLimits::default(),
            sampler_interval: None,
            max_runtime_snapshots: None,
        }
    }

    /// Returns the tracing layer that records `tt.*` spans.
    #[must_use]
    pub fn layer(&self) -> TailtriageLayer {
        self.recorder.layer()
    }

    /// Builds a point-in-time merged run from tracing spans and runtime snapshots.
    ///
    /// # Errors
    ///
    /// Returns [`ImportError`] when tracing span import fails.
    pub fn snapshot_run(&self) -> Result<ImportedRun, ImportError> {
        let tracing_run = self.recorder.snapshot_run()?;
        Ok(merge_runtime_data(
            tracing_run,
            self.runtime_tailtriage.snapshot(),
        ))
    }

    /// Stops runtime sampling and returns a final merged run.
    ///
    /// # Errors
    ///
    /// Returns [`TracingTokioSessionShutdownError`] when tracing import fails.
    pub async fn shutdown(self) -> Result<ImportedRun, TracingTokioSessionShutdownError> {
        self.runtime_sampler.shutdown().await;
        let tracing_run = self
            .recorder
            .snapshot_run()
            .map_err(TracingTokioSessionShutdownError::Import)?;
        Ok(merge_runtime_data(
            tracing_run,
            self.runtime_tailtriage.snapshot(),
        ))
    }
}

impl TracingTokioSessionBuilder {
    /// Sets optional service version metadata.
    #[must_use]
    pub fn service_version(mut self, service_version: impl Into<String>) -> Self {
        self.service_version = Some(service_version.into());
        self
    }
    /// Sets optional run id metadata.
    #[must_use]
    pub fn run_id(mut self, run_id: impl Into<String>) -> Self {
        self.run_id = Some(run_id.into());
        self
    }
    /// Enables strict tracing span import validation.
    #[must_use]
    pub fn strict(mut self, strict: bool) -> Self {
        self.strict = strict;
        self
    }
    /// Sets both tracing recorder limits at once.
    #[must_use]
    pub fn recorder_limits(mut self, limits: RecorderLimits) -> Self {
        self.limits = limits;
        self
    }
    /// Sets maximum tracked open spans.
    #[must_use]
    pub fn max_open_spans(mut self, max_open_spans: usize) -> Self {
        self.limits.max_open_spans = max_open_spans;
        self
    }
    /// Sets maximum retained completed spans.
    #[must_use]
    pub fn max_completed_spans(mut self, max_completed_spans: usize) -> Self {
        self.limits.max_completed_spans = max_completed_spans;
        self
    }
    /// Sets runtime sampler interval. Zero duration is rejected at `start()`.
    #[must_use]
    pub fn sampler_interval(mut self, sampler_interval: Duration) -> Self {
        self.sampler_interval = Some(sampler_interval);
        self
    }
    /// Sets runtime sampler retention cap before core-cap clamping.
    #[must_use]
    pub fn max_runtime_snapshots(mut self, max_runtime_snapshots: usize) -> Self {
        self.max_runtime_snapshots = Some(max_runtime_snapshots);
        self
    }

    /// Starts tracing recording and Tokio runtime sampling.
    ///
    /// # Errors
    ///
    /// Returns [`TracingTokioSessionStartError`] when builder configuration is invalid
    /// or when runtime sampler startup fails (for example missing Tokio runtime or zero interval).
    pub fn start(self) -> Result<TracingTokioSession, TracingTokioSessionStartError> {
        let mut recorder_builder = TracingRecorder::builder(self.service_name.clone())
            .strict(self.strict)
            .limits(self.limits);
        if let Some(service_version) = self.service_version {
            recorder_builder = recorder_builder.service_version(service_version);
        }
        if let Some(run_id) = self.run_id.clone() {
            recorder_builder = recorder_builder.run_id(run_id);
        }
        let recorder = recorder_builder.build();

        let mut runtime_builder = Tailtriage::builder(self.service_name.clone());
        if let Some(run_id) = self.run_id.clone() {
            runtime_builder = runtime_builder.run_id(run_id);
        }
        let runtime_tailtriage = Arc::new(
            runtime_builder
                .sink(MemorySink::new())
                .build()
                .map_err(|_| {
                    TracingTokioSessionStartError::Import(ImportError::EmptyServiceName)
                })?,
        );

        let mut sampler_builder = RuntimeSampler::builder(Arc::clone(&runtime_tailtriage));
        if let Some(interval) = self.sampler_interval {
            sampler_builder = sampler_builder.interval(interval);
        }
        if let Some(max_runtime_snapshots) = self.max_runtime_snapshots {
            sampler_builder = sampler_builder.max_runtime_snapshots(max_runtime_snapshots);
        }
        let runtime_sampler = sampler_builder
            .start()
            .map_err(TracingTokioSessionStartError::SamplerStart)?;

        Ok(TracingTokioSession {
            recorder,
            runtime_tailtriage,
            runtime_sampler,
        })
    }
}

fn merge_runtime_data(tracing_run: ImportedRun, runtime_run: tailtriage_core::Run) -> ImportedRun {
    let (mut run, warnings) = tracing_run.into_parts();
    run.runtime_snapshots = runtime_run.runtime_snapshots;
    run.metadata.effective_tokio_sampler_config =
        runtime_run.metadata.effective_tokio_sampler_config;
    run.truncation.dropped_runtime_snapshots = runtime_run.truncation.dropped_runtime_snapshots;
    for warning in runtime_run.metadata.lifecycle_warnings {
        if warning.contains("runtime") {
            run.metadata.lifecycle_warnings.push(warning);
        }
    }
    ImportedRun::new(run, warnings)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tracing::info_span;
    use tracing_subscriber::prelude::*;

    use super::*;

    #[test]
    fn start_outside_runtime_fails_clearly() {
        let err = TracingTokioSession::builder("svc")
            .sampler_interval(Duration::from_millis(1))
            .start()
            .expect_err("must fail outside Tokio runtime");
        match err {
            TracingTokioSessionStartError::SamplerStart(SamplerStartError::MissingRuntime) => {}
            other => panic!("unexpected error: {other}"),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn session_merges_tracing_and_runtime() {
        let session = TracingTokioSession::builder("svc")
            .sampler_interval(Duration::from_millis(1))
            .start()
            .expect("session starts");
        let _guard = tracing_subscriber::registry()
            .with(session.layer())
            .set_default();
        let span = info_span!(
            "req",
            tt.kind = "request",
            tt.request_id = "r1",
            tt.route = "/r"
        );
        let entered = span.enter();
        drop(entered);
        drop(span);
        tokio::time::sleep(Duration::from_millis(3)).await;
        let run = session.shutdown().await.expect("shutdown ok");
        assert!(!run.run().runtime_snapshots.is_empty());
        assert!(run.run().metadata.effective_tokio_sampler_config.is_some());
        assert_eq!(run.run().requests.len(), 1);
    }
}
