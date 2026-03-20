use std::time::Instant;

use crate::collector::{duration_to_us, lock_map, lock_run};
use crate::{unix_time_ms, InFlightSnapshot, QueueEvent, StageEvent, Tailtriage};

/// RAII guard tracking one in-flight unit for a named gauge.
#[derive(Debug)]
pub struct InflightGuard<'a> {
    pub(crate) tailtriage: &'a Tailtriage,
    pub(crate) gauge: String,
}

impl Drop for InflightGuard<'_> {
    fn drop(&mut self) {
        let count = {
            let mut counts = lock_map(&self.tailtriage.inflight_counts);
            let entry = counts.entry(self.gauge.clone()).or_insert(0);
            if *entry > 0 {
                *entry -= 1;
            }
            *entry
        };

        lock_run(&self.tailtriage.run)
            .inflight
            .push(InFlightSnapshot {
                gauge: self.gauge.clone(),
                at_unix_ms: unix_time_ms(),
                count,
            });
    }
}

/// Thin wrapper for recording stage latency around one await point.
#[derive(Debug)]
pub struct StageTimer<'a> {
    pub(crate) tailtriage: &'a Tailtriage,
    pub(crate) request_id: String,
    pub(crate) stage: String,
}

impl StageTimer<'_> {
    /// Awaits `fut`, records stage duration, and returns the original output.
    ///
    /// This helper is intended for fallible stage work where success can be
    /// derived from `Result::is_ok`.
    ///
    /// # Errors
    ///
    /// Returns the same error value produced by `fut` after recording the
    /// stage event with `success = false`.
    pub async fn await_on<Fut, T, E>(self, fut: Fut) -> Result<T, E>
    where
        Fut: std::future::Future<Output = Result<T, E>>,
    {
        let started_at_unix_ms = unix_time_ms();
        let started = Instant::now();
        let value = fut.await;
        let finished_at_unix_ms = unix_time_ms();
        let success = value.is_ok();

        lock_run(&self.tailtriage.run).stages.push(StageEvent {
            request_id: self.request_id,
            stage: self.stage,
            started_at_unix_ms,
            finished_at_unix_ms,
            latency_us: duration_to_us(started.elapsed()),
            success,
        });

        value
    }

    /// Awaits an infallible stage future and records a successful stage event.
    pub async fn await_value<Fut, T>(self, fut: Fut) -> T
    where
        Fut: std::future::Future<Output = T>,
    {
        let started_at_unix_ms = unix_time_ms();
        let started = Instant::now();
        let value = fut.await;
        let finished_at_unix_ms = unix_time_ms();

        lock_run(&self.tailtriage.run).stages.push(StageEvent {
            request_id: self.request_id,
            stage: self.stage,
            started_at_unix_ms,
            finished_at_unix_ms,
            latency_us: duration_to_us(started.elapsed()),
            success: true,
        });

        value
    }
}

/// Thin wrapper for recording queue-wait latency around one await point.
#[derive(Debug)]
pub struct QueueTimer<'a> {
    pub(crate) tailtriage: &'a Tailtriage,
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
    pub async fn await_on<Fut, T>(self, fut: Fut) -> T
    where
        Fut: std::future::Future<Output = T>,
    {
        let waited_from_unix_ms = unix_time_ms();
        let started = Instant::now();
        let value = fut.await;
        let waited_until_unix_ms = unix_time_ms();

        lock_run(&self.tailtriage.run).queues.push(QueueEvent {
            request_id: self.request_id,
            queue: self.queue,
            waited_from_unix_ms,
            waited_until_unix_ms,
            wait_us: duration_to_us(started.elapsed()),
            depth_at_start: self.depth_at_start,
        });

        value
    }
}
