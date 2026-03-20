use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::InflightGuard;
use crate::RunSink;
use crate::{
    unix_time_ms, Config, InFlightSnapshot, InitError, LocalJsonSink, QueueTimer, RequestEvent,
    RequestMeta, Run, RunMetadata, RuntimeSnapshot, SinkError, StageTimer,
};

/// Per-run collector that records request events and writes the final artifact.
#[derive(Debug)]
pub struct Tailtriage {
    pub(crate) run: Mutex<Run>,
    pub(crate) inflight_counts: Mutex<HashMap<String, u64>>,
    pub(crate) sink: LocalJsonSink,
}

impl Tailtriage {
    /// Initializes tailtriage collection for one service run.
    ///
    /// # Errors
    ///
    /// Returns [`InitError::EmptyServiceName`] if `config.service_name` is blank.
    pub fn init(config: Config) -> Result<Self, InitError> {
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
        })
    }

    /// Times one request future and records its completion as a [`RequestEvent`].
    ///
    /// `outcome` should represent your application-level request result (for example:
    /// `"ok"`, `"error"`, or `"timeout"`).
    pub async fn request<Fut, T>(
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
        lock_run(&self.run).requests.push(RequestEvent {
            request_id: request_id.into(),
            route: route.into(),
            kind,
            started_at_unix_ms,
            finished_at_unix_ms,
            latency_us,
            outcome: outcome.into(),
        });
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

        lock_run(&self.run).inflight.push(InFlightSnapshot {
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
    #[must_use]
    pub fn stage(&self, request_id: impl Into<String>, stage: impl Into<String>) -> StageTimer<'_> {
        StageTimer {
            tailtriage: self,
            request_id: request_id.into(),
            stage: stage.into(),
        }
    }

    /// Returns a queue timing wrapper for one awaited operation.
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
        lock_run(&self.run).runtime_snapshots.push(snapshot);
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
