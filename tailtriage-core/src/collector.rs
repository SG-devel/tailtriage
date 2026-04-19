use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::config::Config;
use crate::InflightGuard;
use crate::RunSink;
use crate::{
    unix_time_ms, BuildError, InFlightSnapshot, Outcome, QueueEvent, QueueTimer, RequestEvent,
    RequestOptions, Run, RunMetadata, RuntimeSnapshot, SinkError, StageEvent, StageTimer,
    UnfinishedRequestSample,
};

/// Per-run collector that records request events and writes the final artifact.
pub struct Tailtriage {
    pub(crate) run: Mutex<Run>,
    pub(crate) inflight_counts: Mutex<HashMap<String, u64>>,
    pending_requests: Mutex<HashMap<u64, PendingRequest>>,
    pub(crate) sink: Arc<dyn RunSink + Send + Sync>,
    pub(crate) mode: crate::CaptureMode,
    pub(crate) effective_core_config: crate::EffectiveCoreConfig,
    pub(crate) limits: crate::CaptureLimits,
    pub(crate) strict_lifecycle: bool,
}

#[derive(Debug, Clone)]
struct PendingRequest {
    request_id: String,
    route: String,
    kind: Option<String>,
    started_at_unix_ms: u64,
    started: Instant,
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
#[must_use = "request completion must be finished explicitly"]
#[derive(Debug)]
pub struct StartedRequest<'a> {
    /// Instrumentation handle for queue/stage/inflight timing.
    pub handle: RequestHandle<'a>,
    /// Single-owner completion token for explicit request finish.
    pub completion: RequestCompletion<'a>,
}

/// Split request lifecycle start result backed by `Arc<Tailtriage>`.
#[must_use = "request completion must be finished explicitly"]
#[derive(Debug)]
pub struct OwnedStartedRequest {
    /// Instrumentation handle for queue/stage/inflight timing.
    pub handle: OwnedRequestHandle,
    /// Single-owner completion token for explicit request finish.
    pub completion: OwnedRequestCompletion,
}

/// Instrumentation-facing request handle.
#[derive(Debug, Clone)]
pub struct RequestHandle<'a> {
    tailtriage: &'a Tailtriage,
    request_id: String,
    route: String,
    kind: Option<String>,
}

/// Instrumentation-facing request handle backed by `Arc<Tailtriage>`.
#[derive(Debug, Clone)]
pub struct OwnedRequestHandle {
    tailtriage: Arc<Tailtriage>,
    request_id: String,
    route: String,
    kind: Option<String>,
}

/// Completion-facing request token.
#[must_use = "request completion tokens must be finished explicitly"]
#[derive(Debug)]
pub struct RequestCompletion<'a> {
    tailtriage: &'a Tailtriage,
    pending_key: u64,
    finished: bool,
}

/// Completion-facing request token backed by `Arc<Tailtriage>`.
#[must_use = "request completion tokens must be finished explicitly"]
#[derive(Debug)]
pub struct OwnedRequestCompletion {
    tailtriage: Arc<Tailtriage>,
    pending_key: u64,
    finished: bool,
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

        let now = unix_time_ms();
        let run = Run::new(RunMetadata {
            run_id: config.run_id.unwrap_or_else(generate_run_id),
            service_name: config.service_name,
            service_version: config.service_version,
            started_at_unix_ms: now,
            finished_at_unix_ms: now,
            mode: config.mode,
            effective_core_config: Some(config.effective_core),
            host: None,
            pid: Some(std::process::id()),
            lifecycle_warnings: Vec::new(),
            unfinished_requests: crate::UnfinishedRequests::default(),
        });

        Ok(Self {
            run: Mutex::new(run),
            inflight_counts: Mutex::new(HashMap::new()),
            pending_requests: Mutex::new(HashMap::new()),
            sink: config.sink,
            mode: config.mode,
            effective_core_config: config.effective_core,
            limits: config.effective_core.capture_limits,
            strict_lifecycle: config.strict_lifecycle,
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
    pub fn begin_request(&self, route: impl Into<String>) -> StartedRequest<'_> {
        self.begin_request_with(route, RequestOptions::new())
    }

    /// Starts a request with optional caller-provided request options.
    pub fn begin_request_with(
        &self,
        route: impl Into<String>,
        options: RequestOptions,
    ) -> StartedRequest<'_> {
        let (request_id, route, kind, pending_key) = self.start_request(route.into(), options);

        StartedRequest {
            handle: RequestHandle {
                tailtriage: self,
                request_id: request_id.clone(),
                route,
                kind,
            },
            completion: RequestCompletion {
                tailtriage: self,
                pending_key,
                finished: false,
            },
        }
    }

