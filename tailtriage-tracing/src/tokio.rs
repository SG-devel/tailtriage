use std::sync::Arc;
use std::time::Duration;

use tailtriage_core::{BuildError, CaptureLimitsOverride, MemorySink, Run, Tailtriage};
use tailtriage_tokio::{RuntimeSampler, SamplerStartError};

use crate::{ImportError, ImportedRun, RecorderLimits, TailtriageLayer, TracingRecorder};

/// Error returned when starting [`TracingTokioSession`].
#[derive(Debug)]
pub enum TracingTokioSessionStartError {
    /// Underlying tracing recorder setup failed.
    Build(BuildError),
    /// Tokio runtime sampler failed to start.
    SamplerStart(SamplerStartError),
}

impl core::fmt::Display for TracingTokioSessionStartError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Build(err) => write!(f, "failed to build tailtriage runtime collector: {err}"),
            Self::SamplerStart(err) => write!(f, "failed to start Tokio runtime sampler: {err}"),
        }
    }
}

impl std::error::Error for TracingTokioSessionStartError {}

/// Error returned when shutting down [`TracingTokioSession`].
#[derive(Debug)]
pub enum TracingTokioSessionShutdownError {
    /// Snapshot import failed.
    Import(ImportError),
}

impl core::fmt::Display for TracingTokioSessionShutdownError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Import(err) => write!(f, "failed to import tracing spans during shutdown: {err}"),
        }
    }
}

impl std::error::Error for TracingTokioSessionShutdownError {}

/// Builder for [`TracingTokioSession`].
#[derive(Debug, Clone)]
pub struct TracingTokioSessionBuilder {
    recorder_builder: crate::TracingRecorderBuilder,
    sampler_interval: Option<Duration>,
    max_runtime_snapshots: Option<usize>,
}

/// Combined tracing and Tokio runtime sampler session.
#[derive(Debug)]
pub struct TracingTokioSession {
    recorder: TracingRecorder,
    runtime_collector: Arc<Tailtriage>,
    sampler: RuntimeSampler,
}

impl TracingTokioSession {
    /// Creates a builder with required service name metadata.
    pub fn builder(service_name: impl Into<String>) -> TracingTokioSessionBuilder {
        TracingTokioSessionBuilder {
            recorder_builder: TracingRecorder::builder(service_name),
            sampler_interval: None,
            max_runtime_snapshots: None,
        }
    }

    /// Returns a cloneable layer that captures spans for tracing import.
    #[must_use]
    pub fn layer(&self) -> TailtriageLayer {
        self.recorder.layer()
    }

    /// Converts currently completed spans and runtime snapshots into one imported run.
    ///
    /// # Errors
    ///
    /// Returns [`crate::ImportError`] when strict span import fails.
    pub fn snapshot_run(&self) -> Result<ImportedRun, crate::ImportError> {
        let mut imported = self.recorder.snapshot_run()?;
        let runtime = self.runtime_collector.snapshot();
        merge_runtime_data(imported.run_mut(), &runtime);
        Ok(imported)
    }

    /// Stops runtime sampling and returns one merged imported run.
    ///
    /// # Errors
    ///
    /// Returns [`TracingTokioSessionShutdownError::Import`] when strict span import fails.
    pub async fn shutdown(self) -> Result<ImportedRun, TracingTokioSessionShutdownError> {
        self.sampler.shutdown().await;
        let mut imported = self
            .recorder
            .snapshot_run()
            .map_err(TracingTokioSessionShutdownError::Import)?;
        let runtime = self.runtime_collector.snapshot();
        merge_runtime_data(imported.run_mut(), &runtime);
        Ok(imported)
    }
}

