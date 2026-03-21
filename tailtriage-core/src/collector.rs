use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::config::generate_request_id;
use crate::InflightGuard;
use crate::RunSink;
use crate::{
    unix_time_ms, BuildError, CaptureLimits, CaptureMode, InFlightSnapshot, LocalJsonSink, Outcome,
    QueueEvent, QueueTimer, RequestEvent, RequestOptions, Run, RunMetadata, RuntimeSnapshot,
    SamplingConfig, SinkError, StageEvent, StageTimer,
};

pub struct Tailtriage {
    pub(crate) run: Mutex<Run>,
    pub(crate) inflight_counts: Mutex<HashMap<String, u64>>,
    pub(crate) sink: Box<dyn RunSink + Send + Sync>,
    pub(crate) limits: CaptureLimits,
    sampling: SamplingConfig,
}

pub struct TailtriageBuilder {
    service_name: String,
    service_version: Option<String>,
    run_id: Option<String>,
    mode: CaptureMode,
    sink: Box<dyn RunSink + Send + Sync>,
    capture_limits: CaptureLimits,
    sampling: SamplingConfig,
}

pub struct RequestContext<'a> {
    tailtriage: &'a Tailtriage,
    request_id: String,
    route: String,
    kind: Option<String>,
    started_at_unix_ms: u64,
    started: Instant,
}

impl Tailtriage {
    #[must_use]
    pub fn builder(service_name: impl Into<String>) -> TailtriageBuilder {
        TailtriageBuilder {
            service_name: service_name.into(),
            service_version: None,
            run_id: None,
            mode: CaptureMode::Light,
            sink: Box::new(LocalJsonSink::new("tailtriage-run.json")),
            capture_limits: CaptureLimits::default(),
            sampling: SamplingConfig::disabled(),
        }
    }

    #[must_use]
    pub fn request(&self, route: impl Into<String>) -> RequestContext<'_> {
        self.request_with(route, RequestOptions::new())
    }

    #[must_use]
    pub fn request_with(
        &self,
        route: impl Into<String>,
        options: RequestOptions,
    ) -> RequestContext<'_> {
        let route = route.into();
        let request_id = options
            .request_id
            .unwrap_or_else(|| generate_request_id(route.as_str()));

        RequestContext {
            tailtriage: self,
            request_id,
            route,
            kind: None,
            started_at_unix_ms: unix_time_ms(),
            started: Instant::now(),
        }
    }

    /// Writes the run artifact to the configured sink.
    ///
    /// # Errors
    /// Returns [`SinkError`] if writing the artifact fails.
    pub fn shutdown(&self) -> Result<(), SinkError> {
        let mut guard = lock_run(&self.run);
        guard.metadata.finished_at_unix_ms = unix_time_ms();
        self.sink.write(&guard)
    }

    #[must_use]
    pub fn snapshot(&self) -> Run {
        lock_run(&self.run).clone()
    }

    #[must_use]
    pub fn configured_runtime_sampling_interval(&self) -> Option<Duration> {
        self.sampling.runtime_interval()
    }

    #[doc(hidden)]
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
        self.record_request_event(RequestEvent {
            request_id: request_id.into(),
            route: route.into(),
            kind,
            started_at_unix_ms,
            finished_at_unix_ms,
            latency_us,
            outcome: outcome.into(),
        });
    }

    pub(crate) fn record_request_event(&self, event: RequestEvent) {
        let mut run = lock_run(&self.run);
        if run.requests.len() >= self.limits.max_requests {
            run.truncation.dropped_requests = run.truncation.dropped_requests.saturating_add(1);
        } else {
            run.requests.push(event);
        }
    }

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
}

impl TailtriageBuilder {
    #[must_use]
    pub fn light(mut self) -> Self {
        self.mode = CaptureMode::Light;
        self
    }

    #[must_use]
    pub fn investigation(mut self) -> Self {
        self.mode = CaptureMode::Investigation;
        self
    }

    #[must_use]
    pub fn service_version(mut self, version: impl Into<String>) -> Self {
        self.service_version = Some(version.into());
        self
    }

