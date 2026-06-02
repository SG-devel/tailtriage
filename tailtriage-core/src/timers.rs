use crate::collector::lock_map;
use crate::{InFlightSnapshot, QueueEvent, StageEvent, Tailtriage};

/// RAII guard tracking one in-flight unit for a named gauge.
#[derive(Debug)]
pub struct InflightGuard<'a> {
    pub(crate) tailtriage: &'a Tailtriage,
    pub(crate) gauge: String,
    pub(crate) enabled: bool,
}

impl Drop for InflightGuard<'_> {
    fn drop(&mut self) {
        if !self.enabled {
            return;
        }

        let count = {
            let mut counts = lock_map(&self.tailtriage.inflight_counts);
            let entry = counts.entry(self.gauge.clone()).or_insert(0);
            if *entry > 0 {
                *entry -= 1;
            }
            *entry
        };

        let sample = self.tailtriage.clock.sample();
        self.tailtriage.record_inflight_snapshot(InFlightSnapshot {
            gauge: self.gauge.clone(),
            at_unix_ms: sample.unix_ms,
            at_run_us: Some(sample.run_elapsed_us),
            count,
        });
    }
}

/// Thin wrapper for recording stage latency around one await point.
#[derive(Debug)]
pub struct StageTimer<'a> {
    pub(crate) tailtriage: &'a Tailtriage,
    pub(crate) enabled: bool,
    pub(crate) request_id: String,
    pub(crate) stage: String,
}

impl StageTimer<'_> {
    /// Awaits `fut`, records stage duration, and returns the original output.
    ///
    /// This helper is intended for fallible stage work where success can be
    /// derived from `Result::is_ok`.
    ///
    /// Prefer this method when your stage naturally returns `Result<T, E>` and
    /// you want success/failure evidence in the resulting triage report.
    ///
    /// # Errors
    ///
    /// Returns the same error value produced by `fut` after recording the
    /// stage event with `success = false`.
    pub async fn await_on<Fut, T, E>(self, fut: Fut) -> Result<T, E>
    where
        Fut: std::future::Future<Output = Result<T, E>>,
    {
        if !self.enabled {
            return fut.await;
        }

        let interval_start = self.tailtriage.clock.start_interval();
        let value = fut.await;
        let finished = self.tailtriage.clock.finish_interval(interval_start);
        let success = value.is_ok();

        self.tailtriage.record_stage_event(StageEvent {
            request_id: self.request_id,
            stage: self.stage,
            started_at_unix_ms: finished.started_at_unix_ms,
            started_at_run_us: finished.started_at_run_us,
            finished_at_unix_ms: finished.finished_at_unix_ms,
            finished_at_run_us: finished.finished_at_run_us,
            latency_us: finished.duration_us,
            success,
        });

        value
    }

    /// Awaits an infallible stage future and records a successful stage event.
    ///
    /// Use this method when there is no meaningful stage-level error signal
    /// (for example, internal CPU work or a prevalidated transformation).
    pub async fn await_value<Fut, T>(self, fut: Fut) -> T
    where
        Fut: std::future::Future<Output = T>,
    {
        if !self.enabled {
            return fut.await;
        }

        let interval_start = self.tailtriage.clock.start_interval();
        let value = fut.await;
        let finished = self.tailtriage.clock.finish_interval(interval_start);

        self.tailtriage.record_stage_event(StageEvent {
            request_id: self.request_id,
            stage: self.stage,
            started_at_unix_ms: finished.started_at_unix_ms,
            started_at_run_us: finished.started_at_run_us,
            finished_at_unix_ms: finished.finished_at_unix_ms,
            finished_at_run_us: finished.finished_at_run_us,
            latency_us: finished.duration_us,
            success: true,
        });

        value
    }
}

/// Thin wrapper for recording queue-wait latency around one await point.
#[derive(Debug)]
pub struct QueueTimer<'a> {
    pub(crate) tailtriage: &'a Tailtriage,
    pub(crate) enabled: bool,
    pub(crate) request_id: String,
    pub(crate) queue: String,
    pub(crate) depth_at_start: Option<u64>,
}

impl QueueTimer<'_> {
    /// Sets the queue depth sample captured at wait start.
    #[must_use]
    pub fn with_depth_at_start(mut self, depth_at_start: u64) -> Self {
        self.depth_at_start = Some(depth_at_start);
        self
    }

    /// Awaits `fut`, records queue wait duration, and returns the original output.
    ///
    /// Queue events are interpreted as application-level wait evidence (a lead,
    /// not proof). Record these around bounded resources to help separate
    /// queueing pressure from slow downstream stage time.
    pub async fn await_on<Fut, T>(self, fut: Fut) -> T
    where
        Fut: std::future::Future<Output = T>,
    {
        if !self.enabled {
            return fut.await;
        }

        let interval_start = self.tailtriage.clock.start_interval();
        let value = fut.await;
        let finished = self.tailtriage.clock.finish_interval(interval_start);

        self.tailtriage.record_queue_event(QueueEvent {
            request_id: self.request_id,
            queue: self.queue,
            waited_from_unix_ms: finished.started_at_unix_ms,
            waited_from_run_us: finished.started_at_run_us,
            waited_until_unix_ms: finished.finished_at_unix_ms,
            waited_until_run_us: finished.finished_at_run_us,
            wait_us: finished.duration_us,
            depth_at_start: self.depth_at_start,
        });

        value
    }
}