impl TracingTokioSessionBuilder {
    /// Sets service version metadata.
    #[must_use]
    pub fn service_version(mut self, service_version: impl Into<String>) -> Self {
        self.recorder_builder = self.recorder_builder.service_version(service_version);
        self
    }
    /// Sets explicit run-id metadata.
    #[must_use]
    pub fn run_id(mut self, run_id: impl Into<String>) -> Self {
        self.recorder_builder = self.recorder_builder.run_id(run_id);
        self
    }
    /// Enables or disables strict conversion mode.
    #[must_use]
    pub fn strict(mut self, strict: bool) -> Self {
        self.recorder_builder = self.recorder_builder.strict(strict);
        self
    }
    /// Sets both open/completed in-memory span retention limits.
    #[must_use]
    pub fn recorder_limits(mut self, limits: RecorderLimits) -> Self {
        self.recorder_builder = self.recorder_builder.limits(limits);
        self
    }
    /// Sets maximum number of concurrently tracked open candidate spans.
    #[must_use]
    pub fn max_open_spans(mut self, max_open_spans: usize) -> Self {
        self.recorder_builder = self.recorder_builder.max_open_spans(max_open_spans);
        self
    }
    /// Sets maximum number of retained completed candidate spans.
    #[must_use]
    pub fn max_completed_spans(mut self, max_completed_spans: usize) -> Self {
        self.recorder_builder = self
            .recorder_builder
            .max_completed_spans(max_completed_spans);
        self
    }
    /// Sets runtime sampler interval.
    #[must_use]
    pub fn sampler_interval(mut self, sampler_interval: Duration) -> Self {
        self.sampler_interval = Some(sampler_interval);
        self
    }
    /// Sets runtime sampler retention cap for runtime snapshots.
    #[must_use]
    pub fn max_runtime_snapshots(mut self, max_runtime_snapshots: usize) -> Self {
        self.max_runtime_snapshots = Some(max_runtime_snapshots);
        self
    }

    /// Builds the session and starts Tokio runtime sampling.
    ///
    /// # Errors
    ///
    /// Returns [`TracingTokioSessionStartError`] when runtime collector build fails,
    /// when sampler interval is zero, or when there is no active Tokio runtime.
    pub fn start(self) -> Result<TracingTokioSession, TracingTokioSessionStartError> {
        let recorder = self.recorder_builder.build();
        let sink = MemorySink::new();
        let mut builder = Tailtriage::builder("tailtriage-tracing-runtime")
            .sink(sink)
            .strict_lifecycle(false);
        if let Some(interval) = self.sampler_interval {
            if interval.is_zero() {
                return Err(TracingTokioSessionStartError::SamplerStart(
                    SamplerStartError::ZeroInterval,
                ));
            }
        }
        if let Some(limit) = self.max_runtime_snapshots {
            builder = builder.capture_limits_override(CaptureLimitsOverride {
                max_runtime_snapshots: Some(limit),
                ..CaptureLimitsOverride::default()
            });
        }
        let runtime_collector = Arc::new(
            builder
                .build()
                .map_err(TracingTokioSessionStartError::Build)?,
        );
        let sampler_builder = RuntimeSampler::builder(Arc::clone(&runtime_collector));
        let sampler_builder = if let Some(interval) = self.sampler_interval {
            sampler_builder.interval(interval)
        } else {
            sampler_builder
        };
        let sampler_builder = if let Some(limit) = self.max_runtime_snapshots {
            sampler_builder.max_runtime_snapshots(limit)
        } else {
            sampler_builder
        };
        let sampler = sampler_builder
            .start()
            .map_err(TracingTokioSessionStartError::SamplerStart)?;
        Ok(TracingTokioSession {
            recorder,
            runtime_collector,
            sampler,
        })
    }
}

fn merge_runtime_data(tracing_run: &mut Run, runtime_run: &Run) {
    tracing_run
        .runtime_snapshots
        .clone_from(&runtime_run.runtime_snapshots);
    tracing_run.metadata.effective_tokio_sampler_config =
        runtime_run.metadata.effective_tokio_sampler_config;
    tracing_run.truncation.dropped_runtime_snapshots =
        runtime_run.truncation.dropped_runtime_snapshots;
    tracing_run.truncation.limits_hit =
        tracing_run.truncation.limits_hit || runtime_run.truncation.limits_hit;
    let existing = tracing_run.metadata.lifecycle_warnings.clone();
    tracing_run.metadata.lifecycle_warnings.extend(
        runtime_run
            .metadata
            .lifecycle_warnings
            .iter()
            .filter(|warning| warning.contains("runtime") && !existing.contains(*warning))
            .cloned(),
    );
}
