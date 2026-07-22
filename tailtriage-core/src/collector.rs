use std::collections::HashMap;
use std::ffi::OsString;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use crate::config::Config;
use crate::time::{FinishedInterval, IntervalStart, RunClock};
use crate::InflightGuard;
use crate::RunSink;
use crate::{
    unix_time_ms, BuildError, InFlightSnapshot, Outcome, QueueEvent, QueueTimer, RequestEvent,
    RequestOptions, Run, RunEndReason, RunMetadata, RuntimeSnapshot, SinkError, StageEvent,
    StageTimer, UnfinishedRequestSample,
};

/// Per-run collector that records request events and writes the final artifact.
pub struct Tailtriage {
    pub(crate) state: CollectorStateCell,
    pub(crate) sink: Arc<dyn RunSink + Send + Sync>,
    pub(crate) mode: crate::CaptureMode,
    pub(crate) effective_core_config: crate::EffectiveCoreConfig,
    pub(crate) limits: crate::CaptureLimits,
    pub(crate) run_clock: RunClock,
    pub(crate) strict_lifecycle: bool,
    pub(crate) truncation_state: TruncationState,
    limits_hit_listener: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
}

#[derive(Debug)]
pub(crate) struct CollectorStateCell {
    pub(crate) mutex: Mutex<CollectorState>,
    closed: Condvar,
}

impl CollectorStateCell {
    fn new(run: Run) -> Self {
        Self {
            mutex: Mutex::new(CollectorState {
                run,
                pending_requests: HashMap::new(),
                inflight_counts: HashMap::new(),
                runtime_sampler_registered: false,
                phase: CollectorPhase::Open,
            }),
            closed: Condvar::new(),
        }
    }
}

#[derive(Debug)]
pub(crate) struct CollectorState {
    pub(crate) run: Run,
    pending_requests: HashMap<u64, PendingRequest>,
    pub(crate) inflight_counts: HashMap<String, u64>,
    runtime_sampler_registered: bool,
    pub(crate) phase: CollectorPhase,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CollectorPhase {
    Open,
    Finalizing,
    Closed(TerminalShutdown),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TerminalShutdown {
    Success,
    Failure(String),
}

#[derive(Debug)]
struct ArmedCompletion {
    pending_key: u64,
    interval_start: IntervalStart,
}

type CompletionState = Option<ArmedCompletion>;

#[derive(Debug, Default)]
pub(crate) struct SectionSaturationState {
    saturated: AtomicBool,
}

impl SectionSaturationState {
    pub(crate) fn mark_saturated(&self) {
        self.saturated.store(true, Ordering::Relaxed);
    }
}

#[derive(Debug, Default)]
pub(crate) struct TruncationState {
    limits_hit: AtomicBool,
    requests: SectionSaturationState,
    stages: SectionSaturationState,
    queues: SectionSaturationState,
    pub(crate) inflight: SectionSaturationState,
    runtime_snapshots: SectionSaturationState,
}

impl TruncationState {
    fn mark_limits_hit(&self) -> bool {
        self.limits_hit
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    pub(crate) fn mark_run_limits_hit(&self, run: &mut Run) -> bool {
        run.truncation.limits_hit = true;
        self.mark_limits_hit()
    }
}

#[derive(Debug, Clone)]
struct PendingRequest {
    request_id: String,
    route: String,
    kind: Option<String>,
    interval_start: IntervalStart,
}

impl std::fmt::Debug for Tailtriage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tailtriage")
            .field("mode", &self.mode)
            .field("limits", &self.limits)
            .field("strict_lifecycle", &self.strict_lifecycle)
            .finish_non_exhaustive()
    }
}

/// Split request lifecycle start result.
///
/// Use `handle` to record queue/stage/inflight evidence, then finish exactly
/// once with `completion`.
///
/// Instrumentation alone does not complete the request lifecycle.
#[must_use = "request completion must be finished explicitly"]
#[derive(Debug)]
pub struct StartedRequest<'a> {
    /// Instrumentation handle for queue/stage/inflight timing.
    pub handle: RequestHandle<'a>,
    /// Single-owner completion token for explicit request finish.
    pub completion: RequestCompletion<'a>,
}

