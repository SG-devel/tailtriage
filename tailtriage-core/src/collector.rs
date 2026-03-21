use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::InflightGuard;
use crate::RunSink;
use crate::{
    unix_time_ms, BuildError, CaptureLimits, CaptureMode, Config, InFlightSnapshot, LocalJsonSink,
    QueueEvent, QueueTimer, RequestEvent, RequestMeta, Run, RunMetadata, RuntimeSnapshot,
    SinkError, StageEvent, StageTimer,
};

/// Per-run collector that records request events and writes the final artifact.
///
/// [`Tailtriage`] is intentionally small: initialize once per process/run,
/// wrap request futures with [`Self::request_with_meta`], wrap critical await points with
/// stage/queue helpers, then flush one JSON artifact for CLI triage.
///
/// # Example
/// ```
/// use futures_executor::block_on;
/// use tailtriage_core::{Config, RequestMeta, Tailtriage};
///
/// let mut config = Config::new("api");
/// config.output_path = std::env::temp_dir().join("tailtriage-api.json");
/// let tailtriage = Tailtriage::init(config)?;
///
/// let request_id = "req-1".to_string();
/// let meta = RequestMeta::new(request_id.clone(), "/checkout").with_kind("http");
///
/// block_on(tailtriage.request_with_meta(meta, "ok", async {
///     tailtriage
///         .queue(request_id.clone(), "ingress")
///         .await_on(async {})
///         .await;
///     tailtriage
///         .stage(request_id, "db")
///         .await_value(async {})
///         .await;
/// }));
///
/// tailtriage.flush()?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug)]
pub struct Tailtriage {
    pub(crate) run: Mutex<Run>,
    pub(crate) inflight_counts: Mutex<HashMap<String, u64>>,
    pub(crate) sink: LocalJsonSink,
    pub(crate) limits: crate::CaptureLimits,
    pub(crate) runtime_sampling_interval: Option<Duration>,
}

impl Tailtriage {
    /// Starts a builder for one tailtriage collector instance.
    #[must_use]
    pub fn builder(service_name: impl Into<String>) -> TailtriageBuilder {
        TailtriageBuilder::new(service_name)
    }

