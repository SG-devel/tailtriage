use super::{AnalyzeConfigError, AnalyzeOptions};

const VALID_OVERRIDE_PATHS: &[&str] = &[
    "queueing.trigger_permille",
    "blocking.min_nonzero_samples_for_signal",
    "blocking.strong_p95_threshold",
    "blocking.strong_peak_threshold",
    "blocking.strong_nonzero_share_permille",
    "blocking.strong_min_samples",
    "executor.min_global_queue_p95_for_signal",
    "downstream.min_stage_samples",
    "downstream.blocking_correlated_stage_patterns",
    "downstream.blocking_correlation_score_margin",
    "confidence.medium_score_threshold",
    "confidence.high_score_threshold",
    "confidence.ambiguity_min_score",
    "confidence.ambiguity_score_gap",
    "evidence.low_completed_request_threshold",
    "route.min_request_count",
    "route.breakdown_limit",
    "route.emit_on_divergent_suspects",
    "route.slowest_to_fastest_p95_ratio_numerator",
    "route.slowest_to_fastest_p95_ratio_denominator",
    "route.slowest_to_global_p95_ratio_numerator",
    "route.slowest_to_global_p95_ratio_denominator",
    "temporal.min_request_count",
    "temporal.min_segment_request_count",
    "temporal.share_shift_permille",
    "temporal.p95_shift_ratio_numerator",
    "temporal.p95_shift_ratio_denominator",
    "temporal.emit_on_suspect_shift",
    "temporal.suppress_runtime_sparse_suspect_shift_without_supporting_movement",
];