/// Split request lifecycle start result backed by `Arc<Tailtriage>`.
///
/// Use `handle` for instrumentation and `completion` to finish exactly once.
#[must_use = "request completion must be finished explicitly"]
#[derive(Debug)]
pub struct OwnedStartedRequest {
    /// Instrumentation handle for queue/stage/inflight timing.
    pub handle: OwnedRequestHandle,
    /// Single-owner completion token for explicit request finish.
    pub completion: OwnedRequestCompletion,
}

/// Instrumentation-facing request handle.
///
/// This handle records queue/stage/inflight signals for one admitted request.
/// It does not complete the request.
///
/// # Example
///
/// ```no_run
/// use tailtriage_core::Tailtriage;
///
/// # async fn demo() -> Result<(), Box<dyn std::error::Error>> {
/// let run = Tailtriage::builder("checkout-service").build()?;
/// let started = run.begin_request("/checkout");
///
/// started.handle.queue("checkout_queue").await_on(async {}).await;
/// let _: Result<(), std::io::Error> = started
///     .handle
///     .stage("db")
///     .await_on(async { Ok(()) })
///     .await;
///
/// started.completion.finish_ok();
/// run.shutdown()?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct RequestHandle<'a> {
    tailtriage: &'a Tailtriage,
    request_id: String,
    route: String,
    kind: Option<String>,
    admitted: bool,
}

/// Instrumentation-facing request handle backed by `Arc<Tailtriage>`.
///
/// This is the owned variant of [`RequestHandle`], useful across spawned tasks
/// and helper layers.
#[derive(Debug, Clone)]
pub struct OwnedRequestHandle {
    tailtriage: Arc<Tailtriage>,
    request_id: String,
    route: String,
    kind: Option<String>,
    admitted: bool,
}

/// Completion-facing request token.
///
/// Each admitted request must be finished exactly once with
/// [`RequestCompletion::finish`], [`RequestCompletion::finish_ok`], or
/// [`RequestCompletion::finish_result`].
///
/// Dropping an admitted token while capture is open records one `cancelled`
/// request. After shutdown finalization wins, Drop and explicit finish are inert.
#[must_use = "request completion tokens must be finished explicitly"]
#[derive(Debug)]
pub struct RequestCompletion<'a> {
    tailtriage: &'a Tailtriage,
    state: CompletionState,
}

/// Completion-facing request token backed by `Arc<Tailtriage>`.
///
/// Owned variant of [`RequestCompletion`]. Dropping an admitted token while
/// capture is open records one `cancelled` request. After shutdown finalization
/// wins, Drop and explicit finish are inert.
#[must_use = "request completion tokens must be finished explicitly"]
#[derive(Debug)]
pub struct OwnedRequestCompletion {
    tailtriage: Arc<Tailtriage>,
    state: CompletionState,
}

/// Error returned when registering Tokio runtime sampler metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeSamplerRegistrationError {
    /// A runtime sampler was already registered for this run.
    DuplicateStart,
}

impl Tailtriage {
    /// Creates a builder-based setup path for one service run.
    #[must_use]
    pub fn builder(service_name: impl Into<String>) -> crate::TailtriageBuilder {
        crate::TailtriageBuilder::new(service_name)
    }

    pub(crate) fn from_config(config: Config) -> Result<Self, BuildError> {
        if config.service_name.trim().is_empty() {
            return Err(BuildError::EmptyServiceName);
        }

        let run_clock = RunClock::new();
        let now = run_clock.run_started_at_unix_ms();
        let run = Run::new(RunMetadata {
            run_id: config.run_id.unwrap_or_else(generate_run_id),
            service_name: config.service_name,
            service_version: config.service_version,
            started_at_unix_ms: now,
            finalized_at_unix_ms: None,
            mode: config.mode,
            effective_core_config: Some(config.effective_core),
            effective_tokio_sampler_config: None,
            host: lookup_host_name(),
            pid: Some(std::process::id()),
            lifecycle_warnings: Vec::new(),
            unfinished_requests: crate::UnfinishedRequests::default(),
            run_end_reason: None,
        });

        Ok(Self {
            state: CollectorStateCell::new(run),
            sink: config.sink,
            mode: config.mode,
            effective_core_config: config.effective_core,
            limits: config.effective_core.capture_limits,
            run_clock,
            strict_lifecycle: config.strict_lifecycle,
            truncation_state: TruncationState::default(),
            limits_hit_listener: Mutex::new(None),
        })
    }

    /// Returns the selected capture mode for this run.
    #[must_use]
    pub const fn selected_mode(&self) -> crate::CaptureMode {
        self.mode
    }

