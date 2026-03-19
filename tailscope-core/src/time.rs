use std::time::{SystemTime, UNIX_EPOCH};

/// Converts a [`SystemTime`] to unix epoch milliseconds.
///
/// Values before [`UNIX_EPOCH`] are clamped to `0`. Values larger than
/// [`u64::MAX`] milliseconds are saturated at `u64::MAX`.
#[must_use]
pub fn system_time_to_unix_ms(time: SystemTime) -> u64 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_millis().try_into().unwrap_or(u64::MAX),
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
    use super::system_time_to_unix_ms;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    #[test]
    fn system_time_to_unix_ms_clamps_epoch_and_overflow() {
        let before_epoch = UNIX_EPOCH
            .checked_sub(Duration::from_millis(1))
            .expect("one millisecond before epoch should be representable");
        assert_eq!(system_time_to_unix_ms(before_epoch), 0);

        let overflow_duration = Duration::from_millis(u64::MAX)
            .checked_add(Duration::from_millis(1))
            .expect("overflow test duration should be representable");
        let overflow = UNIX_EPOCH
            .checked_add(overflow_duration)
            .expect("overflow test timestamp should be representable");
        assert_eq!(system_time_to_unix_ms(overflow), u64::MAX);

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
