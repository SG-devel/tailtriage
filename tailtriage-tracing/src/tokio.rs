use std::sync::Arc;
use std::time::Duration;

use tailtriage_core::{
    BuildError, CaptureLimitsOverride, MemorySink, Run, RuntimeSnapshot, Tailtriage,
};
use tailtriage_tokio::{RuntimeSampler, SamplerStartError};

use crate::{ImportError, ImportedRun, RecorderLimits, TailtriageLayer, TracingRecorder};

/// Error returned when starting [`TracingTokioSession`].
#[derive(Debug)]
#[non_exhaustive]
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
#[non_exhaustive]
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
        let imported = self.recorder.snapshot_run()?;
        let runtime = self.runtime_collector.snapshot();
        Ok(merge_runtime_data(imported, &runtime))
    }

    /// Records one runtime snapshot directly into the internal runtime collector.
    ///
    /// This complements live Tokio sampling for deterministic workload-coupled snapshots.
    pub fn record_runtime_snapshot(&self, snapshot: RuntimeSnapshot) {
        self.runtime_collector.record_runtime_snapshot(snapshot);
    }

    /// Stops runtime sampling and returns one merged imported run.
    ///
    /// # Errors
    ///
    /// Returns [`TracingTokioSessionShutdownError::Import`] when strict span import fails.
    pub async fn shutdown(self) -> Result<ImportedRun, TracingTokioSessionShutdownError> {
        self.sampler.shutdown().await;
        let imported = self
            .recorder
            .snapshot_run()
            .map_err(TracingTokioSessionShutdownError::Import)?;
        let runtime = self.runtime_collector.snapshot();
        Ok(merge_runtime_data(imported, &runtime))
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

fn merge_runtime_data(imported: ImportedRun, runtime_run: &Run) -> ImportedRun {
    let (mut tracing_run, warnings) = imported.into_parts();
    tracing_run
        .runtime_snapshots
        .clone_from(&runtime_run.runtime_snapshots);
    tracing_run.metadata.effective_tokio_sampler_config =
        runtime_run.metadata.effective_tokio_sampler_config;
    tracing_run.truncation.dropped_runtime_snapshots =
        runtime_run.truncation.dropped_runtime_snapshots;
    tracing_run.truncation.limits_hit =
        tracing_run.truncation.limits_hit || runtime_run.truncation.limits_hit;
    for warning in &runtime_run.metadata.lifecycle_warnings {
        if !tracing_run.metadata.lifecycle_warnings.contains(warning) {
            tracing_run
                .metadata
                .lifecycle_warnings
                .push(warning.clone());
        }
    }
    ImportedRun::new(tracing_run, warnings)
}

#[cfg(test)]
mod tests {
    use super::merge_runtime_data;
    use crate::ImportedRun;
    use tailtriage_core::{MemorySink, Tailtriage};

    fn empty_run(service_name: &str) -> tailtriage_core::Run {
        Tailtriage::builder(service_name)
            .sink(MemorySink::new())
            .build()
            .expect("build collector")
            .snapshot()
    }

    #[test]
    fn merge_runtime_data_preserves_tracing_events_and_merges_runtime_fields() {
        let mut tracing_run = empty_run("tracing");
        tracing_run.requests.push(tailtriage_core::RequestEvent {
            request_id: "r1".into(),
            route: "/r1".into(),
            kind: Some("http".into()),
            started_at_unix_ms: 1,
            finished_at_unix_ms: 2,
            latency_us: 1_000,
            outcome: "ok".into(),
        });
        tracing_run.stages.push(tailtriage_core::StageEvent {
            request_id: "r1".into(),
            stage: "db".into(),
            started_at_unix_ms: 1,
            finished_at_unix_ms: 2,
            latency_us: 1_000,
            success: true,
        });
        tracing_run.queues.push(tailtriage_core::QueueEvent {
            request_id: "r1".into(),
            queue: "global".into(),
            waited_from_unix_ms: 1,
            waited_until_unix_ms: 2,
            wait_us: 1_000,
            depth_at_start: Some(2),
        });
        tracing_run.metadata.lifecycle_warnings = vec!["trace-warning".into(), "shared".into()];
        tracing_run.truncation.limits_hit = false;

        let mut runtime_run = empty_run("runtime");
        runtime_run
            .runtime_snapshots
            .push(tailtriage_core::RuntimeSnapshot {
                at_unix_ms: 10,
                alive_tasks: Some(3),
                global_queue_depth: Some(4),
                local_queue_depth: Some(5),
                blocking_queue_depth: Some(6),
                remote_schedule_count: Some(7),
            });
        runtime_run.metadata.effective_tokio_sampler_config =
            Some(tailtriage_core::EffectiveTokioSamplerConfig {
                inherited_mode: tailtriage_core::CaptureMode::Light,
                explicit_mode_override: None,
                resolved_mode: tailtriage_core::CaptureMode::Light,
                resolved_sampler_cadence_ms: 25,
                resolved_runtime_snapshot_retention: 10,
            });
        runtime_run.truncation.dropped_runtime_snapshots = 7;
        runtime_run.truncation.limits_hit = true;
        runtime_run.metadata.lifecycle_warnings = vec!["shared".into(), "non-runtime".into()];

        let merged =
            merge_runtime_data(ImportedRun::new(tracing_run.clone(), vec![]), &runtime_run);
        let run = merged.run();
        assert_eq!(run.requests, tracing_run.requests);
        assert_eq!(run.stages, tracing_run.stages);
        assert_eq!(run.queues, tracing_run.queues);
        assert_eq!(run.runtime_snapshots, runtime_run.runtime_snapshots);
        assert_eq!(
            run.metadata.effective_tokio_sampler_config,
            runtime_run.metadata.effective_tokio_sampler_config
        );
        assert_eq!(run.truncation.dropped_runtime_snapshots, 7);
        assert!(run.truncation.limits_hit);
        assert_eq!(
            run.metadata.lifecycle_warnings,
            vec!["trace-warning", "shared", "non-runtime"]
        );
        assert_eq!(run.requests.len(), 1);
    }
}