impl AnalyzeOptions {
    /// Returns all supported `group.field` override paths for manual analyzer overrides.
    #[must_use]
    pub fn valid_override_paths() -> &'static [&'static str] {
        VALID_OVERRIDE_PATHS
    }

    /// Applies one `group.field=value` override and validates the full options after assignment.
    ///
    /// # Errors
    /// Returns [`AnalyzeConfigError`] if syntax, path lookup, type parsing, or post-apply validation fails.
    #[allow(clippy::too_many_lines)]
    pub fn apply_override(&mut self, raw: &str) -> Result<(), AnalyzeConfigError> {
        let Some((path, value)) = raw.split_once('=') else {
            return Err(AnalyzeConfigError::InvalidOverrideSyntax { raw: raw.into() });
        };
        let path = match path {
            "queueing.trigger_permille" => "queueing.trigger_permille",
            "blocking.min_nonzero_samples_for_signal" => "blocking.min_nonzero_samples_for_signal",
            "blocking.strong_p95_threshold" => "blocking.strong_p95_threshold",
            "blocking.strong_peak_threshold" => "blocking.strong_peak_threshold",
            "blocking.strong_nonzero_share_permille" => "blocking.strong_nonzero_share_permille",
            "blocking.strong_min_samples" => "blocking.strong_min_samples",
            "executor.min_global_queue_p95_for_signal" => {
                "executor.min_global_queue_p95_for_signal"
            }
            "downstream.min_stage_samples" => "downstream.min_stage_samples",
            "downstream.blocking_correlated_stage_patterns" => {
                "downstream.blocking_correlated_stage_patterns"
            }
            "downstream.blocking_correlation_score_margin" => {
                "downstream.blocking_correlation_score_margin"
            }
            "confidence.medium_score_threshold" => "confidence.medium_score_threshold",
            "confidence.high_score_threshold" => "confidence.high_score_threshold",
            "confidence.ambiguity_min_score" => "confidence.ambiguity_min_score",
            "confidence.ambiguity_score_gap" => "confidence.ambiguity_score_gap",
            "evidence.low_completed_request_threshold" => {
                "evidence.low_completed_request_threshold"
            }
            "route.min_request_count" => "route.min_request_count",
            "route.breakdown_limit" => "route.breakdown_limit",
            "route.emit_on_divergent_suspects" => "route.emit_on_divergent_suspects",
            "route.slowest_to_fastest_p95_ratio_numerator" => {
                "route.slowest_to_fastest_p95_ratio_numerator"
            }
            "route.slowest_to_fastest_p95_ratio_denominator" => {
                "route.slowest_to_fastest_p95_ratio_denominator"
            }
            "route.slowest_to_global_p95_ratio_numerator" => {
                "route.slowest_to_global_p95_ratio_numerator"
            }
            "route.slowest_to_global_p95_ratio_denominator" => {
                "route.slowest_to_global_p95_ratio_denominator"
            }
            "temporal.min_request_count" => "temporal.min_request_count",
            "temporal.min_segment_request_count" => "temporal.min_segment_request_count",
            "temporal.share_shift_permille" => "temporal.share_shift_permille",
            "temporal.p95_shift_ratio_numerator" => "temporal.p95_shift_ratio_numerator",
            "temporal.p95_shift_ratio_denominator" => "temporal.p95_shift_ratio_denominator",
            "temporal.emit_on_suspect_shift" => "temporal.emit_on_suspect_shift",
            "temporal.suppress_runtime_sparse_suspect_shift_without_supporting_movement" => {
                "temporal.suppress_runtime_sparse_suspect_shift_without_supporting_movement"
            }
            _ => {
                return Err(AnalyzeConfigError::UnknownOverridePath {
                    path: path.to_owned(),
                    suggestion: nearest_path_suggestion(path),
                });
            }
        };
        match path {
            "queueing.trigger_permille" => self.queueing.trigger_permille = parse_u64(path, value)?,
            "blocking.min_nonzero_samples_for_signal" => {
                self.blocking.min_nonzero_samples_for_signal = parse_usize(path, value)?;
            }
            "blocking.strong_p95_threshold" => {
                self.blocking.strong_p95_threshold = parse_u64(path, value)?;
            }
            "blocking.strong_peak_threshold" => {
                self.blocking.strong_peak_threshold = parse_u64(path, value)?;
            }
            "blocking.strong_nonzero_share_permille" => {
                self.blocking.strong_nonzero_share_permille = parse_u64(path, value)?;
            }
            "blocking.strong_min_samples" => {
                self.blocking.strong_min_samples = parse_usize(path, value)?;
            }
            "executor.min_global_queue_p95_for_signal" => {
                self.executor.min_global_queue_p95_for_signal = parse_u64(path, value)?;
            }
            "downstream.min_stage_samples" => {
                self.downstream.min_stage_samples = parse_usize(path, value)?;
            }
            "downstream.blocking_correlated_stage_patterns" => {
                self.downstream.blocking_correlated_stage_patterns = parse_list(path, value)?;
            }
            "downstream.blocking_correlation_score_margin" => {
                self.downstream.blocking_correlation_score_margin = parse_u8(path, value)?;
            }
            "confidence.medium_score_threshold" => {
                self.confidence.medium_score_threshold = parse_u8(path, value)?;
            }
            "confidence.high_score_threshold" => {
                self.confidence.high_score_threshold = parse_u8(path, value)?;
            }
            "confidence.ambiguity_min_score" => {
                self.confidence.ambiguity_min_score = parse_u8(path, value)?;
            }
            "confidence.ambiguity_score_gap" => {
                self.confidence.ambiguity_score_gap = parse_u8(path, value)?;
            }
            "evidence.low_completed_request_threshold" => {
                self.evidence.low_completed_request_threshold = parse_usize(path, value)?;
            }
            "route.min_request_count" => self.route.min_request_count = parse_usize(path, value)?,
            "route.breakdown_limit" => self.route.breakdown_limit = parse_usize(path, value)?,
            "route.emit_on_divergent_suspects" => {
                self.route.emit_on_divergent_suspects = parse_bool(path, value)?;
            }
            "route.slowest_to_fastest_p95_ratio_numerator" => {
                self.route.slowest_to_fastest_p95_ratio_numerator = parse_u64(path, value)?;
            }
            "route.slowest_to_fastest_p95_ratio_denominator" => {
                self.route.slowest_to_fastest_p95_ratio_denominator = parse_u64(path, value)?;
            }
            "route.slowest_to_global_p95_ratio_numerator" => {
                self.route.slowest_to_global_p95_ratio_numerator = parse_u64(path, value)?;
            }
            "route.slowest_to_global_p95_ratio_denominator" => {
                self.route.slowest_to_global_p95_ratio_denominator = parse_u64(path, value)?;
            }
            "temporal.min_request_count" => {
                self.temporal.min_request_count = parse_usize(path, value)?;
            }
            "temporal.min_segment_request_count" => {
                self.temporal.min_segment_request_count = parse_usize(path, value)?;
            }
            "temporal.share_shift_permille" => {
                self.temporal.share_shift_permille = parse_u64(path, value)?;
            }
            "temporal.p95_shift_ratio_numerator" => {
                self.temporal.p95_shift_ratio_numerator = parse_u64(path, value)?;
            }
            "temporal.p95_shift_ratio_denominator" => {
                self.temporal.p95_shift_ratio_denominator = parse_u64(path, value)?;
            }
            "temporal.emit_on_suspect_shift" => {
                self.temporal.emit_on_suspect_shift = parse_bool(path, value)?;
            }
            "temporal.suppress_runtime_sparse_suspect_shift_without_supporting_movement" => {
                self.temporal
                    .suppress_runtime_sparse_suspect_shift_without_supporting_movement =
                    parse_bool(path, value)?;
            }
            _ => unreachable!("validated override path should always match"),
        }
        self.validate()
    }

    /// Applies multiple overrides in order, stopping on first error.
    ///
    /// # Errors
    /// Returns the first [`AnalyzeConfigError`] encountered while applying overrides.
    pub fn apply_overrides<I, S>(&mut self, overrides: I) -> Result<(), AnalyzeConfigError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        for override_raw in overrides {
            self.apply_override(override_raw.as_ref())?;
        }
        Ok(())
    }
}

