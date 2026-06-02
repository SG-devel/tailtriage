#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

// Tokio runtime integration for tailtriage.
//
// This crate provides [`RuntimeSampler`] for periodic Tokio runtime metrics
// snapshots that feed evidence into the same unified `Tailtriage` API surface.
// Use it when you need stronger separation between executor pressure,
// blocking-pool pressure, queueing, and downstream-stage slowdowns.

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use tailtriage_core::{
    __internal, unix_time_ms, CaptureMode, EffectiveTokioSamplerConfig,
    RuntimeSamplerRegistrationError, RuntimeSnapshot, Tailtriage,
};
use tokio::runtime::Handle;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

mod sealed {
    pub trait Sealed {}

    impl Sealed for tailtriage_core::RequestHandle<'_> {}
    impl Sealed for tailtriage_core::OwnedRequestHandle {}
}

/// Extension helpers that map common Tokio primitives to tailtriage queue/stage/in-flight signals.
pub trait TokioRequestHandleExt: sealed::Sealed {
    /// Records a queue event while waiting to acquire a semaphore permit.
    ///
    /// Equivalent low-level form: `req.queue(label).await_on(semaphore.acquire())` via [`InstrumentedSemaphore::acquire`].
    ///
    /// Records only acquisition wait time, not the protected work after the permit is acquired.
    /// Queue events are recorded only when `acquire()` completes; dropping before completion records no queue event.
    /// Returns Tokio's permit/error types unchanged. Request completion remains explicit.
    fn semaphore<'req, 'sem>(
        &'req self,
        queue: impl Into<String>,
        semaphore: &'sem tokio::sync::Semaphore,
    ) -> InstrumentedSemaphore<'req, 'sem>;
    /// Records a queue event while waiting to acquire an owned semaphore permit.
    ///
    /// Equivalent low-level form: `req.queue(label).await_on(semaphore.acquire_owned())` via [`InstrumentedOwnedSemaphore::acquire_owned`].
    ///
    /// Records only acquisition wait time, not work after permit acquisition.
    /// Queue events are recorded only when `acquire_owned()` completes; dropping before completion records no queue event.
    /// Returns Tokio's permit/error types unchanged. Request completion remains explicit.
    fn owned_semaphore(
        &self,
        queue: impl Into<String>,
        semaphore: Arc<tokio::sync::Semaphore>,
    ) -> InstrumentedOwnedSemaphore<'_>;
    /// Records a queue event while waiting for bounded-channel send/backpressure.
    ///
    /// Equivalent low-level form: `req.queue(label).await_on(sender.send(value))`.
    ///
    /// Measures bounded-channel send/backpressure wait, not receiver-side processing.
    /// Queue events are recorded only when send completes; dropping before completion records no queue event.
    /// Preserves `Result<(), SendError<T>>` unchanged. Request completion remains explicit.
    fn mpsc_send<'a, T>(
        &'a self,
        queue: impl Into<String>,
        sender: &'a tokio::sync::mpsc::Sender<T>,
        value: T,
    ) -> impl Future<Output = Result<(), tokio::sync::mpsc::error::SendError<T>>> + 'a;
    /// Records a queue event while waiting to acquire an async mutex.
    ///
    /// Equivalent low-level form: `req.queue(label).await_on(mutex.lock())`.
    ///
    /// Measures lock acquisition only, not work while holding the guard.
    /// Queue events are recorded only when lock acquisition completes; dropping before completion records no queue event.
    /// Request completion remains explicit.
    fn mutex_lock<'req, 'lock, T>(
        &'req self,
        queue: impl Into<String>,
        mutex: &'lock tokio::sync::Mutex<T>,
    ) -> impl Future<Output = tokio::sync::MutexGuard<'lock, T>> + 'req
    where
        'lock: 'req;
    /// Records a queue event while waiting to acquire an async read lock.
    ///
    /// Equivalent low-level form: `req.queue(label).await_on(lock.read())`.
    ///
    /// Measures acquisition only, not work while holding the guard.
    /// Queue events are recorded only when lock acquisition completes; dropping before completion records no queue event.
    /// Request completion remains explicit.
    fn rwlock_read<'req, 'lock, T>(
        &'req self,
        queue: impl Into<String>,
        lock: &'lock tokio::sync::RwLock<T>,
    ) -> impl Future<Output = tokio::sync::RwLockReadGuard<'lock, T>> + 'req
    where
        'lock: 'req;
    /// Records a queue event while waiting to acquire an async write lock.
    ///
    /// Equivalent low-level form: `req.queue(label).await_on(lock.write())`.
    ///
    /// Measures acquisition only, not work while holding the guard.
    /// Queue events are recorded only when lock acquisition completes; dropping before completion records no queue event.
    /// Request completion remains explicit.
    fn rwlock_write<'req, 'lock, T>(
        &'req self,
        queue: impl Into<String>,
        lock: &'lock tokio::sync::RwLock<T>,
    ) -> impl Future<Output = tokio::sync::RwLockWriteGuard<'lock, T>> + 'req
    where
        'lock: 'req;
    /// Records a stage event while awaiting a spawned task's `JoinHandle`.
    ///
    /// Equivalent low-level form: `req.stage(label).await_on(handle)`.
    ///
    /// Records time spent awaiting the supplied join handle. If the task started earlier, this may not represent the full task lifetime.
    /// Stage success/failure is derived from the outer `Result<T, JoinError>`.
    /// If `T` is itself a `Result`, inner `Err` values are preserved and do not mark the recorded stage as failed.
    /// Preserves `Result<T, JoinError>` unchanged, including panic/cancel join errors.
    /// Dropping before completion records no stage event. Request completion remains explicit.
    fn join_task<T>(
        &self,
        stage: impl Into<String>,
        handle: tokio::task::JoinHandle<T>,
    ) -> impl Future<Output = Result<T, tokio::task::JoinError>>;
    /// Records a stage event around `tokio::time::timeout(timeout, future)`.
    ///
    /// Equivalent low-level form: `req.stage(label).await_on(tokio::time::timeout(timeout, future))`.
    ///
    /// Constructing the helper future does not start timeout budget. Timeout starts when the returned future is polled/awaited.
    /// Timeout elapsed is represented by outer `Err(Elapsed)` and records a failed stage.
    /// Preserves the outer timeout `Result` and any nested inner `Result` exactly (no flattening/remapping).
    /// Because stage success/failure is derived from the outer timeout `Result`, `Ok(Err(_))` is preserved and records a successful stage.
    /// Dropping before completion records no stage event. Request completion remains explicit.
    fn timeout_stage<'a, Fut: Future + 'a>(
        &'a self,
        stage: impl Into<String>,
        timeout: Duration,
        future: Fut,
    ) -> impl Future<Output = Result<Fut::Output, tokio::time::error::Elapsed>> + 'a;
    /// Records a stage for blocking-pool work using `tokio::task::spawn_blocking`.
    ///
    /// Equivalent low-level form:
    /// `req.stage(label).await_on(async move { tokio::task::spawn_blocking(f).await })`.
    ///
    /// This helper is lazy: it calls `spawn_blocking` only when the returned future is first polled,
    /// normally by `.await`.
    /// Recorded stage timing starts at first poll and covers `spawn_blocking` submission through `JoinHandle`
    /// completion.
    /// Stage success/failure is derived from the outer `Result<R, tokio::task::JoinError>`.
    /// If `R` is itself a `Result`, inner `Err` values are preserved and do not mark the recorded stage as failed.
    /// Preserves `Result<R, tokio::task::JoinError>` unchanged.
    /// Dropping before completion records no stage event. Request completion remains explicit.
    /// If you need eager/overlapped spawning, call `tokio::task::spawn_blocking` directly and instrument
    /// the returned handle with `join_task(...)` (which records await time for an already-started task).
    fn blocking_stage<F, R>(
        &self,
        stage: impl Into<String>,
        f: F,
    ) -> impl Future<Output = Result<R, tokio::task::JoinError>>
    where
        F: FnOnce() -> R + Send + 'static,
        R: Send + 'static;
    /// Alias for `inflight(...)` for RAII discoverability.
    ///
    /// Equivalent low-level form: `req.inflight(label)`.
    ///
    /// Records in-flight gauge increments/decrements only. It does not record queue or stage timing. Request completion remains explicit.
    fn inflight_guard(&self, gauge: impl Into<String>) -> tailtriage_core::InflightGuard<'_>;
}

