use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Converts a [`Duration`] since [`UNIX_EPOCH`] to unix epoch milliseconds,
/// saturating at [`u64::MAX`].
#[must_use]
fn duration_to_unix_ms(duration: Duration) -> u64 {
    duration.as_millis().try_into().unwrap_or(u64::MAX)
}

/// Per-run monotonic clock paired with a wall-clock run-start anchor.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RunClock {
    run_started: Instant,
    run_started_at_unix_ms: u64,
}

/// One sampled point from a [`RunClock`].
#[derive(Debug, Clone, Copy)]
pub(crate) struct RunClockSample {
    pub(crate) unix_ms: u64,
    pub(crate) run_elapsed_us: u64,
    pub(crate) instant: Instant,
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

impl RunClock {
    /// Creates a run clock anchored to the current wall-clock and monotonic time.
    #[must_use]
    pub(crate) fn new() -> Self {
        Self {
            run_started: Instant::now(),
            run_started_at_unix_ms: unix_time_ms(),
        }
    }

    /// Samples the current wall-clock time and monotonic run-relative offset.
    #[must_use]
    pub(crate) fn sample(&self) -> RunClockSample {
        let instant = Instant::now();
        RunClockSample {
            unix_ms: unix_time_ms(),
            run_elapsed_us: duration_to_us(instant.duration_since(self.run_started)),
            instant,
        }
    }

    /// Starts a measured interval against this run clock.
    #[must_use]
    pub(crate) fn start_interval(&self) -> IntervalStart {
        let sample = self.sample();
        IntervalStart {
            started_at_unix_ms: sample.unix_ms,
            started_at_run_us: Some(sample.run_elapsed_us),
            started: sample.instant,
        }
    }

    /// Finishes a measured interval against this run clock.
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

    /// Returns the wall-clock run-start anchor.
    #[must_use]
    pub(crate) const fn run_started_at_unix_ms(&self) -> u64 {
        self.run_started_at_unix_ms
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
/// [`u64::MAX`] milliseconds are saturated at `u64::MAX`.
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

#[cfg(test)]
mod tests {
    use super::{duration_to_unix_ms, duration_to_us, system_time_to_unix_ms, RunClock};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    #[test]
    fn duration_to_us_saturates_at_u64_max() {
        assert_eq!(duration_to_us(Duration::from_micros(123)), 123);

        let overflow_duration = Duration::from_micros(u64::MAX)
            .checked_add(Duration::from_micros(1))
            .expect("overflow test duration should be representable");
        assert_eq!(duration_to_us(overflow_duration), u64::MAX);
    }

    #[test]
    fn run_clock_intervals_include_run_relative_offsets() {
        let clock = RunClock::new();
        let start = clock.start_interval();
        std::thread::sleep(Duration::from_millis(1));

        let finished = clock.finish_interval(start);

        assert_eq!(finished.started_at_run_us, start.started_at_run_us);
        assert!(finished.started_at_run_us.is_some());
        assert!(finished.finished_at_run_us.is_some());
        assert!(finished.finished_at_run_us >= finished.started_at_run_us);
        assert!(finished.finished_at_unix_ms >= finished.started_at_unix_ms);
        assert!(finished.duration_us > 0);
        assert!(clock.run_started_at_unix_ms() <= finished.finished_at_unix_ms);
    }

    #[test]
    fn system_time_to_unix_ms_clamps_epoch_and_overflow() {
        let before_epoch = UNIX_EPOCH
            .checked_sub(Duration::from_millis(1))
            .expect("one millisecond before epoch should be representable");
        assert_eq!(system_time_to_unix_ms(before_epoch), 0);

        let overflow_duration = Duration::from_millis(u64::MAX)
            .checked_add(Duration::from_millis(1))
            .expect("overflow test duration should be representable");
        assert_eq!(duration_to_unix_ms(overflow_duration), u64::MAX);

        assert_eq!(system_time_to_unix_ms(UNIX_EPOCH), 0);
        assert_eq!(
            system_time_to_unix_ms(UNIX_EPOCH + Duration::from_millis(123)),
            123
        );
        assert_eq!(
            system_time_to_unix_ms(SystemTime::UNIX_EPOCH + Duration::from_secs(1)),
            1_000
        );
    }
}