    /// Returns the effective resolved core configuration for this run.
    #[must_use]
    pub const fn effective_core_config(&self) -> crate::EffectiveCoreConfig {
        self.effective_core_config
    }

    /// Starts a request with autogenerated correlation for `route`.
    ///
    /// Finish the returned completion token exactly once.
    pub fn begin_request(&self, route: impl Into<String>) -> StartedRequest<'_> {
        self.begin_request_with(route, RequestOptions::new())
    }

    /// Starts a request with optional caller-provided request options.
    ///
    /// Queue/stage/inflight instrumentation from the returned handle records
    /// evidence only; it does not finish the request. Finish must happen
    /// exactly once through the returned completion token.
    pub fn begin_request_with(
        &self,
        route: impl Into<String>,
        options: RequestOptions,
    ) -> StartedRequest<'_> {
        let (request_id, route, kind, pending_key, interval_start) =
            self.start_request(route.into(), options);

        StartedRequest {
            handle: RequestHandle {
                tailtriage: self,
                request_id: request_id.clone(),
                route,
                kind,
                admitted: pending_key.is_some(),
            },
            completion: RequestCompletion {
                tailtriage: self,
                state: pending_key.map(|pending_key| ArmedCompletion {
                    pending_key,
                    interval_start,
                }),
            },
        }
    }

    /// Starts a request with autogenerated correlation for `route` using `Arc<Tailtriage>`.
    ///
    /// This is the owned variant for fractured code paths that need to move
    /// handles across task boundaries.
    pub fn begin_request_owned(self: &Arc<Self>, route: impl Into<String>) -> OwnedStartedRequest {
        self.begin_request_with_owned(route, RequestOptions::new())
    }

    /// Starts a request with caller-provided options using `Arc<Tailtriage>`.
    ///
    /// Finish the returned completion token exactly once.
    pub fn begin_request_with_owned(
        self: &Arc<Self>,
        route: impl Into<String>,
        options: RequestOptions,
    ) -> OwnedStartedRequest {
        let (request_id, route, kind, pending_key, interval_start) =
            self.start_request(route.into(), options);

        OwnedStartedRequest {
            handle: OwnedRequestHandle {
                tailtriage: Arc::clone(self),
                request_id: request_id.clone(),
                route,
                kind,
                admitted: pending_key.is_some(),
            },
            completion: OwnedRequestCompletion {
                tailtriage: Arc::clone(self),
                state: pending_key.map(|pending_key| ArmedCompletion {
                    pending_key,
                    interval_start,
                }),
            },
        }
    }

    /// Returns a clone of the current in-memory run state.
    ///
    /// This does not persist anything. Use [`Tailtriage::shutdown`] to write
    /// the final artifact through the configured sink.
    ///
    /// `snapshot()` is useful for diagnostics and tests while capture is still
    /// running. While capture is active, `metadata.finalized_at_unix_ms` remains
    /// `None`; active snapshots have no run-level end timestamp.
    #[must_use]
    pub fn snapshot(&self) -> Run {
        let state = lock_state(&self.state.mutex);
        state.run.clone()
    }

    /// Writes the current run artifact and finishes the run lifecycle.
    ///
    /// With default/non-strict lifecycle, unfinished requests are recorded in
    /// metadata warnings and unfinished-request samples, then the artifact is written.
    ///
    /// With `strict_lifecycle(true)`, unfinished requests cause an early
    /// retryable [`SinkError::Lifecycle`] return and the artifact is not written
    /// or attempted. Once an eligible shutdown reaches the sink, finalization is
    /// terminal and repeated calls replay that terminal result without a second
    /// sink write.
    ///
    /// # Errors
    ///
    /// Returns [`SinkError`] if lifecycle validation fails in strict mode, or if
    /// serialization or writing fails.
    pub fn shutdown(&self) -> Result<(), SinkError> {
        let candidate = {
            let mut state = lock_state(&self.state.mutex);
            loop {
                match &state.phase {
                    CollectorPhase::Open => break,
                    CollectorPhase::Finalizing => {
                        state = wait_state_closed(&self.state, state);
                    }
                    CollectorPhase::Closed(TerminalShutdown::Success) => return Ok(()),
                    CollectorPhase::Closed(TerminalShutdown::Failure(message)) => {
                        return Err(prior_sink_failure(message.as_str()));
                    }
                }
            }

            let pending_count = state.pending_requests.len();
            if pending_count > 0 && self.strict_lifecycle {
                return Err(SinkError::Lifecycle {
                    unfinished_count: pending_count,
                });
            }

            let mut pending_samples = state.pending_requests.iter().collect::<Vec<_>>();
            pending_samples.sort_by_key(|(pending_key, _)| **pending_key);
            let pending_samples = pending_samples
                .into_iter()
                .take(5)
                .map(|(_, req)| UnfinishedRequestSample {
                    request_id: req.request_id.clone(),
                    route: req.route.clone(),
                })
                .collect::<Vec<_>>();
            state.pending_requests.clear();
            let finalized_at = unix_time_ms();
            state.run.metadata.finalized_at_unix_ms = Some(finalized_at);
            if pending_count > 0 {
                state.run.metadata.lifecycle_warnings.push(format!(
                    "{pending_count} unfinished request(s) remained at shutdown; run includes no fabricated completions"
                ));
                state.run.metadata.unfinished_requests.count = pending_count as u64;
                state.run.metadata.unfinished_requests.sample = pending_samples;
            }
            let candidate = state.run.clone();
            state.phase = CollectorPhase::Finalizing;
            candidate
        };

        let normalized = normalize_for_lifecycle(&candidate);
        let result = self.sink.write(&normalized);
        let terminal = match &result {
            Ok(()) => TerminalShutdown::Success,
            Err(err) => TerminalShutdown::Failure(err.to_string()),
        };
        let mut state = lock_state(&self.state.mutex);
        state.phase = CollectorPhase::Closed(terminal);
        self.state.closed.notify_all();
        result
    }

    /// Sets the run-end reason if not already set.
    pub fn set_run_end_reason_if_absent(&self, reason: RunEndReason) {
        let mut state = lock_state(&self.state.mutex);
        if matches!(state.phase, CollectorPhase::Open)
            && state.run.metadata.run_end_reason.is_none()
        {
            state.run.metadata.run_end_reason = Some(reason);
        }
    }

    /// Registers or clears a callback fired on the first transition to `limits_hit`.
    ///
    /// The callback is invoked at most once per run, exactly when truncation first
    /// transitions from `false` to `true`.
    ///
    /// # Panics
    ///
    /// Panics if the limits-hit listener mutex is poisoned.
    pub fn set_limits_hit_listener(&self, listener: Option<Arc<dyn Fn() + Send + Sync>>) {
        let mut guard = self
            .limits_hit_listener
            .lock()
            .expect("limits-hit listener lock poisoned");
        *guard = listener;
    }

    /// Creates an in-flight guard for `gauge`.
    #[must_use]
    pub(crate) fn inflight(&self, gauge: impl Into<String>) -> InflightGuard<'_> {
        let gauge = gauge.into();
        let count = {
            let sample = self.run_clock.sample();
            let mut state = lock_state(&self.state.mutex);
            if !matches!(state.phase, CollectorPhase::Open) {
                return InflightGuard {
                    tailtriage: self,
                    gauge,
                    enabled: false,
                };
            }
            let entry = state.inflight_counts.entry(gauge.clone()).or_insert(0);
            *entry += 1;
            let count = *entry;
            let mut notify_limits_hit = false;
            if crate::retention::push_inflight_snapshot_bounded(
                &mut state.run,
                self.limits,
                InFlightSnapshot {
                    gauge: gauge.clone(),
                    at_unix_ms: sample.unix_ms,
                    at_run_us: Some(sample.run_elapsed_us),
                    count,
                },
            ) {
                self.truncation_state.inflight.mark_saturated();
                notify_limits_hit = self.truncation_state.mark_run_limits_hit(&mut state.run);
            }
            notify_limits_hit
        };
        if count {
            self.notify_limits_hit_listener();
        }

        InflightGuard {
            tailtriage: self,
            gauge,
            enabled: true,
        }
    }

    /// Records one runtime metrics sample captured by an integration crate.
    pub fn record_runtime_snapshot(&self, mut snapshot: RuntimeSnapshot) {
        if snapshot.at_run_us.is_none() {
            snapshot.at_run_us = Some(self.run_clock.sample().run_elapsed_us);
        }

        let notify_limits_hit = {
            let mut state = lock_state(&self.state.mutex);
            if !matches!(state.phase, CollectorPhase::Open) {
                return;
            }
            if crate::retention::push_runtime_snapshot_bounded(
                &mut state.run,
                self.limits,
                snapshot,
            ) {
                self.truncation_state.runtime_snapshots.mark_saturated();
                self.truncation_state.mark_run_limits_hit(&mut state.run)
            } else {
                false
            }
        };
        if notify_limits_hit {
            self.notify_limits_hit_listener();
        }
    }

    /// Registers one Tokio sampler startup and records effective sampler metadata.
    ///
    /// This method succeeds at most once per run.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeSamplerRegistrationError::DuplicateStart`] when a sampler
    /// was already registered for this run.
    pub(crate) fn register_tokio_runtime_sampler(
        &self,
        config: crate::EffectiveTokioSamplerConfig,
    ) -> Result<(), RuntimeSamplerRegistrationError> {
        let mut state = lock_state(&self.state.mutex);
        if !matches!(state.phase, CollectorPhase::Open) {
            return Ok(());
        }
        if state.runtime_sampler_registered {
            return Err(RuntimeSamplerRegistrationError::DuplicateStart);
        }
        state.runtime_sampler_registered = true;
        state.run.metadata.effective_tokio_sampler_config = Some(config);
        Ok(())
    }

    pub(crate) fn record_stage_event(&self, event: StageEvent) {
        let notify_limits_hit = {
            let mut state = lock_state(&self.state.mutex);
            if !matches!(state.phase, CollectorPhase::Open) {
                return;
            }
            if crate::retention::push_stage_bounded(&mut state.run, self.limits, event) {
                self.truncation_state.stages.mark_saturated();
                self.truncation_state.mark_run_limits_hit(&mut state.run)
            } else {
                false
            }
        };
        if notify_limits_hit {
            self.notify_limits_hit_listener();
        }
    }

    pub(crate) fn record_queue_event(&self, event: QueueEvent) {
        let notify_limits_hit = {
            let mut state = lock_state(&self.state.mutex);
            if !matches!(state.phase, CollectorPhase::Open) {
                return;
            }
            if crate::retention::push_queue_bounded(&mut state.run, self.limits, event) {
                self.truncation_state.queues.mark_saturated();
                self.truncation_state.mark_run_limits_hit(&mut state.run)
            } else {
                false
            }
        };
        if notify_limits_hit {
            self.notify_limits_hit_listener();
        }
    }

    fn resolve_request(&self, pending_key: u64, interval_start: IntervalStart, outcome: Outcome) {
        let finished = self.run_clock.finish_interval(interval_start);
        let mut notify_limits_hit = false;
        {
            let mut state = lock_state(&self.state.mutex);
            if !matches!(state.phase, CollectorPhase::Open) {
                return;
            }
            let Some(pending) = state.pending_requests.remove(&pending_key) else {
                return;
            };
            if crate::retention::push_request_bounded(
                &mut state.run,
                self.limits,
                request_event_from_finished_interval(pending, finished, outcome),
            ) {
                self.truncation_state.requests.mark_saturated();
                notify_limits_hit = self.truncation_state.mark_run_limits_hit(&mut state.run);
            }
        }
        if notify_limits_hit {
            self.notify_limits_hit_listener();
        }
    }

    pub(crate) fn notify_limits_hit_listener(&self) {
        let listener = self
            .limits_hit_listener
            .lock()
            .expect("limits-hit listener lock poisoned")
            .clone();
        if let Some(listener) = listener {
            listener();
        }
    }

    fn start_request(
        &self,
        route: String,
        options: RequestOptions,
    ) -> (String, String, Option<String>, Option<u64>, IntervalStart) {
        let request_id = options
            .request_id
            .unwrap_or_else(|| generate_request_id(&route));
        let pending_key = PENDING_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let kind = options.kind;
        let interval_start = self.run_clock.start_interval();
        let pending = PendingRequest {
            request_id: request_id.clone(),
            route: route.clone(),
            kind: kind.clone(),
            interval_start,
        };
        let mut state = lock_state(&self.state.mutex);
        let pending_key = if matches!(state.phase, CollectorPhase::Open) {
            state.pending_requests.insert(pending_key, pending);
            Some(pending_key)
        } else {
            None
        };

        (request_id, route, kind, pending_key, interval_start)
    }
}