impl TokioRequestHandleExt for tailtriage_core::RequestHandle<'_> {
    fn semaphore<'req, 'sem>(
        &'req self,
        queue: impl Into<String>,
        semaphore: &'sem tokio::sync::Semaphore,
    ) -> InstrumentedSemaphore<'req, 'sem> {
        InstrumentedSemaphore {
            timer: self.queue(queue),
            semaphore,
        }
    }
    fn owned_semaphore(
        &self,
        queue: impl Into<String>,
        semaphore: Arc<tokio::sync::Semaphore>,
    ) -> InstrumentedOwnedSemaphore<'_> {
        InstrumentedOwnedSemaphore {
            timer: self.queue(queue),
            semaphore,
        }
    }
    fn mpsc_send<'a, T>(
        &'a self,
        queue: impl Into<String>,
        sender: &'a tokio::sync::mpsc::Sender<T>,
        value: T,
    ) -> impl Future<Output = Result<(), tokio::sync::mpsc::error::SendError<T>>> + 'a {
        self.queue(queue).await_on(sender.send(value))
    }
    fn mutex_lock<'req, 'lock, T>(
        &'req self,
        queue: impl Into<String>,
        mutex: &'lock tokio::sync::Mutex<T>,
    ) -> impl Future<Output = tokio::sync::MutexGuard<'lock, T>> + 'req
    where
        'lock: 'req,
    {
        self.queue(queue).await_on(mutex.lock())
    }
    fn rwlock_read<'req, 'lock, T>(
        &'req self,
        queue: impl Into<String>,
        lock: &'lock tokio::sync::RwLock<T>,
    ) -> impl Future<Output = tokio::sync::RwLockReadGuard<'lock, T>> + 'req
    where
        'lock: 'req,
    {
        self.queue(queue).await_on(lock.read())
    }
    fn rwlock_write<'req, 'lock, T>(
        &'req self,
        queue: impl Into<String>,
        lock: &'lock tokio::sync::RwLock<T>,
    ) -> impl Future<Output = tokio::sync::RwLockWriteGuard<'lock, T>> + 'req
    where
        'lock: 'req,
    {
        self.queue(queue).await_on(lock.write())
    }
    fn join_task<T>(
        &self,
        stage: impl Into<String>,
        handle: tokio::task::JoinHandle<T>,
    ) -> impl Future<Output = Result<T, tokio::task::JoinError>> {
        self.stage(stage).await_on(handle)
    }
    fn timeout_stage<'a, Fut: Future + 'a>(
        &'a self,
        stage: impl Into<String>,
        timeout: Duration,
        future: Fut,
    ) -> impl Future<Output = Result<Fut::Output, tokio::time::error::Elapsed>> + 'a {
        let timer = self.stage(stage);
        async move {
            timer
                .await_on(async move { tokio::time::timeout(timeout, future).await })
                .await
        }
    }
    fn blocking_stage<F, R>(
        &self,
        stage: impl Into<String>,
        f: F,
    ) -> impl Future<Output = Result<R, tokio::task::JoinError>>
    where
        F: FnOnce() -> R + Send + 'static,
        R: Send + 'static,
    {
        let timer = self.stage(stage);
        async move {
            timer
                .await_on(async move { tokio::task::spawn_blocking(f).await })
                .await
        }
    }
    fn inflight_guard(&self, gauge: impl Into<String>) -> tailtriage_core::InflightGuard<'_> {
        self.inflight(gauge)
    }
}

