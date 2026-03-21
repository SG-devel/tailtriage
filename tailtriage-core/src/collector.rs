use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::config::Config;
use crate::InflightGuard;
use crate::RunSink;
use crate::{
    unix_time_ms, InFlightSnapshot, InitError, LocalJsonSink, QueueEvent, QueueTimer, RequestEvent,
    Run, RunMetadata, RuntimeSnapshot, SinkError, StageEvent, StageTimer, TailtriageBuilder,
};

/// Per-run collector that records request events and writes the final artifact.
#[derive(Debug)]
pub struct Tailtriage {
    pub(crate) run: Mutex<Run>,
    pub(crate) inflight_counts: Mutex<HashMap<String, u64>>,
    pub(crate) sink: LocalJsonSink,
    pub(crate) limits: crate::CaptureLimits,
}

impl Tailtriage {
    /// Starts building a new tailtriage run for `service_name`.
    #[must_use]
    pub fn builder(service_name: impl Into<String>) -> TailtriageBuilder {
        TailtriageBuilder::new(service_name)
    }

    pub(crate) fn from_config(config: Config) -> Result<Self, InitError> {
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
        })
    }

    /// Starts one reusable request context.
    #[must_use]
    pub fn begin_request(&self, route: impl Into<String>) -> RequestContext<'_> {
        RequestContext {
            tailtriage: self,
            request_id: generate_request_id(),
            route: route.into(),
            kind: None,
            started_at_unix_ms: unix_time_ms(),
            started: Instant::now(),
        }
    }

    /// Writes the current run to the configured sink and closes the run.
    ///
    /// # Errors
    ///
    /// Returns [`SinkError`] if writing or serialization fails.
    pub fn shutdown(&self) -> Result<(), SinkError> {
        let mut guard = lock_run(&self.run);
        guard.metadata.finished_at_unix_ms = unix_time_ms();
        self.sink.write(&guard)
    }

    /// Returns a clone of the current in-memory run state.
    #[must_use]
    pub fn snapshot(&self) -> Run {
        lock_run(&self.run).clone()
    }

    /// Returns the output file path used by the configured sink.
    #[must_use]
    pub fn output_path(&self) -> &Path {
        self.sink.path()
    }

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

    pub fn stage(&self, request_id: impl Into<String>, stage: impl Into<String>) -> StageTimer<'_> {
        StageTimer {
            tailtriage: self,
            request_id: request_id.into(),
            stage: stage.into(),
        }
    }

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

    pub(crate) fn record_request_event(&self, event: RequestEvent) {
        let mut run = lock_run(&self.run);
        if run.requests.len() >= self.limits.max_requests {
            run.truncation.dropped_requests = run.truncation.dropped_requests.saturating_add(1);
        } else {
            run.requests.push(event);
        }
    }
}

/// Reusable request context that carries correlation across fractured code.
#[derive(Debug)]
pub struct RequestContext<'a> {
    tailtriage: &'a Tailtriage,
    request_id: String,
    route: String,
    kind: Option<String>,
    started_at_unix_ms: u64,
    started: Instant,
}

impl RequestContext<'_> {
    /// Overrides the generated request ID.
    #[must_use]
    pub fn with_request_id(mut self, request_id: impl Into<String>) -> Self {
        self.request_id = request_id.into();
        self
    }

    /// Sets an optional semantic request kind.
    #[must_use]
    pub fn kind(mut self, kind: impl Into<String>) -> Self {
        self.kind = Some(kind.into());
        self
    }

    /// Returns the request correlation ID.
    #[must_use]
    pub fn request_id(&self) -> &str {
        &self.request_id
    }

    /// Records a named inflight gauge around a scope.
    #[must_use]
    pub fn inflight(&self, gauge: impl Into<String>) -> InflightGuard<'_> {
        self.tailtriage.inflight(gauge)
    }

    /// Times one queue wait in this request context.
    #[must_use]
    pub fn queue(&self, queue: impl Into<String>) -> QueueTimer<'_> {
        self.tailtriage.queue(self.request_id.clone(), queue)
    }

    /// Times one stage in this request context.
    #[must_use]
    pub fn stage(&self, stage: impl Into<String>) -> StageTimer<'_> {
        self.tailtriage.stage(self.request_id.clone(), stage)
    }

    /// Completes this request context with an outcome.
    pub fn complete(&self, outcome: impl Into<String>) {
        let finished_at_unix_ms = unix_time_ms();
        self.tailtriage.record_request_event(RequestEvent {
            request_id: self.request_id.clone(),
            route: self.route.clone(),
            kind: self.kind.clone(),
            started_at_unix_ms: self.started_at_unix_ms,
            finished_at_unix_ms,
            latency_us: duration_to_us(self.started.elapsed()),
            outcome: outcome.into(),
        });
    }

    /// Convenience sugar over [`Self::complete`] that awaits `fut` first.
    pub async fn run<Fut, T>(&self, outcome: impl Into<String>, fut: Fut) -> T
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

fn generate_request_id() -> String {
    let sequence = REQUEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("req-{}-{sequence}", unix_time_ms())
}

static REQUEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);