    #[must_use]
    pub fn run_id(mut self, run_id: impl Into<String>) -> Self {
        self.run_id = Some(run_id.into());
        self
    }

    #[must_use]
    pub fn output(mut self, path: impl AsRef<Path>) -> Self {
        self.sink = Box::new(LocalJsonSink::new(path));
        self
    }

    #[must_use]
    pub fn sink<S>(mut self, sink: S) -> Self
    where
        S: RunSink + Send + Sync + 'static,
    {
        self.sink = Box::new(sink);
        self
    }

    #[must_use]
    pub fn capture_limits(mut self, limits: CaptureLimits) -> Self {
        self.capture_limits = limits;
        self
    }

    #[must_use]
    pub fn sampling(mut self, sampling: SamplingConfig) -> Self {
        self.sampling = sampling;
        self
    }

    /// Builds one run collector instance.
    ///
    /// # Errors
    /// Returns [`BuildError`] when the service name is blank or sampling is invalid.
    pub fn build(self) -> Result<Tailtriage, BuildError> {
        if self.service_name.trim().is_empty() {
            return Err(BuildError::EmptyServiceName);
        }
        if self
            .sampling
            .runtime_interval()
            .is_some_and(|interval| interval.is_zero())
        {
            return Err(BuildError::InvalidRuntimeSamplingInterval);
        }

        let now = unix_time_ms();
        let run = Run::new(RunMetadata {
            run_id: self.run_id.unwrap_or_else(generate_run_id),
            service_name: self.service_name,
            service_version: self.service_version,
            started_at_unix_ms: now,
            finished_at_unix_ms: now,
            mode: self.mode,
            host: None,
            pid: Some(std::process::id()),
        });

        Ok(Tailtriage {
            run: Mutex::new(run),
            inflight_counts: Mutex::new(HashMap::new()),
            sink: self.sink,
            limits: self.capture_limits,
            sampling: self.sampling,
        })
    }
}

impl<'a> RequestContext<'a> {
    #[must_use]
    pub fn with_kind(mut self, kind: impl Into<String>) -> Self {
        self.kind = Some(kind.into());
        self
    }

    #[must_use]
    pub fn request_id(&self) -> &str {
        self.request_id.as_str()
    }

    #[must_use]
    pub fn route(&self) -> &str {
        self.route.as_str()
    }

    #[must_use]
    pub fn kind(&self) -> Option<&str> {
        self.kind.as_deref()
    }

    #[must_use]
    pub fn queue(&self, queue: impl Into<String>) -> QueueTimer<'a> {
        QueueTimer {
            tailtriage: self.tailtriage,
            request_id: self.request_id.clone(),
            queue: queue.into(),
            depth_at_start: None,
        }
    }

    #[must_use]
    pub fn stage(&self, stage: impl Into<String>) -> StageTimer<'a> {
        StageTimer {
            tailtriage: self.tailtriage,
            request_id: self.request_id.clone(),
            stage: stage.into(),
        }
    }

    #[must_use]
    pub fn inflight(&self, gauge: impl Into<String>) -> InflightGuard<'a> {
        let gauge = gauge.into();
        let count = {
            let mut counts = lock_map(&self.tailtriage.inflight_counts);
            let entry = counts.entry(gauge.clone()).or_insert(0);
            *entry += 1;
            *entry
        };

        self.tailtriage.record_inflight_snapshot(InFlightSnapshot {
            gauge: gauge.clone(),
            at_unix_ms: unix_time_ms(),
            count,
        });

        InflightGuard {
            tailtriage: self.tailtriage,
            gauge,
        }
    }

    pub fn complete(self, outcome: Outcome) {
        self.tailtriage.record_request_event(RequestEvent {
            request_id: self.request_id,
            route: self.route,
            kind: self.kind,
            started_at_unix_ms: self.started_at_unix_ms,
            finished_at_unix_ms: unix_time_ms(),
            latency_us: duration_to_us(self.started.elapsed()),
            outcome: outcome.into_string(),
        });
    }

    pub async fn run<Fut, T>(self, outcome: Outcome, fut: Fut) -> T
    where
        Fut: std::future::Future<Output = T>,
    {
        let value = fut.await;
        self.complete(outcome);
        value
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
