use std::sync::Arc;
use std::time::Duration;

use tailtriage_core::{
    BuildError, CaptureLimits, CaptureLimitsOverride, CaptureMode, MemorySink, Run,
    RuntimeSnapshot, Tailtriage,
};
use tailtriage_tokio::{RuntimeSampler, SamplerStartError};

use crate::{
    ImportError, ImportWarning, ImportedRun, RecorderLimits, TailtriageLayer, TracingRecorder,
};

/// Error returned when starting [`TracingTokioSession`].
#[derive(Debug)]
#[non_exhaustive]
pub enum TracingTokioSessionStartError {
    /// Tracing recorder/import configuration failed validation.
    Import(ImportError),
    /// Internal runtime collector setup failed.
    Build(BuildError),
    /// Tokio runtime sampler failed to start.
    SamplerStart(SamplerStartError),
}

impl core::fmt::Display for TracingTokioSessionStartError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Import(err) => {
                write!(f, "failed to configure tracing recorder for startup: {err}")
            }
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
    /// Sets tracing-recorder-specific live memory limits.
    ///
    /// `max_open_spans` bounds concurrently open candidate spans.
    /// `max_completed_candidate_spans` bounds closed raw candidate spans waiting for semantic conversion.
    /// Request/stage/queue semantic retention is configured with [`Self::mode`],
    /// [`Self::capture_limits`], or [`Self::capture_limits_override`].
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
    /// Sets maximum retained closed raw completed candidate spans before semantic conversion.
    ///
    /// This is a live recorder memory cap. Request/stage/queue semantic retention remains
    /// controlled by [`Self::mode`], [`Self::capture_limits`], and [`Self::capture_limits_override`].
    #[must_use]
    pub fn max_completed_candidate_spans(mut self, max_completed_candidate_spans: usize) -> Self {
        self.recorder_builder = self
            .recorder_builder
            .max_completed_candidate_spans(max_completed_candidate_spans);
        self
    }
    /// Sets capture mode used to resolve live completed-evidence retention limits.
    #[must_use]
    pub fn mode(mut self, mode: CaptureMode) -> Self {
        self.recorder_builder = self.recorder_builder.mode(mode);
        self
    }
    /// Sets base capture limits used for live completed-evidence retention.
    #[must_use]
    pub fn capture_limits(mut self, limits: CaptureLimits) -> Self {
        self.recorder_builder = self.recorder_builder.capture_limits(limits);
        self
    }
    /// Sets capture-limit overrides applied on top of the selected capture mode.
    #[must_use]
    pub fn capture_limits_override(mut self, overrides: CaptureLimitsOverride) -> Self {
        self.recorder_builder = self.recorder_builder.capture_limits_override(overrides);
        self
    }
    /// Sets runtime sampler interval.
    #[must_use]
    pub fn sampler_interval(mut self, sampler_interval: Duration) -> Self {
        self.sampler_interval = Some(sampler_interval);
        self
    }
    /// Builds the session and starts Tokio runtime sampling.
    ///
    /// # Errors
    ///
    /// Returns [`TracingTokioSessionStartError`] when tracing recorder configuration is invalid,
    /// when runtime collector build fails, when sampler interval is zero, or when there is no active Tokio runtime.
    pub fn start(self) -> Result<TracingTokioSession, TracingTokioSessionStartError> {
        let resolved_limits = self.recorder_builder.resolved_capture_limits();
        let recorder = self
            .recorder_builder
            .build()
            .map_err(TracingTokioSessionStartError::Import)?;
        let sink = MemorySink::new();
        let builder = Tailtriage::builder("tailtriage-tracing-runtime")
            .sink(sink)
            .strict_lifecycle(false)
            .capture_limits(resolved_limits);
        if let Some(interval) = self.sampler_interval {
            if interval.is_zero() {
                return Err(TracingTokioSessionStartError::SamplerStart(
                    SamplerStartError::ZeroInterval,
                ));
            }
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
        let sampler_builder =
            sampler_builder.max_runtime_snapshots(resolved_limits.max_runtime_snapshots);
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
    let (mut tracing_run, mut warnings) = imported.into_parts();
    tracing_run
        .runtime_snapshots
        .clone_from(&runtime_run.runtime_snapshots);
    if !tracing_run.runtime_snapshots.is_empty() {
        let runtime_min = tracing_run
            .runtime_snapshots
            .iter()
            .map(|snapshot| snapshot.at_unix_ms)
            .min()
            .expect("non-empty runtime snapshots have a minimum timestamp");
        let runtime_max = tracing_run
            .runtime_snapshots
            .iter()
            .map(|snapshot| snapshot.at_unix_ms)
            .max()
            .expect("non-empty runtime snapshots have a maximum timestamp");

        tracing_run.metadata.started_at_unix_ms =
            tracing_run.metadata.started_at_unix_ms.min(runtime_min);
        tracing_run.metadata.finished_at_unix_ms =
            tracing_run.metadata.finished_at_unix_ms.max(runtime_max);

        let finalized = tracing_run
            .metadata
            .finalized_at_unix_ms
            .unwrap_or(tracing_run.metadata.finished_at_unix_ms)
            .max(tracing_run.metadata.finished_at_unix_ms);
        tracing_run.metadata.finalized_at_unix_ms = Some(finalized);
    }
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
        if !warnings
            .iter()
            .any(|import_warning| import_warning.message() == warning)
        {
            warnings.push(ImportWarning::new(warning.clone()));
        }
    }
    ImportedRun::new(tracing_run, warnings)
}

