//! Tokio runtime integration for tailtriage.
//!
//! This crate provides:
//! - [`RuntimeSampler`] for periodic Tokio runtime metrics snapshots.
//! - [`instrument_request`] for request entry-point tracing and optional
//!   request-event recording into a [`tailtriage_core::Tailtriage`] collector.
//!
//! `instrument_request` is optional convenience. You can either:
//! - annotate handlers with the macro for request-level timing and tracing, or
//! - call [`tailtriage_core::Tailtriage::request`] directly for explicit control.
//!
//! Macro example (compile-checked):
//! ```no_run
//! use tailtriage_core::Tailtriage;
//! use tailtriage_tokio::instrument_request;
//!
//! #[instrument_request(
//!     route = "/invoice",
//!     kind = "create_invoice",
//!     tailtriage = tailtriage,
//!     request_id = request_id.clone(),
//!     skip(tailtriage)
//! )]
//! async fn handle_invoice(
//!     tailtriage: &Tailtriage,
//!     request_id: String,
//! ) -> Result<(), &'static str> {
//!     Ok(())
//! }
//!
//! # #[tokio::main(flavor = "current_thread")]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! # let tailtriage = Tailtriage::builder("billing").build()?;
//! handle_invoice(&tailtriage, "req-123".to_string()).await?;
//! # Ok(())
//! # }
//! ```
//!
//! Runtime sampling is worth enabling when you need extra evidence to separate
//! executor or blocking-pool pressure from application-level queue/stage waits.
//! Keep it disabled for the lowest-overhead light runs.

use std::sync::Arc;
use std::time::Duration;

use tailtriage_core::{unix_time_ms, RuntimeSnapshot, Tailtriage};
use tokio::runtime::Handle;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

pub use tailtriage_macros::instrument_request;

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

impl RuntimeSampler {
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
        if interval.is_zero() {
            return Err(SamplerStartError::ZeroInterval);
        }

        let handle = Handle::current();
        let (stop_tx, mut stop_rx) = oneshot::channel();
        let mut ticker = tokio::time::interval(interval);

        let task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = &mut stop_rx => break,
                    _ = ticker.tick() => {
                        tailtriage.record_runtime_snapshot(capture_runtime_snapshot(&handle));
                    }
                }
            }
        });

        Ok(Self {
            stop_tx: Some(stop_tx),
            task,
        })
    }

    /// Starts runtime sampling from the `Tailtriage` sampling configuration.
    ///
    /// # Errors
    ///
    /// Returns [`SamplerStartError::ZeroInterval`] if a zero interval is configured.
    pub fn start_configured(
        tailtriage: Arc<Tailtriage>,
    ) -> Result<Option<Self>, SamplerStartError> {
        match tailtriage.sampling().runtime_interval() {
            Some(interval) => Self::start(tailtriage, interval).map(Some),
            None => Ok(None),
        }
    }

    /// Requests sampler shutdown and waits for task completion.
    pub async fn shutdown(mut self) {
        if let Some(stop_tx) = self.stop_tx.take() {
            let _ = stop_tx.send(());
        }
        let _ = self.task.await;
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

    use tailtriage_core::{SamplingConfig, Tailtriage};

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
        let sampler = RuntimeSampler::start(Arc::clone(&tailtriage), Duration::from_millis(5))
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
    async fn start_configured_returns_none_when_disabled() {
        let tailtriage = Arc::new(Tailtriage::builder("runtime-test").build().expect("build"));
        let sampler = RuntimeSampler::start_configured(tailtriage).expect("configured start");
        assert!(sampler.is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn start_configured_starts_when_sampling_enabled() {
        let tailtriage = Arc::new(
            Tailtriage::builder("runtime-test")
                .sampling(SamplingConfig::runtime(Duration::from_millis(5)))
                .build()
                .expect("build"),
        );
        let sampler = RuntimeSampler::start_configured(Arc::clone(&tailtriage))
            .expect("configured start")
            .expect("sampler should start");
        tokio::time::sleep(Duration::from_millis(20)).await;
        sampler.shutdown().await;
        assert!(!tailtriage.snapshot().runtime_snapshots.is_empty());
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
