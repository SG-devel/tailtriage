#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

//! Tokio runtime integration for tailtriage.
//!
//! This crate provides [`RuntimeSampler`] for periodic Tokio runtime metrics
//! snapshots that feed evidence into the same unified `Tailtriage` API surface.
//! Use it when you need stronger separation between executor pressure,
//! blocking-pool pressure, queueing, and downstream-stage slowdowns.

use std::sync::Arc;
use std::time::Duration;

use tailtriage_core::{
    unix_time_ms, CaptureMode, EffectiveTokioSamplerConfig, RuntimeSnapshot, Tailtriage,
};
use tokio::runtime::Handle;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

/// Returns the crate name for smoke-testing workspace wiring.
#[must_use]
pub const fn crate_name() -> &'static str {
    "tailtriage-tokio"
}

/// Errors produced while starting runtime sampling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SamplerStartError {
    /// Sampling interval must be greater than zero.
    ZeroInterval,
}

impl std::fmt::Display for SamplerStartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ZeroInterval => write!(f, "runtime sampling interval must be greater than zero"),
        }
    }
}

impl std::error::Error for SamplerStartError {}

/// Periodically samples Tokio runtime metrics and records them into a [`Tailtriage`] run.
#[derive(Debug)]
pub struct RuntimeSampler {
    stop_tx: Option<oneshot::Sender<()>>,
    task: JoinHandle<()>,
}

/// Tokio-owned defaults for runtime sampler behavior by capture mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokioSamplerModeDefaults {
    /// Default periodic sampler cadence.
    pub cadence: Duration,
    /// Default maximum number of runtime snapshots this sampler should record.
    pub max_runtime_snapshots: usize,
}

impl TokioSamplerModeDefaults {
    /// Returns Tokio-owned runtime sampler defaults for one capture mode.
    #[must_use]
    pub const fn for_mode(mode: CaptureMode) -> Self {
        match mode {
            CaptureMode::Light => Self {
                cadence: Duration::from_millis(500),
                max_runtime_snapshots: 5_000,
            },
            CaptureMode::Investigation => Self {
                cadence: Duration::from_millis(100),
                max_runtime_snapshots: 50_000,
            },
        }
    }
}

/// Builder for configuring and starting [`RuntimeSampler`].
#[derive(Debug)]
pub struct RuntimeSamplerBuilder {
    tailtriage: Arc<Tailtriage>,
    explicit_mode_override: Option<CaptureMode>,
    interval_override: Option<Duration>,
    max_runtime_snapshots_override: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ResolvedRuntimeSamplerConfig {
    inherited_mode: CaptureMode,
    explicit_mode_override: Option<CaptureMode>,
    resolved_mode: CaptureMode,
    resolved_interval: Duration,
    resolved_max_runtime_snapshots: usize,
}

impl RuntimeSampler {
    /// Creates a builder for configuring runtime sampling.
    #[must_use]
    pub fn builder(tailtriage: Arc<Tailtriage>) -> RuntimeSamplerBuilder {
        RuntimeSamplerBuilder {
            tailtriage,
            explicit_mode_override: None,
            interval_override: None,
            max_runtime_snapshots_override: None,
        }
    }

    /// Starts periodic runtime metrics sampling on the current Tokio runtime.
    ///
    /// Use this during incident triage when runtime pressure evidence is needed
    /// to rank suspects (for example: global queue growth or alive-task spikes).
    /// For minimal-overhead capture, skip sampler startup.
    ///
    /// # Errors
    ///
    /// Returns [`SamplerStartError::ZeroInterval`] when `interval` is zero.
    pub fn start(
        tailtriage: Arc<Tailtriage>,
        interval: Duration,
    ) -> Result<Self, SamplerStartError> {
        Self::builder(tailtriage).interval(interval).start()
    }

    /// Requests sampler shutdown and waits for task completion.
    pub async fn shutdown(mut self) {
        if let Some(stop_tx) = self.stop_tx.take() {
            let _ = stop_tx.send(());
        }
        let _ = self.task.await;
    }
}

impl RuntimeSamplerBuilder {
    /// Overrides mode inheritance with an explicit Tokio-side capture mode.
    #[must_use]
    pub fn mode(mut self, mode: CaptureMode) -> Self {
        self.explicit_mode_override = Some(mode);
        self
    }

    /// Overrides resolved sampler cadence.
    #[must_use]
    pub fn interval(mut self, interval: Duration) -> Self {
        self.interval_override = Some(interval);
        self
    }

    /// Overrides resolved runtime snapshot retention for Tokio sampling.
    #[must_use]
    pub fn max_runtime_snapshots(mut self, max_runtime_snapshots: usize) -> Self {
        self.max_runtime_snapshots_override = Some(max_runtime_snapshots);
        self
    }