    /// Starts a request with autogenerated correlation for `route` using `Arc<Tailtriage>`.
    pub fn begin_request_owned(self: &Arc<Self>, route: impl Into<String>) -> OwnedStartedRequest {
        self.begin_request_with_owned(route, RequestOptions::new())
    }

    /// Starts a request with caller-provided options using `Arc<Tailtriage>`.
    pub fn begin_request_with_owned(
        self: &Arc<Self>,
        route: impl Into<String>,
        options: RequestOptions,
    ) -> OwnedStartedRequest {
        let (request_id, route, kind, pending_key) = self.start_request(route.into(), options);

        OwnedStartedRequest {
            handle: OwnedRequestHandle {
                tailtriage: Arc::clone(self),
                request_id: request_id.clone(),
                route,
                kind,
            },
            completion: OwnedRequestCompletion {
                tailtriage: Arc::clone(self),
                pending_key,
                finished: false,
            },
        }
    }

    /// Returns a clone of the current in-memory run state.
    #[must_use]
    pub fn snapshot(&self) -> Run {
        lock_run(&self.run).clone()
    }

    /// Writes the current run artifact and finishes the run lifecycle.
    ///
    /// # Errors
    ///
    /// Returns [`SinkError`] if serialization or writing fails.
    pub fn shutdown(&self) -> Result<(), SinkError> {
        let mut pending_samples = Vec::new();
        let pending_count = {
            let pending = lock_pending(&self.pending_requests);
            pending_samples.extend(pending.values().take(5).map(|req| UnfinishedRequestSample {
                request_id: req.request_id.clone(),
                route: req.route.clone(),
            }));
            pending.len()
        };

        let mut guard = lock_run(&self.run);
        guard.metadata.finished_at_unix_ms = unix_time_ms();
        if pending_count > 0 {
            guard.metadata.lifecycle_warnings.push(format!(
                "{pending_count} unfinished request(s) remained at shutdown; run includes no fabricated completions"
            ));
            guard.metadata.unfinished_requests.count = pending_count as u64;
            guard.metadata.unfinished_requests.sample = pending_samples;
            if self.strict_lifecycle {
                return Err(SinkError::Lifecycle {
                    unfinished_count: pending_count,
                });
            }
        }

        self.sink.write(&guard)
    }

    /// Creates an in-flight guard for `gauge`.
    #[must_use]
    pub(crate) fn inflight(&self, gauge: impl Into<String>) -> InflightGuard<'_> {
        let gauge = gauge.into();
        let count = {
            let mut counts = lock_map(&self.inflight_counts);
            let entry = counts.entry(gauge.clone()).or_insert(0);
            *entry += 1;
            *entry
        };

        self.record_inflight_snapshot(InFlightSnapshot {
            gauge: gauge.clone(),
            at_unix_ms: unix_time_ms(),
            count,
        });