impl RequestHandle<'_> {
    /// Returns the stable request ID for this request lifecycle.
    #[must_use]
    pub fn request_id(&self) -> &str {
        &self.request_id
    }

    /// Returns the route or operation name associated with this request.
    #[must_use]
    pub fn route(&self) -> &str {
        &self.route
    }

    /// Returns the optional semantic request kind.
    #[must_use]
    pub fn kind(&self) -> Option<&str> {
        self.kind.as_deref()
    }

    /// Starts queue-wait timing instrumentation for `queue`.
    ///
    /// Recording queue events does not finish the request.
    #[must_use]
    pub fn queue(&self, queue: impl Into<String>) -> QueueTimer<'_> {
        QueueTimer {
            tailtriage: self.tailtriage,
            enabled: self.admitted,
            request_id: self.request_id.clone(),
            queue: queue.into(),
            depth_at_start: None,
        }
    }

    /// Starts stage timing instrumentation for `stage`.
    ///
    /// Recording stage events does not finish the request.
    #[must_use]
    pub fn stage(&self, stage: impl Into<String>) -> StageTimer<'_> {
        StageTimer {
            tailtriage: self.tailtriage,
            enabled: self.admitted,
            request_id: self.request_id.clone(),
            stage: stage.into(),
        }
    }

    /// Increments in-flight gauge tracking for `gauge` until the returned guard drops.
    ///
    /// In-flight instrumentation does not finish the request lifecycle.
    #[must_use]
    pub fn inflight(&self, gauge: impl Into<String>) -> InflightGuard<'_> {
        if self.admitted {
            self.tailtriage.inflight(gauge)
        } else {
            InflightGuard {
                tailtriage: self.tailtriage,
                gauge: gauge.into(),
                enabled: false,
            }
        }
    }
}