    /// Resolves configuration and starts periodic runtime metrics sampling.
    ///
    /// # Errors
    ///
    /// Returns [`SamplerStartError::ZeroInterval`] when resolved cadence is zero.
    pub fn start(self) -> Result<RuntimeSampler, SamplerStartError> {
        let resolved = self.resolve_config()?;
        self.tailtriage
            .record_tokio_sampler_config(resolved.into_effective_metadata());

        let tailtriage = Arc::clone(&self.tailtriage);
        let handle = Handle::current();
        let (stop_tx, mut stop_rx) = oneshot::channel();
        let mut ticker = tokio::time::interval(resolved.resolved_interval);
        let mut captured: usize = 0;
        let max_runtime_snapshots = resolved.resolved_max_runtime_snapshots;

        let task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = &mut stop_rx => break,
                    _ = ticker.tick() => {
                        if captured >= max_runtime_snapshots {
                            break;
                        }

                        tailtriage.record_runtime_snapshot(capture_runtime_snapshot(&handle));
                        captured = captured.saturating_add(1);
                    }
                }
            }
        });

        Ok(RuntimeSampler {
            stop_tx: Some(stop_tx),
            task,
        })
    }

    fn resolve_config(&self) -> Result<ResolvedRuntimeSamplerConfig, SamplerStartError> {
        let inherited_mode = self.tailtriage.selected_mode();
        let resolved_mode = self.explicit_mode_override.unwrap_or(inherited_mode);
        let mode_defaults = TokioSamplerModeDefaults::for_mode(resolved_mode);
        let resolved_interval = self.interval_override.unwrap_or(mode_defaults.cadence);
        if resolved_interval.is_zero() {
            return Err(SamplerStartError::ZeroInterval);
        }

        let requested_retention = self
            .max_runtime_snapshots_override
            .unwrap_or(mode_defaults.max_runtime_snapshots);
        let core_runtime_snapshot_cap = self
            .tailtriage
            .effective_core_config()
            .capture_limits
            .max_runtime_snapshots;
        let resolved_max_runtime_snapshots = requested_retention.min(core_runtime_snapshot_cap);

        Ok(ResolvedRuntimeSamplerConfig {
            inherited_mode,
            explicit_mode_override: self.explicit_mode_override,
            resolved_mode,
            resolved_interval,
            resolved_max_runtime_snapshots,
        })
    }
}

impl ResolvedRuntimeSamplerConfig {
    fn into_effective_metadata(self) -> EffectiveTokioSamplerConfig {
        let resolved_sampler_cadence_ms = self.resolved_interval.as_millis();
        let resolved_sampler_cadence_ms =
            u64::try_from(resolved_sampler_cadence_ms).unwrap_or(u64::MAX);

        EffectiveTokioSamplerConfig {
            inherited_mode: self.inherited_mode,
            explicit_mode_override: self.explicit_mode_override,
            resolved_mode: self.resolved_mode,
            resolved_sampler_cadence_ms,
            resolved_runtime_snapshot_retention: self.resolved_max_runtime_snapshots,
        }
    }
}