fn parse_u64(path: &'static str, value: &str) -> Result<u64, AnalyzeConfigError> {
    value
        .parse::<u64>()
        .map_err(|_| AnalyzeConfigError::InvalidOverrideValue {
            path,
            value: value.to_owned(),
            expected: "base-10 unsigned integer (u64)",
        })
}

fn parse_usize(path: &'static str, value: &str) -> Result<usize, AnalyzeConfigError> {
    value
        .parse::<usize>()
        .map_err(|_| AnalyzeConfigError::InvalidOverrideValue {
            path,
            value: value.to_owned(),
            expected: "base-10 unsigned integer (usize)",
        })
}

fn parse_u8(path: &'static str, value: &str) -> Result<u8, AnalyzeConfigError> {
    value
        .parse::<u8>()
        .map_err(|_| AnalyzeConfigError::InvalidOverrideValue {
            path,
            value: value.to_owned(),
            expected: "base-10 unsigned integer in 0..=255 (u8)",
        })
}

fn parse_bool(path: &'static str, value: &str) -> Result<bool, AnalyzeConfigError> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(AnalyzeConfigError::InvalidOverrideValue {
            path,
            value: value.to_owned(),
            expected: "boolean literal true or false",
        }),
    }
}

fn parse_list(path: &'static str, value: &str) -> Result<Vec<String>, AnalyzeConfigError> {
    value
        .split(',')
        .map(str::trim)
        .map(|entry| {
            if entry.is_empty() {
                Err(AnalyzeConfigError::InvalidOverrideValue {
                    path,
                    value: value.to_owned(),
                    expected: "comma-separated non-empty strings",
                })
            } else {
                Ok(entry.to_owned())
            }
        })
        .collect()
}

fn nearest_path_suggestion(path: &str) -> Option<&'static str> {
    VALID_OVERRIDE_PATHS
        .iter()
        .map(|candidate| (*candidate, edit_distance(path, candidate)))
        .min_by_key(|(_, distance)| *distance)
        .and_then(|(candidate, distance)| (distance <= 3).then_some(candidate))
}

