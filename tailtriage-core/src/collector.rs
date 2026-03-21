use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use crate::InflightGuard;
use crate::RunSink;
use crate::{
    config::Config, unix_time_ms, CaptureLimits, CaptureMode, InFlightSnapshot, InitError,
    LocalJsonSink, QueueEvent, QueueTimer, RequestContext, RequestEvent, Run, RunMetadata,
    RuntimeSnapshot, SinkError, StageEvent, StageTimer,
};

/// Per-run collector that records request events and writes the final artifact.
///
/// [`Tailtriage`] is intentionally small: initialize once per process/run with
/// [`Self::builder`], create one [`RequestContext`] per request/work item,
/// instrument queue and stage waits through that context, then call
/// [`Self::shutdown`] to write one JSON artifact for CLI triage.
///
/// # Example
/// ```
/// use futures_executor::block_on;
/// use tailtriage_core::Tailtriage;
///
/// let tailtriage = Tailtriage::builder("api")
///     .output(std::env::temp_dir().join("tailtriage-api.json"))
///     .build()?;
///
/// let request = tailtriage.request("/checkout").with_kind("http");
/// block_on(async {
///     request.queue("ingress").await_on(async {}).await;
///     request.stage("db").await_value(async {}).await;
/// });
/// request.complete("ok");
///
/// tailtriage.shutdown()?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug)]
pub struct Tailtriage {
    pub(crate) run: Mutex<Run>,
    pub(crate) inflight_counts: Mutex<HashMap<String, u64>>,
    pub(crate) sink: LocalJsonSink,
    pub(crate) limits: crate::CaptureLimits,
    request_sequence: AtomicU64,
}

/// Builder for constructing one tailtriage run.
#[derive(Debug, Clone)]
pub struct TailtriageBuilder {
    config: Config,
}

impl Tailtriage {
    /// Starts building one tailtriage run for `service_name`.
    #[must_use]
    pub fn builder(service_name: impl Into<String>) -> TailtriageBuilder {
        TailtriageBuilder {
            config: Config::new(service_name),
        }
    }

    fn from_config(config: Config) -> Result<Self, InitError> {
        if config.service_name.trim().is_empty() {
            return Err(InitError::EmptyServiceName);
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
            request_sequence: AtomicU64::new(0),
        })
    }

    /// Starts one reusable request context for `route`.
    #[must_use]
    pub fn request(&self, route: impl Into<String>) -> RequestContext<'_> {
        let route = route.into();
        let request_id = self.next_request_id(&route);
        RequestContext::new(self, request_id, route)
    }

    /// Returns a clone of the current in-memory run state.
    #[must_use]
    pub fn snapshot(&self) -> Run {
        lock_run(&self.run).clone()
    }

    /// Records one request event using pre-computed timing and outcome fields.
    pub(crate) fn record_request_fields(
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
    pub fn shutdown(&self) -> Result<(), SinkError> {
        let mut guard = lock_run(&self.run);
        guard.metadata.finished_at_unix_ms = unix_time_ms();
        self.sink.write(&guard)
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

    /// Returns a stage timing wrapper for one awaited operation.
    ///
    /// Use stage wrappers for downstream work such as DB/HTTP/cache calls.
    /// Pick [`crate::StageTimer::await_on`] when the stage naturally returns
    /// `Result<T, E>`, or [`crate::StageTimer::await_value`] for infallible
    /// futures where success should always be recorded as `true`.
    #[must_use]
    pub(crate) fn stage(
        &self,
        request_id: impl Into<String>,
        stage: impl Into<String>,
    ) -> StageTimer<'_> {
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
    pub(crate) fn queue(
        &self,
        request_id: impl Into<String>,
        queue: impl Into<String>,
    ) -> QueueTimer<'_> {
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

    fn next_request_id(&self, route: &str) -> String {
        let route_prefix = route
            .chars()
            .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
            .collect::<String>();
        let sequence = self.request_sequence.fetch_add(1, Ordering::Relaxed);
        format!("{route_prefix}-{}-{sequence}", unix_time_ms())
    }
}

impl TailtriageBuilder {
    /// Configures light capture mode.
    #[must_use]
    pub fn light(mut self) -> Self {
        self.config.mode = CaptureMode::Light;
        self
    }

    /// Configures investigation capture mode.
    #[must_use]
    pub fn investigation(mut self) -> Self {
        self.config.mode = CaptureMode::Investigation;
        self
    }

    /// Sets the service version in run metadata.
    #[must_use]
    pub fn service_version(mut self, version: impl Into<String>) -> Self {
        self.config.service_version = Some(version.into());
        self
    }

    /// Sets an explicit run ID.
    #[must_use]
    pub fn run_id(mut self, run_id: impl Into<String>) -> Self {
        self.config.run_id = Some(run_id.into());
        self
    }

    /// Sets the JSON artifact output path.
    #[must_use]
    pub fn output(mut self, output_path: impl Into<std::path::PathBuf>) -> Self {
        self.config.output_path = output_path.into();
        self
    }

    /// Sets bounded capture limits.
    #[must_use]
    pub fn capture_limits(mut self, capture_limits: CaptureLimits) -> Self {
        self.config.capture_limits = capture_limits;
        self
    }

    /// Finalizes builder configuration and constructs a collector.
    ///
    /// # Errors
    ///
    /// Returns [`InitError::EmptyServiceName`] when the service name is blank.
    pub fn build(self) -> Result<Tailtriage, InitError> {
        Tailtriage::from_config(self.config)
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

fn generate_run_id() -> String {
    format!("run-{}", unix_time_ms())
}