/// Captures one point-in-time runtime metrics snapshot from `handle`.
#[must_use]
pub fn capture_runtime_snapshot(handle: &Handle) -> RuntimeSnapshot {
    let metrics = handle.metrics();

    #[cfg(tokio_unstable)]
    let local_queue_depth = {
        let worker_count: usize = metrics.num_workers();
        (0..worker_count).try_fold(0_u64, |sum, worker| {
            let worker_depth: u64 = metrics.worker_local_queue_depth(worker).try_into().ok()?;
            sum.checked_add(worker_depth)
        })
    };

    #[cfg(not(tokio_unstable))]
    let local_queue_depth = None;

    #[cfg(tokio_unstable)]
    let blocking_queue_depth = u64::try_from(metrics.blocking_queue_depth()).ok();

    #[cfg(not(tokio_unstable))]
    let blocking_queue_depth = None;

    #[cfg(tokio_unstable)]
    let remote_schedule_count = Some(metrics.remote_schedule_count());

    #[cfg(not(tokio_unstable))]
    let remote_schedule_count = None;

    RuntimeSnapshot {
        at_unix_ms: unix_time_ms(),
        alive_tasks: u64::try_from(metrics.num_alive_tasks()).ok(),
        global_queue_depth: u64::try_from(metrics.global_queue_depth()).ok(),
        local_queue_depth,
        blocking_queue_depth,
        remote_schedule_count,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use tailtriage_core::{CaptureMode, Tailtriage};

    use super::crate_name;
    use super::{RuntimeSampler, SamplerStartError};

    #[test]
    fn crate_name_is_stable() {
        assert_eq!(crate_name(), "tailtriage-tokio");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn runtime_sampler_records_snapshots() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time before epoch")
            .as_nanos();

        let tailtriage = Arc::new(
            Tailtriage::builder("runtime-test")
                .output(std::env::temp_dir().join(format!("tailtriage_tokio_sampler_{nanos}.json")))
                .build()
                .expect("build should succeed"),
        );
        let sampler = RuntimeSampler::builder(Arc::clone(&tailtriage))
            .interval(Duration::from_millis(5))
            .start()
            .expect("sampler should start");

        tokio::time::sleep(Duration::from_millis(20)).await;
        sampler.shutdown().await;

        let snapshot = tailtriage.snapshot();
        assert!(
            !snapshot.runtime_snapshots.is_empty(),
            "sampler should record runtime snapshots"
        );

        let first = &snapshot.runtime_snapshots[0];
        assert!(first.alive_tasks.is_some());
        assert!(first.global_queue_depth.is_some());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn runtime_sampler_rejects_zero_interval() {
        let tailtriage = Arc::new(
            Tailtriage::builder("runtime-test")
                .output(std::env::temp_dir().join("tailtriage_tokio_zero_interval.json"))
                .build()
                .expect("build should succeed"),
        );

        let err = RuntimeSampler::start(tailtriage, Duration::ZERO)
            .expect_err("zero interval should fail");
        assert_eq!(err, SamplerStartError::ZeroInterval);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn core_light_inherits_tokio_light_defaults() {
        let tailtriage = Arc::new(
            Tailtriage::builder("runtime-test")
                .output(std::env::temp_dir().join("tailtriage_tokio_inherit_light.json"))
                .light()
                .build()
                .expect("build should succeed"),
        );

        let sampler = RuntimeSampler::builder(Arc::clone(&tailtriage))
            .start()
            .expect("sampler should start");
        sampler.shutdown().await;

        let snapshot = tailtriage.snapshot();
        let config = snapshot
            .metadata
            .effective_tokio_sampler_config
            .expect("tokio config should be recorded");
        let defaults = super::TokioSamplerModeDefaults::for_mode(CaptureMode::Light);
        assert_eq!(config.inherited_mode, CaptureMode::Light);
        assert_eq!(config.explicit_mode_override, None);
        assert_eq!(config.resolved_mode, CaptureMode::Light);
        let cadence_ms = u64::try_from(defaults.cadence.as_millis()).expect("cadence fits in u64");
        assert_eq!(config.resolved_sampler_cadence_ms, cadence_ms);
        assert_eq!(
            config.resolved_runtime_snapshot_retention,
            defaults.max_runtime_snapshots
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn core_investigation_inherits_tokio_investigation_defaults() {
        let tailtriage = Arc::new(
            Tailtriage::builder("runtime-test")
                .output(std::env::temp_dir().join("tailtriage_tokio_inherit_investigation.json"))
                .investigation()
                .build()
                .expect("build should succeed"),
        );

        let sampler = RuntimeSampler::builder(Arc::clone(&tailtriage))
            .start()
            .expect("sampler should start");
        sampler.shutdown().await;

        let snapshot = tailtriage.snapshot();
        let config = snapshot
            .metadata
            .effective_tokio_sampler_config
            .expect("tokio config should be recorded");
        let defaults = super::TokioSamplerModeDefaults::for_mode(CaptureMode::Investigation);
        assert_eq!(config.inherited_mode, CaptureMode::Investigation);
        assert_eq!(config.explicit_mode_override, None);
        assert_eq!(config.resolved_mode, CaptureMode::Investigation);
        let cadence_ms = u64::try_from(defaults.cadence.as_millis()).expect("cadence fits in u64");
        assert_eq!(config.resolved_sampler_cadence_ms, cadence_ms);
        assert_eq!(
            config.resolved_runtime_snapshot_retention,
            defaults.max_runtime_snapshots
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn explicit_tokio_mode_override_beats_inherited_core_mode() {
        let tailtriage = Arc::new(
            Tailtriage::builder("runtime-test")
                .output(std::env::temp_dir().join("tailtriage_tokio_mode_override.json"))
                .light()
                .build()
                .expect("build should succeed"),
        );

        let sampler = RuntimeSampler::builder(Arc::clone(&tailtriage))
            .mode(CaptureMode::Investigation)
            .start()
            .expect("sampler should start");
        sampler.shutdown().await;

        let snapshot = tailtriage.snapshot();
        let config = snapshot
            .metadata
            .effective_tokio_sampler_config
            .expect("tokio config should be recorded");
        assert_eq!(config.inherited_mode, CaptureMode::Light);
        assert_eq!(
            config.explicit_mode_override,
            Some(CaptureMode::Investigation)
        );
        assert_eq!(config.resolved_mode, CaptureMode::Investigation);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn explicit_cadence_override_beats_mode_default() {
        let tailtriage = Arc::new(
            Tailtriage::builder("runtime-test")
                .output(std::env::temp_dir().join("tailtriage_tokio_interval_override.json"))
                .build()
                .expect("build should succeed"),
        );

        let sampler = RuntimeSampler::builder(Arc::clone(&tailtriage))
            .interval(Duration::from_millis(17))
            .start()
            .expect("sampler should start");
        sampler.shutdown().await;

        let snapshot = tailtriage.snapshot();
        let config = snapshot
            .metadata
            .effective_tokio_sampler_config
            .expect("tokio config should be recorded");
        assert_eq!(config.resolved_sampler_cadence_ms, 17);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn explicit_retention_override_beats_mode_default() {
        let tailtriage = Arc::new(
            Tailtriage::builder("runtime-test")
                .output(std::env::temp_dir().join("tailtriage_tokio_retention_override.json"))
                .build()
                .expect("build should succeed"),
        );

        let sampler = RuntimeSampler::builder(Arc::clone(&tailtriage))
            .interval(Duration::from_millis(1))
            .max_runtime_snapshots(1)
            .start()
            .expect("sampler should start");

        tokio::time::sleep(Duration::from_millis(12)).await;
        sampler.shutdown().await;

        let snapshot = tailtriage.snapshot();
        let config = snapshot
            .metadata
            .effective_tokio_sampler_config
            .expect("tokio config should be recorded");
        assert_eq!(config.resolved_runtime_snapshot_retention, 1);
        assert_eq!(snapshot.runtime_snapshots.len(), 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn sampler_stops_task_after_reaching_resolved_cap() {
        let tailtriage = Arc::new(
            Tailtriage::builder("runtime-test")
                .output(std::env::temp_dir().join("tailtriage_tokio_cap_stops_task.json"))
                .build()
                .expect("build should succeed"),
        );

        let sampler = RuntimeSampler::builder(Arc::clone(&tailtriage))
            .interval(Duration::from_millis(1))
            .max_runtime_snapshots(1)
            .start()
            .expect("sampler should start");

        tokio::time::sleep(Duration::from_millis(12)).await;
        assert!(
            sampler.task.is_finished(),
            "sampler task should exit at cap"
        );

        let before = tailtriage.snapshot().runtime_snapshots.len();
        tokio::time::sleep(Duration::from_millis(12)).await;
        let after = tailtriage.snapshot().runtime_snapshots.len();
        assert_eq!(before, 1);
        assert_eq!(after, 1);

        // shutdown remains safe after the task has already exited at cap.
        sampler.shutdown().await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn tokio_retention_override_is_clamped_by_core_limit() {
        let tailtriage = Arc::new(
            Tailtriage::builder("runtime-test")
                .output(std::env::temp_dir().join("tailtriage_tokio_retention_clamp.json"))
                .capture_limits_override(tailtriage_core::CaptureLimitsOverride {
                    max_runtime_snapshots: Some(3),
                    ..tailtriage_core::CaptureLimitsOverride::default()
                })
                .build()
                .expect("build should succeed"),
        );

        let sampler = RuntimeSampler::builder(Arc::clone(&tailtriage))
            .interval(Duration::from_millis(1))
            .max_runtime_snapshots(50)
            .start()
            .expect("sampler should start");

        tokio::time::sleep(Duration::from_millis(25)).await;
        sampler.shutdown().await;

        let snapshot = tailtriage.snapshot();
        let config = snapshot
            .metadata
            .effective_tokio_sampler_config
            .expect("tokio config should be recorded");

        assert_eq!(config.resolved_runtime_snapshot_retention, 3);
        assert_eq!(snapshot.runtime_snapshots.len(), 3);
        assert_eq!(snapshot.truncation.dropped_runtime_snapshots, 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn sampler_does_not_autostart_from_capture_mode() {
        let tailtriage = Tailtriage::builder("runtime-test")
            .output(std::env::temp_dir().join("tailtriage_tokio_no_autostart.json"))
            .investigation()
            .build()
            .expect("build should succeed");

        tokio::time::sleep(Duration::from_millis(10)).await;
        let snapshot = tailtriage.snapshot();
        assert!(snapshot.runtime_snapshots.is_empty());
        assert!(snapshot.metadata.effective_tokio_sampler_config.is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn unavailable_runtime_metrics_are_recorded_as_none() {
        let snapshot = super::capture_runtime_snapshot(&tokio::runtime::Handle::current());

        #[cfg(not(tokio_unstable))]
        {
            assert_eq!(snapshot.local_queue_depth, None);
            assert_eq!(snapshot.blocking_queue_depth, None);
            assert_eq!(snapshot.remote_schedule_count, None);
        }
    }
}