impl TokioRequestHandleExt for tailtriage_core::OwnedRequestHandle {
    fn semaphore<'req, 'sem>(
        &'req self,
        queue: impl Into<String>,
        semaphore: &'sem tokio::sync::Semaphore,
    ) -> InstrumentedSemaphore<'req, 'sem> {
        InstrumentedSemaphore {
            timer: self.queue(queue),
            semaphore,
        }
    }
    fn owned_semaphore(
        &self,
        queue: impl Into<String>,
        semaphore: Arc<tokio::sync::Semaphore>,
    ) -> InstrumentedOwnedSemaphore<'_> {
        InstrumentedOwnedSemaphore {
            timer: self.queue(queue),
            semaphore,
        }
    }
    fn mpsc_send<'a, T>(
        &'a self,
        queue: impl Into<String>,
        sender: &'a tokio::sync::mpsc::Sender<T>,
        value: T,
    ) -> impl Future<Output = Result<(), tokio::sync::mpsc::error::SendError<T>>> + 'a {
        self.queue(queue).await_on(sender.send(value))
    }
    fn mutex_lock<'req, 'lock, T>(
        &'req self,
        queue: impl Into<String>,
        mutex: &'lock tokio::sync::Mutex<T>,
    ) -> impl Future<Output = tokio::sync::MutexGuard<'lock, T>> + 'req
    where
        'lock: 'req,
    {
        self.queue(queue).await_on(mutex.lock())
    }
    fn rwlock_read<'req, 'lock, T>(
        &'req self,
        queue: impl Into<String>,
        lock: &'lock tokio::sync::RwLock<T>,
    ) -> impl Future<Output = tokio::sync::RwLockReadGuard<'lock, T>> + 'req
    where
        'lock: 'req,
    {
        self.queue(queue).await_on(lock.read())
    }
    fn rwlock_write<'req, 'lock, T>(
        &'req self,
        queue: impl Into<String>,
        lock: &'lock tokio::sync::RwLock<T>,
    ) -> impl Future<Output = tokio::sync::RwLockWriteGuard<'lock, T>> + 'req
    where
        'lock: 'req,
    {
        self.queue(queue).await_on(lock.write())
    }
    fn join_task<T>(
        &self,
        stage: impl Into<String>,
        handle: tokio::task::JoinHandle<T>,
    ) -> impl Future<Output = Result<T, tokio::task::JoinError>> {
        self.stage(stage).await_on(handle)
    }
    fn timeout_stage<'a, Fut: Future + 'a>(
        &'a self,
        stage: impl Into<String>,
        timeout: Duration,
        future: Fut,
    ) -> impl Future<Output = Result<Fut::Output, tokio::time::error::Elapsed>> + 'a {
        let timer = self.stage(stage);
        async move {
            timer
                .await_on(async move { tokio::time::timeout(timeout, future).await })
                .await
        }
    }
    fn blocking_stage<F, R>(
        &self,
        stage: impl Into<String>,
        f: F,
    ) -> impl Future<Output = Result<R, tokio::task::JoinError>>
    where
        F: FnOnce() -> R + Send + 'static,
        R: Send + 'static,
    {
        let timer = self.stage(stage);
        async move {
            timer
                .await_on(async move { tokio::task::spawn_blocking(f).await })
                .await
        }
    }
    fn inflight_guard(&self, gauge: impl Into<String>) -> tailtriage_core::InflightGuard<'_> {
        self.inflight(gauge)
    }
}

