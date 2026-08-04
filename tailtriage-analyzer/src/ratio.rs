/// Returns whether `higher / lower` meets the configured `numerator / denominator`.
///
/// Analyzer option validation owns nonzero ratio components. This helper owns
/// the shared zero-baseline and overflow behavior used by scoped movement
/// policies; callers retain ownership of which values are compared.
pub(super) fn meets_ratio(higher: u64, lower: u64, numerator: u64, denominator: u64) -> bool {
    lower > 0 && higher.saturating_mul(denominator) >= lower.saturating_mul(numerator)
}

#[cfg(test)]
mod tests {
    use super::meets_ratio;

    #[test]
    fn ratio_boundary_is_inclusive_and_distinguishes_adjacent_values() {
        assert!(!meets_ratio(2, 2, 3, 2));
        assert!(meets_ratio(3, 2, 3, 2));
        assert!(meets_ratio(4, 2, 3, 2));
    }

    #[test]
    fn zero_baseline_never_meets_ratio() {
        assert!(!meets_ratio(u64::MAX, 0, 3, 2));
    }

    #[test]
    fn ratio_comparison_preserves_saturating_overflow_behavior() {
        assert!(meets_ratio(u64::MAX, u64::MAX, 3, 2));
        assert!(meets_ratio(u64::MAX - 1, u64::MAX, 3, 2));
    }
}