#[cfg(test)]
mod tests {
    use super::merge_runtime_data;
    use super::{TracingTokioSession, TracingTokioSessionStartError};
    use crate::{ImportError, ImportWarning, ImportedRun};
    use tailtriage_core::{MemorySink, RuntimeSnapshot, Tailtriage};

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

    #[test]
    fn merge_runtime_data_runtime_snapshots_expand_metadata_bounds() {
        let mut tracing_run = empty_run("tracing");
        tracing_run.metadata.started_at_unix_ms = 1_500;
        tracing_run.metadata.finished_at_unix_ms = 1_800;
        tracing_run.metadata.finalized_at_unix_ms = Some(1_800);

        let mut runtime_run = empty_run("runtime");
        runtime_run.runtime_snapshots = vec![
            RuntimeSnapshot {
                at_unix_ms: 1_000,
                alive_tasks: None,
                global_queue_depth: None,
                local_queue_depth: None,
                blocking_queue_depth: None,
                remote_schedule_count: None,
            },
            RuntimeSnapshot {
                at_unix_ms: 2_200,
                alive_tasks: None,
                global_queue_depth: None,
                local_queue_depth: None,
                blocking_queue_depth: None,
                remote_schedule_count: None,
            },
        ];

        let merged = merge_runtime_data(ImportedRun::new(tracing_run, vec![]), &runtime_run);
        let run = merged.run();
        assert_eq!(run.metadata.started_at_unix_ms, 1_000);
        assert_eq!(run.metadata.finished_at_unix_ms, 2_200);
        assert_eq!(run.metadata.finalized_at_unix_ms, Some(2_200));
        assert_eq!(run.runtime_snapshots, runtime_run.runtime_snapshots);
        assert!(run.runtime_snapshots.iter().all(|snapshot| {
            snapshot.at_unix_ms >= run.metadata.started_at_unix_ms
                && snapshot.at_unix_ms <= run.metadata.finished_at_unix_ms
        }));
    }

    #[test]
    fn merge_runtime_data_without_runtime_snapshots_preserves_metadata_bounds() {
        let mut tracing_run = empty_run("tracing");
        tracing_run.metadata.started_at_unix_ms = 1_500;
        tracing_run.metadata.finished_at_unix_ms = 1_800;
        tracing_run.metadata.finalized_at_unix_ms = Some(1_900);

        let runtime_run = empty_run("runtime");

        let merged =
            merge_runtime_data(ImportedRun::new(tracing_run.clone(), vec![]), &runtime_run);
        let run = merged.run();
        assert_eq!(
            run.metadata.started_at_unix_ms,
            tracing_run.metadata.started_at_unix_ms
        );
        assert_eq!(
            run.metadata.finished_at_unix_ms,
            tracing_run.metadata.finished_at_unix_ms
        );
        assert_eq!(
            run.metadata.finalized_at_unix_ms,
            tracing_run.metadata.finalized_at_unix_ms
        );
    }