        InflightGuard {
            tailtriage: self,
            gauge,
        }
    }

    /// Records one runtime metrics sample captured by an integration crate.
    pub fn record_runtime_snapshot(&self, snapshot: RuntimeSnapshot) {
        let mut run = lock_run(&self.run);
        if run.runtime_snapshots.len() >= self.limits.max_runtime_snapshots {
            run.truncation.limits_hit = true;
            run.truncation.dropped_runtime_snapshots =
                run.truncation.dropped_runtime_snapshots.saturating_add(1);
        } else {
            run.runtime_snapshots.push(snapshot);
        }
    }

    pub(crate) fn record_stage_event(&self, event: StageEvent) {
        let mut run = lock_run(&self.run);
        if run.stages.len() >= self.limits.max_stages {
            run.truncation.limits_hit = true;
            run.truncation.dropped_stages = run.truncation.dropped_stages.saturating_add(1);
        } else {
            run.stages.push(event);
        }
    }

    pub(crate) fn record_queue_event(&self, event: QueueEvent) {
        let mut run = lock_run(&self.run);
        if run.queues.len() >= self.limits.max_queues {
            run.truncation.limits_hit = true;
            run.truncation.dropped_queues = run.truncation.dropped_queues.saturating_add(1);
        } else {
            run.queues.push(event);
        }
    }

    pub(crate) fn record_inflight_snapshot(&self, snapshot: InFlightSnapshot) {
        let mut run = lock_run(&self.run);
        if run.inflight.len() >= self.limits.max_inflight_snapshots {
            run.truncation.limits_hit = true;
            run.truncation.dropped_inflight_snapshots =
                run.truncation.dropped_inflight_snapshots.saturating_add(1);
        } else {
            run.inflight.push(snapshot);
        }
    }

    fn record_request_event(&self, event: RequestEvent) {
        let mut run = lock_run(&self.run);
        if run.requests.len() >= self.limits.max_requests {
            run.truncation.limits_hit = true;
            run.truncation.dropped_requests = run.truncation.dropped_requests.saturating_add(1);
        } else {
            run.requests.push(event);
        }
    }

    fn start_request(
        &self,
        route: String,
        options: RequestOptions,
    ) -> (String, String, Option<String>, u64) {
        let request_id = options
            .request_id
            .unwrap_or_else(|| generate_request_id(&route));
        let pending_key = PENDING_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let kind = options.kind;
        let pending = PendingRequest {
            request_id: request_id.clone(),
            route: route.clone(),
            kind: kind.clone(),
            started_at_unix_ms: unix_time_ms(),
            started: Instant::now(),
        };
        lock_pending(&self.pending_requests).insert(pending_key, pending);

        (request_id, route, kind, pending_key)
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
    #[must_use]
    pub fn queue(&self, queue: impl Into<String>) -> QueueTimer<'_> {
        QueueTimer {
            tailtriage: self.tailtriage,
            request_id: self.request_id.clone(),
            queue: queue.into(),
            depth_at_start: None,
        }
    }

    /// Starts stage timing instrumentation for `stage`.
    #[must_use]
    pub fn stage(&self, stage: impl Into<String>) -> StageTimer<'_> {
        StageTimer {
            tailtriage: self.tailtriage,
            request_id: self.request_id.clone(),
            stage: stage.into(),
        }
    }

    /// Increments in-flight gauge tracking for `gauge` until the returned guard drops.
    #[must_use]
    pub fn inflight(&self, gauge: impl Into<String>) -> InflightGuard<'_> {
        self.tailtriage.inflight(gauge)
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
        if self.finished {
            debug_assert!(
                !self.finished,
                "tailtriage request completion was finished more than once; each request must be finished exactly once"
            );
            return;
        }

        let pending = lock_pending(&self.tailtriage.pending_requests).remove(&self.pending_key);
        let Some(pending) = pending else {
            debug_assert!(
                false,
                "tailtriage request completion token had no pending request entry"
            );
            self.finished = true;
            return;
        };
        self.finished = true;

        self.tailtriage.record_request_event(RequestEvent {
            request_id: pending.request_id,
            route: pending.route,
            kind: pending.kind,
            started_at_unix_ms: pending.started_at_unix_ms,
            finished_at_unix_ms: unix_time_ms(),
            latency_us: duration_to_us(pending.started.elapsed()),
            outcome: outcome.into_string(),
        });
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
            request_id: self.request_id.clone(),
            stage: stage.into(),
        }
    }

    /// Creates an in-flight guard for `gauge`.
    #[must_use]
    pub fn inflight(&self, gauge: impl Into<String>) -> InflightGuard<'_> {
        self.tailtriage.as_ref().inflight(gauge)
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
        if self.finished {
            debug_assert!(
                !self.finished,
                "tailtriage request completion was finished more than once; each request must be finished exactly once"
            );
            return;
        }

        let pending = lock_pending(&self.tailtriage.pending_requests).remove(&self.pending_key);
        let Some(pending) = pending else {
            self.finished = true;
            return;
        };
        self.finished = true;

        self.tailtriage.record_request_event(RequestEvent {
            request_id: pending.request_id,
            route: pending.route,
            kind: pending.kind,
            started_at_unix_ms: pending.started_at_unix_ms,
            finished_at_unix_ms: unix_time_ms(),
            latency_us: duration_to_us(pending.started.elapsed()),
            outcome: outcome.into_string(),
        });
    }
}

impl Drop for RequestCompletion<'_> {
    fn drop(&mut self) {
        debug_assert!(
            self.finished || std::thread::panicking(),
            "tailtriage request completion dropped without finish(...), finish_ok(), or finish_result(...)"
        );
    }
}

impl Drop for OwnedRequestCompletion {
    fn drop(&mut self) {
        debug_assert!(
            self.finished || std::thread::panicking(),
            "tailtriage request completion dropped without finish(...), finish_ok(), or finish_result(...)"
        );
    }
}

pub(crate) fn lock_run(run: &Mutex<Run>) -> std::sync::MutexGuard<'_, Run> {
    match run.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

pub(crate) fn lock_map(
    map: &Mutex<HashMap<String, u64>>,
) -> std::sync::MutexGuard<'_, HashMap<String, u64>> {
    match map.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn lock_pending(
    map: &Mutex<HashMap<u64, PendingRequest>>,
) -> std::sync::MutexGuard<'_, HashMap<u64, PendingRequest>> {
    match map.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

pub(crate) fn duration_to_us(duration: Duration) -> u64 {
    duration.as_micros().try_into().unwrap_or(u64::MAX)
}

fn generate_run_id() -> String {
    format!("run-{}", unix_time_ms())
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