fn edit_distance(lhs: &str, rhs: &str) -> usize {
    let rhs_chars: Vec<char> = rhs.chars().collect();
    let mut prev: Vec<usize> = (0..=rhs_chars.len()).collect();
    let mut curr = vec![0; rhs_chars.len() + 1];

    for (i, lc) in lhs.chars().enumerate() {
        curr[0] = i + 1;
        for (j, rc) in rhs_chars.iter().enumerate() {
            let cost = usize::from(lc != *rc);
            curr[j + 1] = (prev[j + 1] + 1).min(curr[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[rhs_chars.len()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::analyze_option_descriptors;
    use std::collections::HashSet;

    #[test]
    fn every_valid_path_can_be_applied() {
        let mut options = AnalyzeOptions::default();
        let valid = [
            "queueing.trigger_permille=250",
            "blocking.min_nonzero_samples_for_signal=5",
            "blocking.strong_p95_threshold=15",
            "blocking.strong_peak_threshold=25",
            "blocking.strong_nonzero_share_permille=600",
            "blocking.strong_min_samples=40",
            "executor.min_global_queue_p95_for_signal=2",
            "downstream.min_stage_samples=5",
            "downstream.blocking_correlated_stage_patterns=alpha, beta",
            "downstream.blocking_correlation_score_margin=3",
            "confidence.medium_score_threshold=70",
            "confidence.high_score_threshold=90",
            "confidence.ambiguity_min_score=65",
            "confidence.ambiguity_score_gap=5",
            "evidence.low_completed_request_threshold=25",
            "route.min_request_count=4",
            "route.breakdown_limit=12",
            "route.emit_on_divergent_suspects=false",
            "route.slowest_to_fastest_p95_ratio_numerator=4",
            "route.slowest_to_fastest_p95_ratio_denominator=3",
            "route.slowest_to_global_p95_ratio_numerator=6",
            "route.slowest_to_global_p95_ratio_denominator=5",
            "temporal.min_request_count=22",
            "temporal.min_segment_request_count=10",
            "temporal.share_shift_permille=250",
            "temporal.p95_shift_ratio_numerator=4",
            "temporal.p95_shift_ratio_denominator=3",
            "temporal.emit_on_suspect_shift=false",
            "temporal.suppress_runtime_sparse_suspect_shift_without_supporting_movement=false",
        ];
        options.apply_overrides(valid).expect("all overrides valid");
    }

    #[test]
    fn descriptors_and_valid_paths_match() {
        let descriptor_paths: HashSet<_> = analyze_option_descriptors()
            .iter()
            .map(|d| d.path)
            .collect();
        let override_paths: HashSet<_> = AnalyzeOptions::valid_override_paths()
            .iter()
            .copied()
            .collect();
        assert_eq!(descriptor_paths, override_paths);
    }

    #[test]
    fn valid_paths_have_no_duplicates() {
        let mut seen = HashSet::new();
        for path in AnalyzeOptions::valid_override_paths() {
            assert!(seen.insert(*path), "duplicate path: {path}");
        }
    }

    #[test]
    fn missing_equals_fails() {
        let err = AnalyzeOptions::default()
            .apply_override("queueing.trigger_permille")
            .expect_err("missing = should fail");
        assert!(matches!(
            err,
            AnalyzeConfigError::InvalidOverrideSyntax { .. }
        ));
    }

    #[test]
    fn unknown_path_fails_and_suggests() {
        let err = AnalyzeOptions::default()
            .apply_override("queuing.trigger_permille=300")
            .expect_err("unknown path should fail");
        assert_eq!(
            err,
            AnalyzeConfigError::UnknownOverridePath {
                path: "queuing.trigger_permille".into(),
                suggestion: Some("queueing.trigger_permille"),
            }
        );
    }

    #[test]
    fn invalid_numeric_and_bool_types_fail() {
        let err = AnalyzeOptions::default()
            .apply_override("queueing.trigger_permille=abc")
            .expect_err("invalid unsigned int should fail");
        assert!(matches!(
            err,
            AnalyzeConfigError::InvalidOverrideValue { .. }
        ));

        let err = AnalyzeOptions::default()
            .apply_override("confidence.medium_score_threshold=256")
            .expect_err("u8 overflow should fail");
        assert!(matches!(
            err,
            AnalyzeConfigError::InvalidOverrideValue { .. }
        ));

        let err = AnalyzeOptions::default()
            .apply_override("route.emit_on_divergent_suspects=yes")
            .expect_err("invalid bool should fail");
        assert!(matches!(
            err,
            AnalyzeConfigError::InvalidOverrideValue { .. }
        ));
    }

    #[test]
    fn list_trims_and_rejects_empty_entries() {
        let mut options = AnalyzeOptions::default();
        options
            .apply_override("downstream.blocking_correlated_stage_patterns=a, b ,c")
            .expect("list parse should work");
        assert_eq!(
            options.downstream.blocking_correlated_stage_patterns,
            vec!["a", "b", "c"]
        );

        let err = options
            .apply_override("downstream.blocking_correlated_stage_patterns=a,,b")
            .expect_err("empty list entry should fail");
        assert!(matches!(
            err,
            AnalyzeConfigError::InvalidOverrideValue { .. }
        ));
    }

    #[test]
    fn repeated_override_uses_last_value() {
        let mut options = AnalyzeOptions::default();
        options
            .apply_overrides([
                "queueing.trigger_permille=250",
                "queueing.trigger_permille=260",
            ])
            .expect("overrides should pass");
        assert_eq!(options.queueing.trigger_permille, 260);
    }

    #[test]
    fn invalid_config_from_override_fails_validation() {
        let mut options = AnalyzeOptions::default();
        let err = options
            .apply_override("route.breakdown_limit=0")
            .expect_err("validation should fail");
        assert_eq!(
            err,
            AnalyzeConfigError::InvalidConfigValue {
                path: "route.breakdown_limit",
                message: "must be > 0".into(),
            }
        );
    }

    #[test]
    fn apply_overrides_stops_on_first_error() {
        let mut options = AnalyzeOptions::default();
        let err = options
            .apply_overrides([
                "queueing.trigger_permille=250",
                "bad.path=1",
                "queueing.trigger_permille=260",
            ])
            .expect_err("should stop on first error");
        assert!(matches!(
            err,
            AnalyzeConfigError::UnknownOverridePath { .. }
        ));
        assert_eq!(options.queueing.trigger_permille, 250);
    }
}
