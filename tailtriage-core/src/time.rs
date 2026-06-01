use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Converts a [`Duration`] since [`UNIX_EPOCH`] to unix epoch milliseconds,
/// saturating at [`u64::MAX`].
#[must_use]
fn duration_to_unix_ms(duration: Duration) -> u64 {
    duration.as_millis().try_into().unwrap_or(u64::MAX)
}

/// Start timestamp samples for an interval measured with both wall-clock and monotonic time.
#[derive(Debug, Clone, Copy)]
pub(crate) struct IntervalStart {
    pub(crate) started_at_unix_ms: u64,
    pub(crate) started: Instant,
}

/// Completed timestamp samples for an interval.
#[derive(Debug, Clone, Copy)]
pub(crate) struct FinishedInterval {
    pub(crate) started_at_unix_ms: u64,
    pub(crate) finished_at_unix_ms: u64,
    pub(crate) duration_us: u64,
}

/// Converts a [`Duration`] to microseconds, saturating at [`u64::MAX`].
#[must_use]
pub(crate) fn duration_to_us(duration: Duration) -> u64 {
    duration.as_micros().try_into().unwrap_or(u64::MAX)
}

/// Starts an interval, sampling wall time before monotonic time.
#[must_use]
pub(crate) fn start_interval() -> IntervalStart {
    IntervalStart {
        started_at_unix_ms: unix_time_ms(),
        started: Instant::now(),
    }
}

/// Finishes an interval, sampling wall time and computing duration from monotonic time.
#[must_use]
pub(crate) fn finish_interval(start: IntervalStart) -> FinishedInterval {
    FinishedInterval {
        started_at_unix_ms: start.started_at_unix_ms,
        finished_at_unix_ms: unix_time_ms(),
        duration_us: duration_to_us(start.started.elapsed()),
    }
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
    use super::{
        duration_to_unix_ms, duration_to_us, finish_interval, start_interval,
        system_time_to_unix_ms,
    };
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    #[test]
    fn duration_to_us_saturates_on_overflow() {
        assert_eq!(duration_to_us(Duration::from_micros(123)), 123);

        let overflow_duration = Duration::from_micros(u64::MAX)
            .checked_add(Duration::from_micros(1))
            .expect("overflow test duration should be representable");
        assert_eq!(duration_to_us(overflow_duration), u64::MAX);
    }

    #[test]
    fn finish_interval_preserves_timestamp_ordering() {
        let started = start_interval();
        let finished = finish_interval(started);

        assert!(finished.finished_at_unix_ms >= finished.started_at_unix_ms);
        assert!(i128::from(finished.duration_us) >= 0);
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
