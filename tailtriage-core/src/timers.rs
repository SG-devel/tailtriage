use std::panic::{catch_unwind, AssertUnwindSafe};

use crate::collector::{lock_state, CollectorPhase};
use crate::time::IntervalStart;
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

        let sample = self.tailtriage.run_clock.sample();
        let notify_limits_hit = {
            let mut state = lock_state(&self.tailtriage.state.mutex);
            if !matches!(state.phase, CollectorPhase::Open) {
                return;
            }
            let entry = state.inflight_counts.entry(self.gauge.clone()).or_insert(0);
            if *entry > 0 {
                *entry -= 1;
            }
            let count = *entry;
            if crate::retention::push_inflight_snapshot_bounded(
                &mut state.run,
                self.tailtriage.limits,
                InFlightSnapshot {
                    gauge: self.gauge.clone(),
                    at_unix_ms: sample.unix_ms,
                    at_run_us: Some(sample.run_elapsed_us),
                    count,
                },
            ) {
                self.tailtriage.truncation_state.inflight.mark_saturated();
                self.tailtriage
                    .truncation_state
                    .mark_run_limits_hit(&mut state.run)
            } else {
                false
            }
        };
        if notify_limits_hit {
            self.tailtriage.notify_limits_hit_listener();
        }
    }
}

#[derive(Debug)]
enum ArmedTimerRecord<'a> {
    Stage {
        tailtriage: &'a Tailtriage,
        request_id: String,
        stage: String,
        interval_start: Option<IntervalStart>,
    },
    Queue {
        tailtriage: &'a Tailtriage,
        request_id: String,
        queue: String,
        depth_at_start: Option<u64>,
        interval_start: Option<IntervalStart>,
    },
}

impl ArmedTimerRecord<'_> {
    fn disarm(&mut self) -> Option<IntervalStart> {
        match self {
            Self::Stage { interval_start, .. } | Self::Queue { interval_start, .. } => {
                interval_start.take()
            }
        }
    }
}

impl Drop for ArmedTimerRecord<'_> {
    fn drop(&mut self) {
        let _ = catch_unwind(AssertUnwindSafe(|| match self {
            Self::Stage {
                tailtriage,
                request_id,
                stage,
                interval_start,
            } => {
                if let Some(start) = interval_start.take() {
                    let finished = tailtriage.run_clock.finish_interval(start);
                    tailtriage.record_stage_event(
                        StageEvent::new(
                            request_id.clone(),
                            stage.clone(),
                            finished.started_at_unix_ms,
                            finished.finished_at_unix_ms,
                            finished.duration_us,
                            false,
                        )
                        .with_run_interval(finished.started_at_run_us, finished.finished_at_run_us)
                        .into_partial(),
                    );
                }
            }
            Self::Queue {
                tailtriage,
                request_id,
                queue,
                depth_at_start,
                interval_start,
            } => {
                if let Some(start) = interval_start.take() {
                    let finished = tailtriage.run_clock.finish_interval(start);
                    let mut event = QueueEvent::new(
                        request_id.clone(),
                        queue.clone(),
                        finished.started_at_unix_ms,
                        finished.finished_at_unix_ms,
                        finished.duration_us,
                    )
                    .with_run_interval(finished.started_at_run_us, finished.finished_at_run_us)
                    .into_partial();
                    event.depth_at_start = *depth_at_start;
                    tailtriage.record_queue_event(event);
                }
            }
        }));
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
    /// Timing begins on first poll. A future dropped before first poll records
    /// no evidence. A polled helper future dropped before normal readiness
    /// records one bounded partial event ending at observed Drop; this does not
    /// prove the underlying operation stopped. Partial stage events have
    /// `completed = false` and `success = false`.
    ///
    /// # Errors
    ///
    /// Returns the same error value produced by `fut` after recording the
    /// stage event with `success = false` for completed errors.
    pub async fn await_on<Fut, T, E>(self, fut: Fut) -> Result<T, E>
    where
        Fut: std::future::Future<Output = Result<T, E>>,
    {
        if !self.enabled {
            return fut.await;
        }

        let mut guard = ArmedTimerRecord::Stage {
            tailtriage: self.tailtriage,
            request_id: self.request_id.clone(),
            stage: self.stage.clone(),
            interval_start: Some(self.tailtriage.run_clock.start_interval()),
        };
        let value = fut.await;
        if let Some(interval_start) = guard.disarm() {
            let finished = self.tailtriage.run_clock.finish_interval(interval_start);
            let success = value.is_ok();
            self.tailtriage.record_stage_event(
                StageEvent::new(
                    self.request_id,
                    self.stage,
                    finished.started_at_unix_ms,
                    finished.finished_at_unix_ms,
                    finished.duration_us,
                    success,
                )
                .with_run_interval(finished.started_at_run_us, finished.finished_at_run_us),
            );
        }
        value
    }

    /// Awaits an infallible stage future and records a successful stage event.
    ///
    /// Timing begins on first poll. A future dropped before first poll records
    /// no evidence. A polled helper future dropped before normal readiness
    /// records one bounded partial event ending at observed Drop.
    pub async fn await_value<Fut, T>(self, fut: Fut) -> T
    where
        Fut: std::future::Future<Output = T>,
    {
        if !self.enabled {
            return fut.await;
        }

        let mut guard = ArmedTimerRecord::Stage {
            tailtriage: self.tailtriage,
            request_id: self.request_id.clone(),
            stage: self.stage.clone(),
            interval_start: Some(self.tailtriage.run_clock.start_interval()),
        };
        let value = fut.await;
        if let Some(interval_start) = guard.disarm() {
            let finished = self.tailtriage.run_clock.finish_interval(interval_start);
            self.tailtriage.record_stage_event(
                StageEvent::new(
                    self.request_id,
                    self.stage,
                    finished.started_at_unix_ms,
                    finished.finished_at_unix_ms,
                    finished.duration_us,
                    true,
                )
                .with_run_interval(finished.started_at_run_us, finished.finished_at_run_us),
            );
        }
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
    /// Timing begins on first poll. A future dropped before first poll records
    /// no evidence. A polled helper future dropped before normal readiness
    /// records one bounded partial wait event ending at observed Drop; this does
    /// not prove the underlying operation stopped.
    pub async fn await_on<Fut, T>(self, fut: Fut) -> T
    where
        Fut: std::future::Future<Output = T>,
    {
        if !self.enabled {
            return fut.await;
        }

        let mut guard = ArmedTimerRecord::Queue {
            tailtriage: self.tailtriage,
            request_id: self.request_id.clone(),
            queue: self.queue.clone(),
            depth_at_start: self.depth_at_start,
            interval_start: Some(self.tailtriage.run_clock.start_interval()),
        };
        let value = fut.await;
        if let Some(interval_start) = guard.disarm() {
            let finished = self.tailtriage.run_clock.finish_interval(interval_start);
            let mut event = QueueEvent::new(
                self.request_id,
                self.queue,
                finished.started_at_unix_ms,
                finished.finished_at_unix_ms,
                finished.duration_us,
            )
            .with_run_interval(finished.started_at_run_us, finished.finished_at_run_us);
            event.depth_at_start = self.depth_at_start;
            self.tailtriage.record_queue_event(event);
        }
        value
    }
}