    #[test]
    fn merge_runtime_data_does_not_move_finalized_backwards() {
        let mut tracing_run = empty_run("tracing");
        tracing_run.metadata.started_at_unix_ms = 1_500;
        tracing_run.metadata.finished_at_unix_ms = 1_800;
        tracing_run.metadata.finalized_at_unix_ms = Some(2_500);

        let mut runtime_run = empty_run("runtime");
        runtime_run.runtime_snapshots = vec![RuntimeSnapshot {
            at_unix_ms: 2_200,
            alive_tasks: None,
            global_queue_depth: None,
            local_queue_depth: None,
            blocking_queue_depth: None,
            remote_schedule_count: None,
        }];

        let merged = merge_runtime_data(ImportedRun::new(tracing_run, vec![]), &runtime_run);
        let run = merged.run();
        assert_eq!(run.metadata.finished_at_unix_ms, 2_200);
        assert_eq!(run.metadata.finalized_at_unix_ms, Some(2_500));
        assert!(run.runtime_snapshots.iter().all(|snapshot| {
            snapshot.at_unix_ms >= run.metadata.started_at_unix_ms
                && snapshot.at_unix_ms <= run.metadata.finished_at_unix_ms
        }));
    }

    #[test]
    fn merge_runtime_data_repairs_missing_finalized_when_runtime_snapshots_present() {
        let mut tracing_run = empty_run("tracing");
        tracing_run.metadata.started_at_unix_ms = 1_500;
        tracing_run.metadata.finished_at_unix_ms = 1_800;
        tracing_run.metadata.finalized_at_unix_ms = None;

        let mut runtime_run = empty_run("runtime");
        runtime_run.runtime_snapshots = vec![RuntimeSnapshot {
            at_unix_ms: 1_900,
            alive_tasks: None,
            global_queue_depth: None,
            local_queue_depth: None,
            blocking_queue_depth: None,
            remote_schedule_count: None,
        }];

        let merged = merge_runtime_data(ImportedRun::new(tracing_run, vec![]), &runtime_run);
        let run = merged.run();
        assert_eq!(
            run.metadata.finalized_at_unix_ms,
            Some(run.metadata.finished_at_unix_ms)
        );
    }

    #[test]
    fn merge_runtime_data_adds_runtime_lifecycle_warning_to_metadata_and_import_warnings() {
        let tracing_run = empty_run("tracing");
        let mut runtime_run = empty_run("runtime");
        runtime_run.metadata.lifecycle_warnings = vec!["runtime-warning".into()];

        let merged = merge_runtime_data(ImportedRun::new(tracing_run, vec![]), &runtime_run);
        assert_eq!(
            merged.run().metadata.lifecycle_warnings,
            vec!["runtime-warning"]
        );
        assert_eq!(
            merged.warnings(),
            &[ImportWarning::new("runtime-warning".to_string())]
        );
    }

    #[test]
    fn merge_runtime_data_deduplicates_warning_messages_in_imported_run_warnings() {
        let tracing_run = empty_run("tracing");
        let mut runtime_run = empty_run("runtime");
        runtime_run.metadata.lifecycle_warnings = vec![
            "shared-warning".into(),
            "shared-warning".into(),
            "unique-warning".into(),
        ];
        let existing_warnings = vec![
            ImportWarning::new("shared-warning"),
            ImportWarning::new("existing-warning"),
        ];

        let merged = merge_runtime_data(
            ImportedRun::new(tracing_run, existing_warnings),
            &runtime_run,
        );

        assert_eq!(
            merged.run().metadata.lifecycle_warnings,
            vec!["shared-warning", "unique-warning"]
        );
        assert_eq!(
            merged.warnings(),
            &[
                ImportWarning::new("shared-warning"),
                ImportWarning::new("existing-warning"),
                ImportWarning::new("unique-warning"),
            ]
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn tracing_tokio_session_start_rejects_blank_service_name() {
        let err = TracingTokioSession::builder("   ")
            .start()
            .expect_err("blank service name should fail before sampler startup");
        assert!(matches!(
            err,
            TracingTokioSessionStartError::Import(ImportError::EmptyServiceName)
        ));
    }
}
