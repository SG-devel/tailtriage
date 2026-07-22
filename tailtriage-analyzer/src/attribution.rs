#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct AttributionInput {
    pub(super) interval: Option<(u64, u64)>,
    pub(super) duration_us: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct AttributedDuration {
    pub(super) duration_us: u64,
    pub(super) is_precise: bool,
}

pub(super) fn attributed_elapsed_duration(
    events: &[AttributionInput],
    cap_us: u64,
) -> AttributedDuration {
    if events.is_empty() || cap_us == 0 {
        return AttributedDuration {
            duration_us: 0,
            is_precise: true,
        };
    }

    if events.iter().any(|event| event.interval.is_none()) {
        let duration_us = events
            .iter()
            .fold(0_u64, |sum, event| sum.saturating_add(event.duration_us))
            .min(cap_us);
        return AttributedDuration {
            duration_us,
            is_precise: false,
        };
    }

    let mut intervals = events
        .iter()
        .map(|event| event.interval.expect("checked complete intervals"))
        .collect::<Vec<_>>();
    intervals.sort_unstable_by_key(|&(start, end)| (start, end));

    let mut covered = 0_u64;
    let mut merged: Option<(u64, u64)> = None;
    for (start, end) in intervals {
        debug_assert!(start <= end, "normalized intervals must not be inverted");
        match merged {
            None => merged = Some((start, end)),
            Some((merged_start, merged_end)) if start <= merged_end => {
                merged = Some((merged_start, merged_end.max(end)));
            }
            Some((merged_start, merged_end)) => {
                covered = covered.saturating_add(merged_end.saturating_sub(merged_start));
                merged = Some((start, end));
            }
        }
    }
    if let Some((start, end)) = merged {
        covered = covered.saturating_add(end.saturating_sub(start));
    }

    AttributedDuration {
        duration_us: covered.min(cap_us),
        is_precise: true,
    }
}

#[cfg(test)]
mod tests {
    use super::{attributed_elapsed_duration, AttributionInput};

    fn precise_duration(intervals: &[(u64, u64)], cap_us: u64) -> u64 {
        let events = intervals
            .iter()
            .map(|&(start, end)| AttributionInput {
                interval: Some((start, end)),
                duration_us: end - start,
            })
            .collect::<Vec<_>>();
        let attributed = attributed_elapsed_duration(&events, cap_us);
        assert!(attributed.is_precise);
        attributed.duration_us
    }

    #[test]
    fn empty_intervals_are_precise_zero() {
        let attributed = attributed_elapsed_duration(&[], 100);
        assert!(attributed.is_precise);
        assert_eq!(attributed.duration_us, 0);
    }

    #[test]
    fn one_interval() {
        assert_eq!(precise_duration(&[(10, 40)], 100), 30);
    }

    #[test]
    fn disjoint_intervals_sum_covered_time() {
        assert_eq!(precise_duration(&[(0, 20), (40, 70)], 100), 50);
    }

    #[test]
    fn touching_intervals_are_merged() {
        assert_eq!(precise_duration(&[(0, 20), (20, 50)], 100), 50);
    }

    #[test]
    fn nested_intervals_do_not_inflate_time() {
        assert_eq!(precise_duration(&[(0, 80), (20, 40)], 100), 80);
    }

    #[test]
    fn partially_overlapping_intervals_are_union_attributed() {
        assert_eq!(precise_duration(&[(0, 60), (40, 90)], 100), 90);
    }

    #[test]
    fn exact_duplicate_intervals_count_once() {
        assert_eq!(precise_duration(&[(0, 60), (0, 60)], 100), 60);
    }

    #[test]
    fn unsorted_input_is_sorted_before_union() {
        assert_eq!(precise_duration(&[(50, 90), (0, 20), (15, 30)], 100), 70);
    }

    #[test]
    fn zero_length_intervals_do_not_add_time() {
        assert_eq!(precise_duration(&[(0, 0), (10, 10), (20, 30)], 100), 10);
    }
}
