/// Returns whether `higher / lower` meets the configured `numerator / denominator`.
///
/// Analyzer option validation owns nonzero ratio components. This helper owns
/// the shared inclusive boundary, zero-baseline rejection, and exact
/// cross-multiplication used by scoped movement policies; callers retain
/// ownership of which values are compared.
pub(super) fn meets_ratio(higher: u64, lower: u64, numerator: u64, denominator: u64) -> bool {
    lower > 0
        && u128::from(higher) * u128::from(denominator) >= u128::from(lower) * u128::from(numerator)
}

#[cfg(test)]
mod tests {
    use super::meets_ratio;

    // TT-TEST: support
    #[test]
    fn ratio_boundary_is_inclusive_and_distinguishes_adjacent_values() {
        assert!(!meets_ratio(2, 2, 3, 2));
        assert!(meets_ratio(3, 2, 3, 2));
        assert!(meets_ratio(4, 2, 3, 2));
    }

    // TT-TEST: support
    #[test]
    fn zero_baseline_never_meets_ratio() {
        assert!(!meets_ratio(u64::MAX, 0, 3, 2));
    }

    // TT-TEST: support
    #[test]
    fn large_qualifying_and_non_qualifying_values_are_exact() {
        assert!(meets_ratio(u64::MAX, u64::MAX / 2, 3, 2));
        assert!(!meets_ratio(u64::MAX / 2, u64::MAX, 3, 2));
        assert!(!meets_ratio(u64::MAX - 1, u64::MAX, 3, 2));
    }

    // TT-TEST: support
    #[test]
    fn arithmetic_is_exact_near_u64_max() {
        assert!(meets_ratio(u64::MAX, u64::MAX - 1, 1, 1));
        assert!(!meets_ratio(u64::MAX - 1, u64::MAX, 1, 1));
    }
}
