use std::sync::Arc;
use std::time::Duration;

use tailtriage_core::{BuildError, MemorySink, Run, Tailtriage};

use crate::{ImportError, ImportedRun};
use tailtriage_tokio::{RuntimeSampler, SamplerStartError};

use crate::{RecorderLimits, TailtriageLayer, TracingRecorder};

/// Session that couples tracing span capture with optional Tokio runtime sampling.
#[derive(Debug)]
pub struct TracingTokioSession {
    recorder: TracingRecorder,
    sampler_tailtriage: Arc<Tailtriage>,
    sampler: RuntimeSampler,
}

/// Builder for [`TracingTokioSession`].
#[derive(Debug, Clone)]
pub struct TracingTokioSessionBuilder {
    service_name: String,
    service_version: Option<String>,
    run_id: Option<String>,
    strict: bool,
    recorder_limits: RecorderLimits,
    sampler_interval: Duration,
    max_runtime_snapshots: Option<usize>,
}

/// Errors returned when starting [`TracingTokioSession`].
#[derive(Debug)]
pub enum TracingTokioSessionStartError {
    /// Internal sampler collector could not be initialized.
    Build(BuildError),
    /// Tokio runtime sampler could not start.
    SamplerStart(SamplerStartError),
}

impl std::fmt::Display for TracingTokioSessionStartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Build(err) => write!(f, "{err}"),
            Self::SamplerStart(err) => write!(f, "{err}"),
        }
    }
}
impl std::error::Error for TracingTokioSessionStartError {}

/// Errors returned when shutting down [`TracingTokioSession`].
#[derive(Debug)]
pub enum TracingTokioSessionShutdownError {
    /// Tracing recorder import options were invalid.
    Snapshot(ImportError),
}