/// Queue-instrumented semaphore acquisition helper.
#[must_use = "constructing the wrapper records nothing until acquire() is awaited"]
pub struct InstrumentedSemaphore<'req, 'sem> {
    timer: tailtriage_core::QueueTimer<'req>,
    semaphore: &'sem tokio::sync::Semaphore,
}
impl<'sem> InstrumentedSemaphore<'_, 'sem> {
    /// Awaits a borrowed semaphore permit while recording queue wait duration.
    ///
    /// # Errors
    ///
    /// Returns [`tokio::sync::AcquireError`] when the semaphore is closed.
    pub async fn acquire(
        self,
    ) -> Result<tokio::sync::SemaphorePermit<'sem>, tokio::sync::AcquireError> {
        self.timer.await_on(self.semaphore.acquire()).await
    }
}
/// Queue-instrumented owned semaphore acquisition helper.
#[must_use = "constructing the wrapper records nothing until acquire_owned() is awaited"]
pub struct InstrumentedOwnedSemaphore<'a> {
    timer: tailtriage_core::QueueTimer<'a>,
    semaphore: Arc<tokio::sync::Semaphore>,
}
impl InstrumentedOwnedSemaphore<'_> {
    /// Awaits an owned semaphore permit while recording queue wait duration.
    ///
    /// # Errors
    ///
    /// Returns [`tokio::sync::AcquireError`] when the semaphore is closed.
    pub async fn acquire_owned(
        self,
    ) -> Result<tokio::sync::OwnedSemaphorePermit, tokio::sync::AcquireError> {
        self.timer.await_on(self.semaphore.acquire_owned()).await
    }
}

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
    /// Runtime sampling requires an active Tokio runtime.
    MissingRuntime,
    /// Only one runtime sampler may be started for each Tailtriage run.
    DuplicateStart,
}

impl std::fmt::Display for SamplerStartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ZeroInterval => write!(f, "runtime sampling interval must be greater than zero"),
            Self::MissingRuntime => write!(
                f,
                "runtime sampling requires an active Tokio runtime on the current thread"
            ),
            Self::DuplicateStart => {
                write!(
                    f,
                    "only one runtime sampler may be started per Tailtriage run"
                )
            }
        }
    }
}

impl std::error::Error for SamplerStartError {}

/// Periodically samples Tokio runtime metrics and records them into a [`Tailtriage`] run.
///
/// The sampler records an initial runtime snapshot promptly after start, then
/// follows the resolved cadence. The cadence is a target periodic sampling
/// cadence, not a hard real-time guarantee; actual timing depends on Tokio
/// scheduling and runtime conditions.
#[derive(Debug)]
pub struct RuntimeSampler {
    stop_tx: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<()>>,
}