impl RequestCompletion<'_> {
    /// Finishes this request with an explicit [`Outcome`].
    pub fn finish(mut self, outcome: Outcome) {
        self.finish_internal(outcome);
    }

    /// Convenience helper for successfully completed requests.
    pub fn finish_ok(self) {
        self.finish(Outcome::Ok);
    }

    /// Finishes this request from `result` and returns `result` unchanged.
    ///
    /// # Errors
    ///
    /// This method does not create new errors. It returns `result` unchanged,
    /// including the original `Err(E)` value.
    pub fn finish_result<T, E>(self, result: Result<T, E>) -> Result<T, E> {
        let outcome = if result.is_ok() {
            Outcome::Ok
        } else {
            Outcome::Error
        };
        self.finish(outcome);
        result
    }

    fn finish_internal(&mut self, outcome: Outcome) {
        if let Some(armed) = self.state.take() {
            self.tailtriage
                .resolve_request(armed.pending_key, armed.interval_start, outcome);
        }
    }
}

impl OwnedRequestHandle {
    /// Correlation ID attached to this request.
    #[must_use]
    pub fn request_id(&self) -> &str {
        &self.request_id
    }

    /// Route/operation name attached to this request.
    #[must_use]
    pub fn route(&self) -> &str {
        &self.route
    }