impl std::fmt::Display for TracingTokioSessionShutdownError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Snapshot(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for TracingTokioSessionShutdownError {}

impl TracingTokioSession {
    /// Creates a builder with required service name metadata.
    pub fn builder(service_name: impl Into<String>) -> TracingTokioSessionBuilder {
        TracingTokioSessionBuilder {
            service_name: service_name.into(),
            service_version: None,
            run_id: None,
            strict: false,
            recorder_limits: RecorderLimits::default(),
            sampler_interval: Duration::from_millis(500),
            max_runtime_snapshots: None,
        }
    }

    /// Returns the tracing layer for request/stage/queue span intake.
    #[must_use]
    pub fn layer(&self) -> TailtriageLayer {
        self.recorder.layer()
    }

    /// Returns a merged run with tracing-derived evidence and runtime sampler evidence.
    /// # Errors
    ///
    /// Returns [`ImportError`] when strict tracing conversion fails.
    pub fn snapshot_run(&self) -> Result<ImportedRun, ImportError> {
        let tracing_run = self.recorder.snapshot_run()?;
        Ok(merge_runtime_data(
            tracing_run,
            &self.sampler_tailtriage.snapshot(),
        ))
    }

    /// Stops runtime sampling and returns a merged final run.
    /// # Errors
    ///
    /// Returns [`TracingTokioSessionShutdownError`] when final tracing conversion fails.
    pub async fn shutdown(self) -> Result<ImportedRun, TracingTokioSessionShutdownError> {
        self.sampler.shutdown().await;
        let tracing_run = self
            .recorder
            .snapshot_run()
            .map_err(TracingTokioSessionShutdownError::Snapshot)?;
        Ok(merge_runtime_data(
            tracing_run,
            &self.sampler_tailtriage.snapshot(),
        ))
    }
}

impl TracingTokioSessionBuilder {
    #[must_use]
    /// Sets service version metadata used by tracing import conversion.
    pub fn service_version(mut self, service_version: impl Into<String>) -> Self {
        self.service_version = Some(service_version.into());
        self
    }
    #[must_use]
    /// Sets explicit run id metadata for both tracing and runtime sampler data.
    pub fn run_id(mut self, run_id: impl Into<String>) -> Self {
        self.run_id = Some(run_id.into());
        self
    }
    #[must_use]
    /// Enables or disables strict tracing span conversion.
    pub fn strict(mut self, strict: bool) -> Self {
        self.strict = strict;
        self
    }
    #[must_use]
    /// Sets tracing recorder retention limits.
    pub fn recorder_limits(mut self, limits: RecorderLimits) -> Self {
        self.recorder_limits = limits;
        self
    }
    #[must_use]
    /// Sets tracing recorder max open spans.
    pub fn max_open_spans(mut self, max_open_spans: usize) -> Self {
        self.recorder_limits.max_open_spans = max_open_spans;
        self
    }
    #[must_use]
    /// Sets tracing recorder max completed spans.
    pub fn max_completed_spans(mut self, max_completed_spans: usize) -> Self {
        self.recorder_limits.max_completed_spans = max_completed_spans;
        self
    }
    #[must_use]
    /// Sets Tokio runtime sampler interval.
    pub fn sampler_interval(mut self, sampler_interval: Duration) -> Self {
        self.sampler_interval = sampler_interval;
        self
    }
    #[must_use]
    /// Sets Tokio sampler runtime snapshot cap.
    pub fn max_runtime_snapshots(mut self, max_runtime_snapshots: usize) -> Self {
        self.max_runtime_snapshots = Some(max_runtime_snapshots);
        self
    }

    /// Starts a coupled tracing + Tokio runtime sampler session.
    ///
    /// # Errors
    ///
    /// Returns [`TracingTokioSessionStartError`] when internal sampler storage cannot be built
    /// or when runtime sampler startup fails (for example zero interval or missing Tokio runtime).
    pub fn start(self) -> Result<TracingTokioSession, TracingTokioSessionStartError> {
        let mut recorder_builder = TracingRecorder::builder(self.service_name)
            .strict(self.strict)
            .limits(self.recorder_limits);
        if let Some(service_version) = self.service_version {
            recorder_builder = recorder_builder.service_version(service_version);
        }
        if let Some(run_id) = self.run_id.as_ref() {
            recorder_builder = recorder_builder.run_id(run_id.clone());
        }
        let recorder = recorder_builder.build();
        let mut tailtriage_builder = Tailtriage::builder("tailtriage-tracing-tokio")
            .sink(MemorySink::new())
            .light();
        if let Some(run_id) = self.run_id.as_ref() {
            tailtriage_builder = tailtriage_builder.run_id(run_id.clone());
        }
        let sampler_tailtriage = Arc::new(
            tailtriage_builder
                .build()
                .map_err(TracingTokioSessionStartError::Build)?,
        );
        let mut sampler_builder = RuntimeSampler::builder(Arc::clone(&sampler_tailtriage))
            .interval(self.sampler_interval);
        if let Some(max_runtime_snapshots) = self.max_runtime_snapshots {
            sampler_builder = sampler_builder.max_runtime_snapshots(max_runtime_snapshots);
        }
        let sampler = sampler_builder
            .start()
            .map_err(TracingTokioSessionStartError::SamplerStart)?;
        Ok(TracingTokioSession {
            recorder,
            sampler_tailtriage,
            sampler,
        })
    }
}

fn merge_runtime_data(tracing_run: ImportedRun, runtime: &Run) -> ImportedRun {
    let (mut run, warnings) = tracing_run.into_parts();
    run.runtime_snapshots.clone_from(&runtime.runtime_snapshots);
    run.metadata.effective_tokio_sampler_config = runtime.metadata.effective_tokio_sampler_config;
    run.truncation.dropped_runtime_snapshots = runtime.truncation.dropped_runtime_snapshots;
    for warning in &runtime.metadata.lifecycle_warnings {
        if !run
            .metadata
            .lifecycle_warnings
            .iter()
            .any(|current| current == warning)
        {
            run.metadata.lifecycle_warnings.push(warning.clone());
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

    #[tokio::test(flavor = "current_thread")]
    async fn session_merges_tracing_and_runtime_evidence() {
        let session = TracingTokioSession::builder("svc")
            .sampler_interval(Duration::from_millis(1))
            .start()
            .expect("session should start");

        let subscriber = tracing_subscriber::registry().with(session.layer());
        tracing::subscriber::with_default(subscriber, || {
            let request = info_span!(
                "request",
                tt.kind = "request",
                tt.request_id = "req-1",
                tt.route = "/checkout"
            );
            let _request_guard = request.enter();
            let stage = info_span!(
                "stage",
                tt.kind = "stage",
                tt.request_id = "req-1",
                tt.stage = "db"
            );
            let _stage_guard = stage.enter();
        });

        tokio::time::sleep(Duration::from_millis(5)).await;

        let run = session.shutdown().await.expect("shutdown should work");
        assert!(!run.run().requests.is_empty());
        assert!(!run.run().stages.is_empty());
        assert!(!run.run().runtime_snapshots.is_empty());
        assert!(run.run().metadata.effective_tokio_sampler_config.is_some());
    }

    #[test]
    fn session_start_outside_runtime_fails_clearly() {
        let err = TracingTokioSession::builder("svc")
            .sampler_interval(Duration::from_millis(5))
            .start()
            .expect_err("missing runtime should fail");
        match err {
            TracingTokioSessionStartError::SamplerStart(SamplerStartError::MissingRuntime) => {}
            other => panic!("unexpected error: {other}"),
        }
    }

    fn merged_metadata_for_test() -> tailtriage_core::RunMetadata {
        TracingRecorder::builder("svc")
            .build()
            .snapshot_run()
            .unwrap()
            .run()
            .metadata
            .clone()
    }

    #[test]
    fn merge_runtime_data_keeps_tracing_events_without_duplication() {
        let tracing = TracingRecorder::builder("svc")
            .build()
            .snapshot_run()
            .unwrap();
        let runtime = tailtriage_core::Run::new(merged_metadata_for_test());
        let merged = merge_runtime_data(tracing, &runtime);
        assert!(merged.run().requests.is_empty());
    }
}