    /// Initializes tailtriage collection for one service run.
    ///
    /// # Errors
    ///
    /// Returns [`InitError::EmptyServiceName`] if `config.service_name` is blank.
    pub fn init(config: Config) -> Result<Self, BuildError> {
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
            host: None,
            pid: Some(std::process::id()),
        });

        Ok(Self {
            run: Mutex::new(run),
            inflight_counts: Mutex::new(HashMap::new()),
            sink: LocalJsonSink::new(config.output_path),
            limits: config.capture_limits,
            runtime_sampling_interval: config.runtime_sampling_interval,
        })
    }

    /// Returns configured Tokio runtime sampling interval.
    #[must_use]
    pub fn runtime_sampling_interval(&self) -> Option<Duration> {
        self.runtime_sampling_interval
    }

    /// Creates a request context using an auto-generated request ID.
    #[must_use]
    pub fn request(&self, route: impl Into<String>) -> RequestContext<'_> {
        let meta = RequestMeta::for_route(route);
        RequestContext::new(self, meta.request_id, meta.route)
    }

    /// Creates a request context using caller-supplied request ID.
    #[must_use]
    pub fn request_with_id(
        &self,
        route: impl Into<String>,
        request_id: impl Into<String>,
    ) -> RequestContext<'_> {
        RequestContext::new(self, request_id.into(), route.into())
    }

    /// Times one request future and records its completion as a [`RequestEvent`].
    ///
    /// `outcome` should represent your application-level request result (for example:
    /// `"ok"`, `"error"`, or `"timeout"`).
    pub async fn request_with_meta<Fut, T>(
        &self,
        meta: RequestMeta,
        outcome: impl Into<String>,
        fut: Fut,
    ) -> T
    where
        Fut: std::future::Future<Output = T>,
    {
        let started_at_unix_ms = unix_time_ms();
        let started = Instant::now();
        let value = fut.await;
        let finished_at_unix_ms = unix_time_ms();

        self.record_request_fields(
            meta.request_id,
            meta.route,
            meta.kind,
            (started_at_unix_ms, finished_at_unix_ms),
            duration_to_us(started.elapsed()),
            outcome,
        );

        value
    }

    /// Returns a clone of the current in-memory run state.
    #[must_use]
    pub fn snapshot(&self) -> Run {
        lock_run(&self.run).clone()
    }

    /// Records one request event using pre-computed timing and outcome fields.
    pub fn record_request_fields(
        &self,
        request_id: impl Into<String>,
        route: impl Into<String>,
        kind: Option<String>,
        time_window_unix_ms: (u64, u64),
        latency_us: u64,
        outcome: impl Into<String>,
    ) {
        let (started_at_unix_ms, finished_at_unix_ms) = time_window_unix_ms;
        let event = RequestEvent {
            request_id: request_id.into(),
            route: route.into(),
            kind,
            started_at_unix_ms,
            finished_at_unix_ms,
            latency_us,
            outcome: outcome.into(),
        };
        self.record_request_event(event);
    }

    /// Writes the current run to the configured sink.
    ///
    /// # Errors
    ///
    /// Returns [`SinkError`] if writing or serialization fails.
    pub fn flush(&self) -> Result<(), SinkError> {
        let mut guard = lock_run(&self.run);
        guard.metadata.finished_at_unix_ms = unix_time_ms();
        self.sink.write(&guard)
    }

    /// Finalizes capture and writes artifact to disk.
    ///
    /// # Errors
    ///
    /// Returns [`SinkError`] if writing or serialization fails.
    pub fn shutdown(&self) -> Result<(), SinkError> {
        self.flush()
    }

    /// Returns the output file path used by the configured sink.
    #[must_use]
    pub fn output_path(&self) -> &Path {
        self.sink.path()
    }

    /// Creates an in-flight guard for `gauge`.
    ///
    /// The counter is incremented on creation and decremented when the returned
    /// guard is dropped.
    #[must_use]
    pub fn inflight(&self, gauge: impl Into<String>) -> InflightGuard<'_> {
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

    /// Returns a stage timing wrapper for one awaited operation.
    ///
    /// Use stage wrappers for downstream work such as DB/HTTP/cache calls.
    /// Pick [`crate::StageTimer::await_on`] when the stage naturally returns
    /// `Result<T, E>`, or [`crate::StageTimer::await_value`] for infallible
    /// futures where success should always be recorded as `true`.
    #[must_use]
    pub fn stage(&self, request_id: impl Into<String>, stage: impl Into<String>) -> StageTimer<'_> {
        StageTimer {
            tailtriage: self,
            request_id: request_id.into(),
            stage: stage.into(),
        }
    }

    /// Returns a queue timing wrapper for one awaited operation.
    ///
    /// Use this around waits caused by application queueing/backpressure
    /// (for example a semaphore permit wait or bounded channel receive).
    #[must_use]
    pub fn queue(&self, request_id: impl Into<String>, queue: impl Into<String>) -> QueueTimer<'_> {
        QueueTimer {
            tailtriage: self,
            request_id: request_id.into(),
            queue: queue.into(),
            depth_at_start: None,
        }
    }

    /// Records one Tokio runtime metrics sample.
    pub fn record_runtime_snapshot(&self, snapshot: RuntimeSnapshot) {
        let mut run = lock_run(&self.run);
        if run.runtime_snapshots.len() >= self.limits.max_runtime_snapshots {
            run.truncation.dropped_runtime_snapshots =
                run.truncation.dropped_runtime_snapshots.saturating_add(1);
        } else {
            run.runtime_snapshots.push(snapshot);
        }
    }

    pub(crate) fn record_stage_event(&self, event: StageEvent) {
        let mut run = lock_run(&self.run);
        if run.stages.len() >= self.limits.max_stages {
            run.truncation.dropped_stages = run.truncation.dropped_stages.saturating_add(1);
        } else {
            run.stages.push(event);
        }
    }

    pub(crate) fn record_queue_event(&self, event: QueueEvent) {
        let mut run = lock_run(&self.run);
        if run.queues.len() >= self.limits.max_queues {
            run.truncation.dropped_queues = run.truncation.dropped_queues.saturating_add(1);
        } else {
            run.queues.push(event);
        }
    }

    pub(crate) fn record_inflight_snapshot(&self, snapshot: InFlightSnapshot) {
        let mut run = lock_run(&self.run);
        if run.inflight.len() >= self.limits.max_inflight_snapshots {
            run.truncation.dropped_inflight_snapshots =
                run.truncation.dropped_inflight_snapshots.saturating_add(1);
        } else {
            run.inflight.push(snapshot);
        }
    }

    fn record_request_event(&self, event: RequestEvent) {
        let mut run = lock_run(&self.run);
        if run.requests.len() >= self.limits.max_requests {
            run.truncation.dropped_requests = run.truncation.dropped_requests.saturating_add(1);
        } else {
            run.requests.push(event);
        }
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

pub(crate) fn duration_to_us(duration: Duration) -> u64 {
    duration.as_micros().try_into().unwrap_or(u64::MAX)
}

/// Builder for [`Tailtriage`].
#[derive(Debug, Clone)]
pub struct TailtriageBuilder {
    config: Config,
}

impl TailtriageBuilder {
    #[must_use]
    pub fn new(service_name: impl Into<String>) -> Self {
        Self {
            config: Config::new(service_name),
        }
    }

    #[must_use]
    pub fn light(mut self) -> Self {
        self.config.mode = CaptureMode::Light;
        self
    }

    #[must_use]
    pub fn investigation(mut self) -> Self {
        self.config.mode = CaptureMode::Investigation;
        self
    }

    #[must_use]
    pub fn output(mut self, output_path: impl Into<std::path::PathBuf>) -> Self {
        self.config.output_path = output_path.into();
        self
    }

    #[must_use]
    pub fn service_version(mut self, service_version: impl Into<String>) -> Self {
        self.config.service_version = Some(service_version.into());
        self
    }

    #[must_use]
    pub fn run_id(mut self, run_id: impl Into<String>) -> Self {
        self.config.run_id = Some(run_id.into());
        self
    }

    #[must_use]
    pub fn capture_limits(mut self, capture_limits: CaptureLimits) -> Self {
        self.config.capture_limits = capture_limits;
        self
    }

    #[must_use]
    pub fn runtime_sampling_interval(mut self, interval: Duration) -> Self {
        self.config.runtime_sampling_interval = Some(interval);
        self
    }

    /// Builds a [`Tailtriage`] collector from builder settings.
    ///
    /// # Errors
    ///
    /// Returns [`BuildError::EmptyServiceName`] when service name is blank.
    pub fn build(self) -> Result<Tailtriage, BuildError> {
        Tailtriage::init(self.config)
    }
}

/// Request-scoped instrumentation context.
#[derive(Debug)]
pub struct RequestContext<'a> {
    tailtriage: &'a Tailtriage,
    request_id: String,
    route: String,
    kind: Option<String>,
}