    /// Optional kind metadata attached to this request.
    #[must_use]
    pub fn kind(&self) -> Option<&str> {
        self.kind.as_deref()
    }

    /// Starts queue-wait timing instrumentation for `queue`.
    #[must_use]
    pub fn queue(&self, queue: impl Into<String>) -> QueueTimer<'_> {
        QueueTimer {
            tailtriage: self.tailtriage.as_ref(),
            enabled: self.admitted,
            request_id: self.request_id.clone(),
            queue: queue.into(),
            depth_at_start: None,
        }
    }

    /// Starts stage timing instrumentation for `stage`.
    #[must_use]
    pub fn stage(&self, stage: impl Into<String>) -> StageTimer<'_> {
        StageTimer {
            tailtriage: self.tailtriage.as_ref(),
            enabled: self.admitted,
            request_id: self.request_id.clone(),
            stage: stage.into(),
        }
    }

    /// Creates an in-flight guard for `gauge`.
    #[must_use]
    pub fn inflight(&self, gauge: impl Into<String>) -> InflightGuard<'_> {
        if self.admitted {
            self.tailtriage.as_ref().inflight(gauge)
        } else {
            InflightGuard {
                tailtriage: self.tailtriage.as_ref(),
                gauge: gauge.into(),
                enabled: false,
            }
        }
    }
}

