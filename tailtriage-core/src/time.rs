use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Converts a [`Duration`] since [`UNIX_EPOCH`] to unix epoch milliseconds,
/// saturating at [`u64::MAX`].
#[must_use]
fn duration_to_unix_ms(duration: Duration) -> u64 {
    duration.as_millis().try_into().unwrap_or(u64::MAX)
}

/// Monotonic and wall-clock start sample for one measured interval.
#[derive(Debug, Clone, Copy)]
pub(crate) struct IntervalStart {
    pub(crate) started_at_unix_ms: u64,
    pub(crate) started: Instant,
}

/// Completed interval timing derived from a start sample.
#[derive(Debug, Clone, Copy)]
pub(crate) struct FinishedInterval {
    pub(crate) started_at_unix_ms: u64,
    pub(crate) finished_at_unix_ms: u64,
    pub(crate) duration_us: u64,
}

/// Starts a measured interval using a wall-clock sample and monotonic instant.
#[must_use]
pub(crate) fn start_interval() -> IntervalStart {
    let started_at_unix_ms = unix_time_ms();
    let started = Instant::now();

    IntervalStart {
        started_at_unix_ms,
        started,
    }
}

/// Finishes a measured interval using wall-clock finish time and monotonic duration.
#[must_use]
pub(crate) fn finish_interval(start: IntervalStart) -> FinishedInterval {
    let finished = Instant::now();
    let finished_at_unix_ms = unix_time_ms();

    FinishedInterval {
        started_at_unix_ms: start.started_at_unix_ms,
        finished_at_unix_ms,
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
    fn duration_to_us_saturates_at_u64_max() {
        assert_eq!(duration_to_us(Duration::from_micros(123)), 123);

        let overflow_duration = Duration::from_micros(u64::MAX)
            .checked_add(Duration::from_micros(1))
            .expect("overflow test duration should be representable");
        assert_eq!(duration_to_us(overflow_duration), u64::MAX);
    }

    #[test]
    fn finish_interval_preserves_timestamp_ordering() {
        let start = start_interval();
        std::thread::sleep(Duration::from_millis(1));

        let finished = finish_interval(start);

        assert_eq!(finished.started_at_unix_ms, start.started_at_unix_ms);
        assert!(finished.finished_at_unix_ms >= finished.started_at_unix_ms);
        assert!(finished.duration_us > 0);
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