impl<'a> RequestContext<'a> {
    fn new(tailtriage: &'a Tailtriage, request_id: String, route: String) -> Self {
        Self {
            tailtriage,
            request_id,
            route,
            kind: None,
        }
    }

    #[must_use]
    pub fn with_kind(mut self, kind: impl Into<String>) -> Self {
        self.kind = Some(kind.into());
        self
    }

    #[must_use]
    pub fn stage(&self, stage: impl Into<String>) -> StageTimer<'a> {
        self.tailtriage.stage(self.request_id.clone(), stage)
    }

    #[must_use]
    pub fn queue(&self, queue: impl Into<String>) -> QueueTimer<'a> {
        self.tailtriage.queue(self.request_id.clone(), queue)
    }

    #[must_use]
    pub fn inflight(&self, gauge: impl Into<String>) -> InflightGuard<'a> {
        self.tailtriage.inflight(gauge)
    }

    pub fn complete(
        self,
        time_window_unix_ms: (u64, u64),
        latency_us: u64,
        outcome: impl Into<String>,
    ) {
        self.tailtriage.record_request_fields(
            self.request_id,
            self.route,
            self.kind,
            time_window_unix_ms,
            latency_us,
            outcome,
        );
    }

    pub async fn run<Fut, T>(self, outcome: impl Into<String>, fut: Fut) -> T
    where
        Fut: std::future::Future<Output = T>,
    {
        let started_at_unix_ms = unix_time_ms();
        let started = Instant::now();
        let value = fut.await;
        let finished_at_unix_ms = unix_time_ms();
        self.complete(
            (started_at_unix_ms, finished_at_unix_ms),
            duration_to_us(started.elapsed()),
            outcome,
        );
        value
    }
}

fn generate_run_id() -> String {
    format!("run-{}", unix_time_ms())
}