impl OwnedRequestCompletion {
    /// Finishes the request with explicit outcome.
    pub fn finish(mut self, outcome: Outcome) {
        self.finish_internal(outcome);
    }

    /// Finishes the request as success.
    pub fn finish_ok(self) {
        self.finish(Outcome::Ok);
    }

    /// Maps `result` into request outcome and returns the original result.
    ///
    /// # Errors
    ///
    /// This method does not create new errors. It returns `result` unchanged,
    /// including the original `Err(E)` value.
    pub fn finish_result<T, E>(self, result: Result<T, E>) -> Result<T, E> {
        let outcome = if result.is_ok() {
            Outcome::Ok
        } else {
            Outcome::Error
        };
        self.finish(outcome);
        result
    }

    fn finish_internal(&mut self, outcome: Outcome) {
        if let Some(armed) = self.state.take() {
            self.tailtriage
                .resolve_request(armed.pending_key, armed.interval_start, outcome);
        }
    }
}

fn request_event_from_finished_interval(
    pending: PendingRequest,
    finished: FinishedInterval,
    outcome: Outcome,
) -> RequestEvent {
    debug_assert_eq!(
        pending.interval_start.started_at_unix_ms,
        finished.started_at_unix_ms
    );
    RequestEvent {
        request_id: pending.request_id,
        route: pending.route,
        kind: pending.kind,
        started_at_unix_ms: finished.started_at_unix_ms,
        started_at_run_us: finished.started_at_run_us,
        finished_at_unix_ms: finished.finished_at_unix_ms,
        finished_at_run_us: finished.finished_at_run_us,
        latency_us: finished.duration_us,
        outcome: outcome.into_string(),
    }
}

impl Drop for RequestCompletion<'_> {
    fn drop(&mut self) {
        if let Some(armed) = self.state.take() {
            self.tailtriage.resolve_request(
                armed.pending_key,
                armed.interval_start,
                Outcome::Cancelled,
            );
        }
    }
}

impl Drop for OwnedRequestCompletion {
    fn drop(&mut self) {
        if let Some(armed) = self.state.take() {
            self.tailtriage.resolve_request(
                armed.pending_key,
                armed.interval_start,
                Outcome::Cancelled,
            );
        }
    }
}

fn normalize_for_lifecycle(run: &Run) -> Run {
    let normalized = crate::normalize_run_permissive(run);
    let warnings = crate::summarize_run_validation_lifecycle(&normalized);
    let mut run = normalized.run;
    for warning in warnings {
        if !run.metadata.lifecycle_warnings.contains(&warning) {
            run.metadata.lifecycle_warnings.push(warning);
        }
    }
    run
}

pub(crate) fn lock_state(
    mutex: &Mutex<CollectorState>,
) -> std::sync::MutexGuard<'_, CollectorState> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn wait_state_closed<'a>(
    cell: &CollectorStateCell,
    guard: std::sync::MutexGuard<'a, CollectorState>,
) -> std::sync::MutexGuard<'a, CollectorState> {
    match cell.closed.wait(guard) {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn prior_sink_failure(message: &str) -> SinkError {
    SinkError::Io(std::io::Error::other(format!(
        "prior tailtriage sink attempt failed: {message}"
    )))
}

pub(crate) fn generate_run_id() -> String {
    format!("run-{}", uuid::Uuid::new_v4())
}

fn lookup_host_name() -> Option<String> {
    let os_host = hostname::get().ok()?;
    normalize_host_name(os_host)
}

fn normalize_host_name(host: OsString) -> Option<String> {
    let host = host.into_string().ok()?;
    let trimmed = host.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_owned())
}

fn generate_request_id(route: &str) -> String {
    let route_prefix = route
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect::<String>();
    let sequence = REQUEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("{route_prefix}-{}-{sequence}", unix_time_ms())
}

static REQUEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static PENDING_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[cfg(test)]
mod tests {
    use super::normalize_host_name;
    use std::ffi::OsString;

    #[test]
    fn normalize_host_name_rejects_blank_values() {
        assert_eq!(normalize_host_name(OsString::from("")), None);
        assert_eq!(normalize_host_name(OsString::from("   ")), None);
    }

    #[test]
    fn normalize_host_name_trims_non_blank_values() {
        assert_eq!(
            normalize_host_name(OsString::from(" checkout-host \n")),
            Some("checkout-host".to_owned())
        );
    }
}