/// Tokio-owned defaults for runtime sampler behavior by capture mode.
///
/// Numeric defaults:
///
/// - Light: `cadence = 500ms`, `max_runtime_snapshots = 5_000`
/// - Investigation: `cadence = 100ms`, `max_runtime_snapshots = 50_000`
///
/// These Tokio defaults are applied only when [`RuntimeSampler`] is started.
/// Selecting core [`CaptureMode`] never auto-starts runtime sampling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokioSamplerModeDefaults {
    /// Default target periodic sampler cadence.
    ///
    /// Sampling records an initial snapshot promptly after start and then follows
    /// this target cadence; it is not a hard real-time timing guarantee.
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
    /// For lower runtime-cost core-only capture categories, skip sampler startup.
    ///
    /// # Errors
    ///
    /// Returns [`SamplerStartError::ZeroInterval`] when `interval` is zero.
    ///
    /// Returns [`SamplerStartError::MissingRuntime`] when called outside an
    /// active Tokio runtime.
    ///
    /// Returns [`SamplerStartError::DuplicateStart`] when runtime sampling was
    /// already started for this run.
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
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
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
    ///
    /// The resolved cadence is a target periodic sampling cadence after the
    /// initial prompt sample, not a hard real-time timing guarantee.
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
    /// Resolution precedence:
    ///
    /// 1. inherited mode from the core-selected mode on [`Tailtriage`]
    /// 2. optional explicit Tokio override via [`Self::mode`]
    /// 3. optional cadence override via [`Self::interval`]
    /// 4. optional runtime snapshot retention override via
    ///    [`Self::max_runtime_snapshots`]
    ///
    /// Resolved runtime snapshot retention is clamped by the core run cap
    /// (`effective_core_config.capture_limits.max_runtime_snapshots`). The
    /// sampler records an initial sample promptly after start, then follows the
    /// resolved target cadence; cadence is not a hard real-time guarantee.
    ///
    /// # Errors
    ///
    /// Returns [`SamplerStartError::ZeroInterval`] when resolved cadence is zero.
    ///
    /// Returns [`SamplerStartError::MissingRuntime`] when called outside an
    /// active Tokio runtime.
    ///
    /// Returns [`SamplerStartError::DuplicateStart`] when a sampler was already
    /// started for this run.
    pub fn start(self) -> Result<RuntimeSampler, SamplerStartError> {
        let resolved = self.resolve_config()?;
        let handle = Handle::try_current().map_err(|_| SamplerStartError::MissingRuntime)?;
        __internal::register_tokio_runtime_sampler(
            &self.tailtriage,
            resolved.into_effective_metadata(),
        )
        .map_err(|err| match err {
            RuntimeSamplerRegistrationError::DuplicateStart => SamplerStartError::DuplicateStart,
        })?;

        let tailtriage = Arc::clone(&self.tailtriage);
        let (stop_tx, mut stop_rx) = oneshot::channel();
        let mut captured: usize = 0;
        let max_runtime_snapshots = resolved.resolved_max_runtime_snapshots;
        let resolved_interval = resolved.resolved_interval;

        let runtime_handle = handle.clone();
        let task = handle.spawn(async move {
            let mut ticker = tokio::time::interval(resolved_interval);
            loop {
                tokio::select! {
                    _ = &mut stop_rx => break,
                    _ = ticker.tick() => {
                        if captured >= max_runtime_snapshots {
                            break;
                        }

                        tailtriage.record_runtime_snapshot(capture_runtime_snapshot(&runtime_handle));
                        captured = captured.saturating_add(1);
                        if captured >= max_runtime_snapshots {
                            break;
                        }
                    }
                }
            }
        });

        Ok(RuntimeSampler {
            stop_tx: Some(stop_tx),
            task: Some(task),
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
        at_run_us: None,
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

    async fn wait_until(
        timeout: Duration,
        mut condition: impl FnMut() -> bool,
        failure_message: &str,
    ) {
        let deadline = tokio::time::Instant::now() + timeout;
        while tokio::time::Instant::now() < deadline {
            if condition() {
                return;
            }
            tokio::task::yield_now().await;
            tokio::time::sleep(Duration::from_millis(1)).await;
        }

        assert!(condition(), "{failure_message}");
    }

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

        wait_until(
            Duration::from_millis(250),
            || !tailtriage.snapshot().runtime_snapshots.is_empty(),
            "sampler should record runtime snapshots",
        )
        .await;
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

    #[test]
    fn runtime_sampler_requires_active_runtime() {
        let tailtriage = Arc::new(
            Tailtriage::builder("runtime-test")
                .output(std::env::temp_dir().join("tailtriage_tokio_missing_runtime.json"))
                .build()
                .expect("build should succeed"),
        );

        let err = RuntimeSampler::builder(Arc::clone(&tailtriage))
            .interval(Duration::from_millis(5))
            .start()
            .expect_err("starting outside runtime should fail");
        assert_eq!(err, SamplerStartError::MissingRuntime);
        assert!(
            tailtriage
                .snapshot()
                .metadata
                .effective_tokio_sampler_config
                .is_none(),
            "failed startup must not mutate sampler metadata"
        );
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

        wait_until(
            Duration::from_millis(250),
            || {
                sampler
                    .task
                    .as_ref()
                    .expect("sampler should spawn task when capture is enabled")
                    .is_finished()
            },
            "sampler task should exit after recording the configured cap",
        )
        .await;
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

        wait_until(
            Duration::from_millis(250),
            || {
                sampler
                    .task
                    .as_ref()
                    .expect("sampler should spawn task when capture is enabled")
                    .is_finished()
            },
            "sampler task should exit at cap",
        )
        .await;

        let before = tailtriage.snapshot().runtime_snapshots.len();
        tokio::time::sleep(Duration::from_millis(12)).await;
        let after = tailtriage.snapshot().runtime_snapshots.len();
        assert_eq!(before, 1);
        assert_eq!(after, 1);

        // shutdown remains safe after the task has already exited at cap.
        sampler.shutdown().await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn runtime_sampler_records_when_started() {
        let tailtriage = Arc::new(
            Tailtriage::builder("runtime-test")
                .output(std::env::temp_dir().join("tailtriage_tokio_disabled_sampler.json"))
                .build()
                .expect("build should succeed"),
        );

        let sampler = RuntimeSampler::builder(Arc::clone(&tailtriage))
            .interval(Duration::from_millis(1))
            .start()
            .expect("sampler should start");
        wait_until(
            Duration::from_millis(250),
            || !tailtriage.snapshot().runtime_snapshots.is_empty(),
            "sampler should record at least one runtime snapshot after startup",
        )
        .await;
        sampler.shutdown().await;

        let snapshot = tailtriage.snapshot();
        assert!(!snapshot.runtime_snapshots.is_empty());
        assert!(
            snapshot.metadata.effective_tokio_sampler_config.is_some(),
            "sampler startup should record effective sampler metadata"
        );
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

        wait_until(
            Duration::from_millis(250),
            || tailtriage.snapshot().runtime_snapshots.len() == 3,
            "sampler should stop after recording the clamped runtime snapshot limit",
        )
        .await;
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

    #[tokio::test(flavor = "current_thread")]
    async fn runtime_sampler_rejects_duplicate_start_for_same_run() {
        let tailtriage = Arc::new(
            Tailtriage::builder("runtime-test")
                .output(std::env::temp_dir().join("tailtriage_tokio_duplicate_start.json"))
                .build()
                .expect("build should succeed"),
        );

        let sampler = RuntimeSampler::builder(Arc::clone(&tailtriage))
            .interval(Duration::from_millis(11))
            .start()
            .expect("first sampler should start");

        let err = RuntimeSampler::builder(Arc::clone(&tailtriage))
            .interval(Duration::from_millis(17))
            .start()
            .expect_err("duplicate sampler start should fail");
        assert_eq!(err, SamplerStartError::DuplicateStart);

        sampler.shutdown().await;

        let metadata = tailtriage
            .snapshot()
            .metadata
            .effective_tokio_sampler_config
            .expect("first sampler startup should record metadata");
        assert_eq!(
            metadata.resolved_sampler_cadence_ms, 11,
            "duplicate start must not overwrite prior metadata"
        );
    }
}

#[cfg(test)]
mod helper_tests {
    use std::rc::Rc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    use tailtriage_core::Tailtriage;

    use crate::TokioRequestHandleExt;

    fn run() -> Tailtriage {
        Tailtriage::builder("tokio-helpers")
            .output(std::env::temp_dir().join("tailtriage_tokio_helpers.json"))
            .build()
            .expect("build")
    }

    async fn acquire_semaphore_with_primitive_lifetime<'s>(
        req: &tailtriage_core::RequestHandle<'_>,
        semaphore: &'s tokio::sync::Semaphore,
    ) -> tokio::sync::SemaphorePermit<'s> {
        req.semaphore("sem_lifetime", semaphore)
            .acquire()
            .await
            .expect("permit")
    }

    async fn acquire_mutex_with_primitive_lifetime<'m>(
        req: &tailtriage_core::RequestHandle<'_>,
        mutex: &'m tokio::sync::Mutex<u64>,
    ) -> tokio::sync::MutexGuard<'m, u64> {
        req.mutex_lock("mutex_lifetime", mutex).await
    }

    async fn acquire_rwlock_read_with_primitive_lifetime<'r>(
        req: &tailtriage_core::RequestHandle<'_>,
        lock: &'r tokio::sync::RwLock<u64>,
    ) -> tokio::sync::RwLockReadGuard<'r, u64> {
        req.rwlock_read("rw_read_lifetime", lock).await
    }

    async fn acquire_rwlock_write_with_primitive_lifetime<'r>(
        req: &tailtriage_core::RequestHandle<'_>,
        lock: &'r tokio::sync::RwLock<u64>,
    ) -> tokio::sync::RwLockWriteGuard<'r, u64> {
        req.rwlock_write("rw_write_lifetime", lock).await
    }

    #[tokio::test(flavor = "current_thread")]
    async fn queue_and_lock_helpers_record_queue_only() {
        let run = run();
        let started = run.begin_request("/helpers");
        let req = started.handle.clone();
        let sem = Arc::new(tokio::sync::Semaphore::new(1));
        let permit = req.semaphore("sem", &sem).acquire().await.expect("permit");
        drop(permit);
        let owned = req
            .owned_semaphore("owned_sem", Arc::clone(&sem))
            .acquire_owned()
            .await
            .expect("owned permit");
        drop(owned);

        let closed = tokio::sync::Semaphore::new(0);
        closed.close();
        assert!(req.semaphore("closed", &closed).acquire().await.is_err());

        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        tx.send(7u8).await.expect("seed send");
        assert_eq!(rx.recv().await, Some(7));
        drop(tx);
        assert_eq!(rx.recv().await, None);

        let (tx2, mut rx2) = tokio::sync::mpsc::channel(1);
        tx2.send(1u8).await.expect("fill");
        let send_future = req.mpsc_send("send_wait", &tx2, 2u8);
        tokio::pin!(send_future);
        tokio::select! {
            () = tokio::time::sleep(Duration::from_millis(5)) => {}
            _ = &mut send_future => panic!("send should wait while channel is full"),
        }
        assert_eq!(rx2.recv().await, Some(1));
        assert_eq!(send_future.await, Ok(()));
        drop(rx2);
        let send_closed = req.mpsc_send("send_closed", &tx2, 9u8).await;
        assert_eq!(
            send_closed
                .expect_err("closed receiver should return SendError")
                .0,
            9u8
        );

        let mutex = Arc::new(tokio::sync::Mutex::new(5usize));
        let _g = req.mutex_lock("mutex", &mutex).await;
        let rw = Arc::new(tokio::sync::RwLock::new(3usize));
        {
            let _r = req.rwlock_read("rw_read", &rw).await;
        }
        {
            let _w = req.rwlock_write("rw_write", &rw).await;
        }

        started.completion.finish_ok();
        let snap = run.snapshot();
        assert_eq!(snap.requests.len(), 1);
        assert_eq!(snap.queues.len(), 8);
        assert!(snap.queues.iter().any(|q| q.queue == "sem"));
        assert!(snap.queues.iter().any(|q| q.queue == "owned_sem"));
        assert!(snap.queues.iter().any(|q| q.queue == "closed"));
        assert!(snap.queues.iter().any(|q| q.queue == "send_wait"));
        assert!(snap.queues.iter().any(|q| q.queue == "send_closed"));
        assert!(snap.queues.iter().any(|q| q.queue == "mutex"));
        assert!(snap.queues.iter().any(|q| q.queue == "rw_read"));
        assert!(snap.queues.iter().any(|q| q.queue == "rw_write"));
        assert!(snap.stages.is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn stage_helpers_and_inflight_behave_and_preserve_results() {
        let run = run();
        let started = run.begin_request("/stages");
        let req = started.handle.clone();

        assert_eq!(
            req.join_task("join_ok", tokio::spawn(async { 42usize }))
                .await
                .expect("join ok"),
            42
        );
        assert!(req
            .join_task("join_panic", tokio::spawn(async { panic!("boom") }))
            .await
            .is_err());
        assert_eq!(
            req.timeout_stage("timeout_ok", Duration::from_millis(50), async { 11usize })
                .await,
            Ok(11)
        );
        let nested: Result<Result<(), &'static str>, tokio::time::error::Elapsed> = req
            .timeout_stage("timeout_nested", Duration::from_millis(50), async {
                Err("inner")
            })
            .await;
        assert_eq!(nested, Ok(Err("inner")));
        assert!(req
            .timeout_stage("timeout_elapsed", Duration::from_millis(5), async {
                tokio::time::sleep(Duration::from_millis(30)).await;
                1usize
            })
            .await
            .is_err());
        assert_eq!(
            req.blocking_stage("blocking_ok", || 99usize)
                .await
                .expect("ok"),
            99
        );
        assert!(req
            .blocking_stage("blocking_panic", || -> usize { panic!("x") })
            .await
            .is_err());

        {
            let _g = req.inflight_guard("busy");
            assert_eq!(run.snapshot().inflight.len(), 1);
        }
        let inflight_snap = run.snapshot();
        let busy: Vec<_> = inflight_snap
            .inflight
            .iter()
            .filter(|g| g.gauge == "busy")
            .collect();
        assert_eq!(busy.len(), 2);
        assert_eq!(busy[0].count, 1);
        assert_eq!(busy[1].count, 0);

        started.completion.finish_ok();
        let snap = run.snapshot();
        assert_eq!(snap.requests.len(), 1);
        assert_eq!(snap.stages.len(), 7);
        let stage = |name: &str| snap.stages.iter().find(|s| s.stage == name).unwrap();
        assert!(stage("join_ok").success);
        assert!(!stage("join_panic").success);
        assert!(stage("timeout_ok").success);
        assert!(!stage("timeout_elapsed").success);
        assert!(stage("timeout_nested").success);
        assert!(stage("blocking_ok").success);
        assert!(!stage("blocking_panic").success);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn blocking_stage_is_lazy_until_polled_and_records_on_await() {
        let run = run();
        let started = run.begin_request("/blocking-lazy");
        let req = started.handle.clone();
        let counter = Arc::new(AtomicUsize::new(0));

        let future = req.blocking_stage("blocking_late", {
            let counter = Arc::clone(&counter);
            move || {
                counter.fetch_add(1, Ordering::SeqCst);
                7usize
            }
        });
        drop(future);
        tokio::task::yield_now().await;
        tokio::time::sleep(Duration::from_millis(5)).await;
        assert_eq!(counter.load(Ordering::SeqCst), 0);
        assert!(run
            .snapshot()
            .stages
            .iter()
            .all(|stage| stage.stage != "blocking_late"));

        assert_eq!(
            req.blocking_stage("blocking_late", {
                let counter = Arc::clone(&counter);
                move || {
                    counter.fetch_add(1, Ordering::SeqCst);
                    9usize
                }
            })
            .await
            .expect("join should succeed"),
            9
        );
        assert_eq!(counter.load(Ordering::SeqCst), 1);
        let matching: Vec<_> = run
            .snapshot()
            .stages
            .into_iter()
            .filter(|stage| stage.stage == "blocking_late")
            .collect();
        assert_eq!(matching.len(), 1);

        started.completion.finish_ok();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn timeout_stage_is_lazy_until_polled() {
        let run = run();
        let started = run.begin_request("/timeout-lazy");
        let req = started.handle.clone();

        let helper =
            req.timeout_stage("timeout_lazy", Duration::from_millis(50), async { 55usize });
        assert!(run
            .snapshot()
            .stages
            .iter()
            .all(|stage| stage.stage != "timeout_lazy"));
        tokio::time::sleep(Duration::from_millis(80)).await;
        assert_eq!(helper.await, Ok(55));

        let matching: Vec<_> = run
            .snapshot()
            .stages
            .into_iter()
            .filter(|stage| stage.stage == "timeout_lazy")
            .collect();
        assert_eq!(matching.len(), 1);
        assert!(matching[0].success);

        started.completion.finish_ok();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn timeout_stage_accepts_non_send_futures() {
        let run = run();
        let started = run.begin_request("/timeout-nonsend");
        let req = started.handle.clone();
        let value = Rc::new(8usize);
        let value_for_future = Rc::clone(&value);
        let out = req
            .timeout_stage("timeout_rc", Duration::from_millis(20), async move {
                *value_for_future
            })
            .await;
        assert_eq!(out, Ok(8));
        assert_eq!(*value, 8);
        started.completion.finish_ok();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn dropping_pending_queue_and_stage_helpers_records_no_events() {
        let run = run();
        let started = run.begin_request("/drop-pending");
        let req = started.handle.clone();

        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        tx.send(1u8).await.expect("fill channel");
        {
            let send_future = req.mpsc_send("drop_send_wait", &tx, 2u8);
            tokio::pin!(send_future);
            tokio::select! {
                () = tokio::time::sleep(Duration::from_millis(1)) => {}
                _ = &mut send_future => panic!("send should still be pending"),
            }
        }
        assert_eq!(rx.recv().await, Some(1));

        {
            let stage_future = req.timeout_stage(
                "drop_stage_wait",
                Duration::from_secs(30),
                std::future::pending::<usize>(),
            );
            tokio::pin!(stage_future);
            tokio::select! {
                () = tokio::time::sleep(Duration::from_millis(1)) => {}
                _ = &mut stage_future => panic!("stage should still be pending"),
            }
        }

        let snap = run.snapshot();
        assert!(snap.queues.iter().all(|q| q.queue != "drop_send_wait"));
        assert!(snap.stages.iter().all(|s| s.stage != "drop_stage_wait"));

        started.completion.finish_ok();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn owned_request_handle_works_and_helpers_do_not_finish_request() {
        let run = Arc::new(run());
        let started = run.begin_request_owned("/owned");
        let owned = started.handle.clone();
        let sem = Arc::new(tokio::sync::Semaphore::new(1));
        let permit = owned
            .owned_semaphore("owned_sem", Arc::clone(&sem))
            .acquire_owned()
            .await
            .expect("owned permit");
        drop(permit);
        let _ = owned
            .timeout_stage("owned_timeout", Duration::from_millis(10), async { 1usize })
            .await
            .expect("ok");
        assert!(run.snapshot().requests.is_empty());
        started.completion.finish_ok();
        assert_eq!(run.snapshot().requests.len(), 1);
    }

    #[test]
    fn extension_impls_exist_for_borrowed_and_owned_handles() {
        fn assert_impl<T: crate::TokioRequestHandleExt>() {}
        assert_impl::<tailtriage_core::RequestHandle<'_>>();
        assert_impl::<tailtriage_core::OwnedRequestHandle>();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn semaphore_helper_preserves_semaphore_lifetime() {
        let run = run();
        let sem = tokio::sync::Semaphore::new(1);

        let permit = {
            let started = run.begin_request("/lifetime-sem");
            let permit = acquire_semaphore_with_primitive_lifetime(&started.handle, &sem).await;
            started.completion.finish_ok();
            permit
        };

        drop(permit);
        assert_eq!(run.snapshot().requests.len(), 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn lock_helpers_preserve_lock_lifetimes() {
        let run = run();
        let mutex = tokio::sync::Mutex::new(41_u64);
        let rw = tokio::sync::RwLock::new(7_u64);

        let mut guard = {
            let started = run.begin_request("/lifetime-mutex");
            let guard = acquire_mutex_with_primitive_lifetime(&started.handle, &mutex).await;
            started.completion.finish_ok();
            guard
        };
        *guard += 1;
        drop(guard);

        let read_guard = {
            let started = run.begin_request("/lifetime-rw-read");
            let guard = acquire_rwlock_read_with_primitive_lifetime(&started.handle, &rw).await;
            started.completion.finish_ok();
            guard
        };
        assert_eq!(*read_guard, 7);
        drop(read_guard);

        let mut write_guard = {
            let started = run.begin_request("/lifetime-rw-write");
            let guard = acquire_rwlock_write_with_primitive_lifetime(&started.handle, &rw).await;
            started.completion.finish_ok();
            guard
        };
        *write_guard += 2;
        drop(write_guard);

        assert_eq!(*mutex.lock().await, 42);
        assert_eq!(*rw.read().await, 9);
    }
}
