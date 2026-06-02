use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Converts a [`Duration`] since [`UNIX_EPOCH`] to unix epoch milliseconds,
/// saturating at [`u64::MAX`].
#[must_use]
fn duration_to_unix_ms(duration: Duration) -> u64 {
    duration.as_millis().try_into().unwrap_or(u64::MAX)
}

/// Per-run monotonic clock used to derive run-relative timestamps.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RunClock {
    run_start: Instant,
    run_start_unix_ms: u64,
}

/// A single wall-clock and run-relative monotonic sample.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RunClockSample {
    pub(crate) unix_ms: u64,
    pub(crate) run_elapsed_us: u64,
    pub(crate) instant: Instant,
}

impl RunClock {
    /// Starts a new run clock from the current wall-clock and monotonic time.
    #[must_use]
    pub(crate) fn new() -> Self {
        Self {
            run_start: Instant::now(),
            run_start_unix_ms: unix_time_ms(),
        }
    }

    /// Samples wall-clock and monotonic elapsed time for this run.
    #[must_use]
    pub(crate) fn sample(&self) -> RunClockSample {
        let instant = Instant::now();
        RunClockSample {
            unix_ms: unix_time_ms(),
            run_elapsed_us: duration_to_us(instant.duration_since(self.run_start)),
            instant,
        }
    }

    /// Starts a measured interval using this run's clock.
    #[must_use]
    pub(crate) fn start_interval(&self) -> IntervalStart {
        let sample = self.sample();
        IntervalStart {
            started_at_unix_ms: sample.unix_ms,
            started_at_run_us: Some(sample.run_elapsed_us),
            started: sample.instant,
        }
    }

    /// Finishes a measured interval using this run's clock.
    #[must_use]
    pub(crate) fn finish_interval(&self, start: IntervalStart) -> FinishedInterval {
        let sample = self.sample();
        FinishedInterval {
            started_at_unix_ms: start.started_at_unix_ms,
            started_at_run_us: start.started_at_run_us,
            finished_at_unix_ms: sample.unix_ms,
            finished_at_run_us: Some(sample.run_elapsed_us),
            duration_us: duration_to_us(sample.instant.duration_since(start.started)),
        }
    }

    /// Returns the unix timestamp captured when this run clock was created.
    #[must_use]
    pub(crate) const fn run_start_unix_ms(&self) -> u64 {
        self.run_start_unix_ms
    }
}

/// Monotonic and wall-clock start sample for one measured interval.
#[derive(Debug, Clone, Copy)]
pub(crate) struct IntervalStart {
    pub(crate) started_at_unix_ms: u64,
    pub(crate) started_at_run_us: Option<u64>,
    pub(crate) started: Instant,
}

/// Completed interval timing derived from a start sample.
#[derive(Debug, Clone, Copy)]
pub(crate) struct FinishedInterval {
    pub(crate) started_at_unix_ms: u64,
    pub(crate) started_at_run_us: Option<u64>,
    pub(crate) finished_at_unix_ms: u64,
    pub(crate) finished_at_run_us: Option<u64>,
    pub(crate) duration_us: u64,
}

/// Starts a measured interval using a wall-clock sample and monotonic instant.
#[allow(dead_code)]
#[must_use]
pub(crate) fn start_interval() -> IntervalStart {
    let started_at_unix_ms = unix_time_ms();
    let started = Instant::now();

    IntervalStart {
        started_at_unix_ms,
        started_at_run_us: None,
        started,
    }
}

/// Finishes a measured interval using wall-clock finish time and monotonic duration.
#[allow(dead_code)]
#[must_use]
pub(crate) fn finish_interval(start: IntervalStart) -> FinishedInterval {
    let finished = Instant::now();
    let finished_at_unix_ms = unix_time_ms();

    FinishedInterval {
        started_at_unix_ms: start.started_at_unix_ms,
        started_at_run_us: start.started_at_run_us,
        finished_at_unix_ms,
        finished_at_run_us: None,
        duration_us: duration_to_us(finished.duration_since(start.started)),
    }
}

/// Converts a [`Duration`] to microseconds, saturating at [`u64::MAX`].
#[must_use]
pub(crate) fn duration_to_us(duration: Duration) -> u64 {
    duration.as_micros().try_into().unwrap_or(u64::MAX)
}

/// Converts a [`SystemTime`] to unix epoch milliseconds.
///
/// Values before [`UNIX_EPOCH`] are clamped to `0`. Values larger than
/// [`u64::MAX`] milliseconds are saturated at [`u64::MAX`].
#[must_use]
pub fn system_time_to_unix_ms(time: SystemTime) -> u64 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(duration) => duration_to_unix_ms(duration),
        Err(_) => 0,
    }
}

/// Returns the current unix epoch timestamp in milliseconds.
#[must_use]
pub fn unix_time_ms() -> u64 {
    system_time_to_unix_ms(SystemTime::now())
}
