use super::{AnalyzeConfigError, AnalyzeOptions};

const VALID_OVERRIDE_PATHS: [&str; 29] = [
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
    /// Returns all supported `group.field` override paths for analyzer CLI-style overrides.
    #[must_use]
    pub fn valid_override_paths() -> &'static [&'static str] {
        &VALID_OVERRIDE_PATHS
    }

    /// Applies one `path=value` override and validates the full resulting configuration.
    ///
    /// # Errors
    /// Returns [`AnalyzeConfigError`] when syntax is invalid, path is unknown,
    /// value parsing fails, or resulting semantic validation fails.
    #[allow(clippy::too_many_lines)]
    pub fn apply_override(&mut self, raw: &str) -> Result<(), AnalyzeConfigError> {
        let Some((path, value)) = raw.split_once('=') else {
            return Err(AnalyzeConfigError::InvalidOverrideSyntax {
                raw: raw.to_owned(),
            });
        };

        match path {
            "queueing.trigger_permille" => {
                self.queueing.trigger_permille = parse_u64("queueing.trigger_permille", value)?;
            }
            "blocking.min_nonzero_samples_for_signal" => {
                self.blocking.min_nonzero_samples_for_signal =
                    parse_usize("blocking.min_nonzero_samples_for_signal", value)?;
            }
            "blocking.strong_p95_threshold" => {
                self.blocking.strong_p95_threshold =
                    parse_u64("blocking.strong_p95_threshold", value)?;
            }
            "blocking.strong_peak_threshold" => {
                self.blocking.strong_peak_threshold =
                    parse_u64("blocking.strong_peak_threshold", value)?;
            }
            "blocking.strong_nonzero_share_permille" => {
                self.blocking.strong_nonzero_share_permille =
                    parse_u64("blocking.strong_nonzero_share_permille", value)?;
            }
            "blocking.strong_min_samples" => {
                self.blocking.strong_min_samples =
                    parse_usize("blocking.strong_min_samples", value)?;
            }
            "executor.min_global_queue_p95_for_signal" => {
                self.executor.min_global_queue_p95_for_signal =
                    parse_u64("executor.min_global_queue_p95_for_signal", value)?;
            }
            "downstream.min_stage_samples" => {
                self.downstream.min_stage_samples =
                    parse_usize("downstream.min_stage_samples", value)?;
            }
            "downstream.blocking_correlated_stage_patterns" => {
                self.downstream.blocking_correlated_stage_patterns =
                    parse_string_list("downstream.blocking_correlated_stage_patterns", value)?;
            }
            "downstream.blocking_correlation_score_margin" => {
                self.downstream.blocking_correlation_score_margin =
                    parse_u8("downstream.blocking_correlation_score_margin", value)?;
            }
            "confidence.medium_score_threshold" => {
                self.confidence.medium_score_threshold =
                    parse_u8("confidence.medium_score_threshold", value)?;
            }
            "confidence.high_score_threshold" => {
                self.confidence.high_score_threshold =
                    parse_u8("confidence.high_score_threshold", value)?;
            }
            "confidence.ambiguity_min_score" => {
                self.confidence.ambiguity_min_score =
                    parse_u8("confidence.ambiguity_min_score", value)?;
            }
            "confidence.ambiguity_score_gap" => {
                self.confidence.ambiguity_score_gap =
                    parse_u8("confidence.ambiguity_score_gap", value)?;
            }
            "evidence.low_completed_request_threshold" => {
                self.evidence.low_completed_request_threshold =
                    parse_usize("evidence.low_completed_request_threshold", value)?;
            }
            "route.min_request_count" => {
                self.route.min_request_count = parse_usize("route.min_request_count", value)?;
            }
            "route.breakdown_limit" => {
                self.route.breakdown_limit = parse_usize("route.breakdown_limit", value)?;
            }
            "route.emit_on_divergent_suspects" => {
                self.route.emit_on_divergent_suspects =
                    parse_bool("route.emit_on_divergent_suspects", value)?;
            }
            "route.slowest_to_fastest_p95_ratio_numerator" => {
                self.route.slowest_to_fastest_p95_ratio_numerator =
                    parse_u64("route.slowest_to_fastest_p95_ratio_numerator", value)?;
            }
            "route.slowest_to_fastest_p95_ratio_denominator" => {
                self.route.slowest_to_fastest_p95_ratio_denominator =
                    parse_u64("route.slowest_to_fastest_p95_ratio_denominator", value)?;
            }
            "route.slowest_to_global_p95_ratio_numerator" => {
                self.route.slowest_to_global_p95_ratio_numerator =
                    parse_u64("route.slowest_to_global_p95_ratio_numerator", value)?;
            }
            "route.slowest_to_global_p95_ratio_denominator" => {
                self.route.slowest_to_global_p95_ratio_denominator =
                    parse_u64("route.slowest_to_global_p95_ratio_denominator", value)?;
            }
            "temporal.min_request_count" => {
                self.temporal.min_request_count = parse_usize("temporal.min_request_count", value)?;
            }
            "temporal.min_segment_request_count" => {
                self.temporal.min_segment_request_count =
                    parse_usize("temporal.min_segment_request_count", value)?;
            }
            "temporal.share_shift_permille" => {
                self.temporal.share_shift_permille =
                    parse_u64("temporal.share_shift_permille", value)?;
            }
            "temporal.p95_shift_ratio_numerator" => {
                self.temporal.p95_shift_ratio_numerator =
                    parse_u64("temporal.p95_shift_ratio_numerator", value)?;
            }
            "temporal.p95_shift_ratio_denominator" => {
                self.temporal.p95_shift_ratio_denominator =
                    parse_u64("temporal.p95_shift_ratio_denominator", value)?;
            }
            "temporal.emit_on_suspect_shift" => {
                self.temporal.emit_on_suspect_shift =
                    parse_bool("temporal.emit_on_suspect_shift", value)?;
            }
            "temporal.suppress_runtime_sparse_suspect_shift_without_supporting_movement" => {
                self.temporal
                    .suppress_runtime_sparse_suspect_shift_without_supporting_movement =
                    parse_bool("temporal.suppress_runtime_sparse_suspect_shift_without_supporting_movement", value)?;
            }
            _ => {
                return Err(AnalyzeConfigError::UnknownOverridePath {
                    path: path.to_owned(),
                    suggestion: nearest_path(path),
                });
            }
        }

        self.validate()
    }

    /// Applies multiple `path=value` overrides in order and stops on first failure.
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
            expected: "base-10 unsigned integer in range 0..=255 (u8)",
        })
}
fn parse_bool(path: &'static str, value: &str) -> Result<bool, AnalyzeConfigError> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(AnalyzeConfigError::InvalidOverrideValue {
            path,
            value: value.to_owned(),
            expected: "boolean literal 'true' or 'false'",
        }),
    }
}
fn parse_string_list(path: &'static str, value: &str) -> Result<Vec<String>, AnalyzeConfigError> {
    let mut out = Vec::new();
    for item in value.split(',') {
        let trimmed = item.trim();
        if trimmed.is_empty() {
            return Err(AnalyzeConfigError::InvalidOverrideValue {
                path,
                value: value.to_owned(),
                expected: "comma-separated non-empty strings (e.g. alpha,beta)",
            });
        }
        out.push(trimmed.to_owned());
    }
    Ok(out)
}

fn nearest_path(path: &str) -> Option<&'static str> {
    let best = VALID_OVERRIDE_PATHS
        .iter()
        .map(|candidate| (*candidate, edit_distance(path, candidate)))
        .min_by_key(|(_, distance)| *distance)?;
    (best.1 <= 3).then_some(best.0)
}

fn edit_distance(left: &str, right: &str) -> usize {
    let left_chars: Vec<char> = left.chars().collect();
    let right_chars: Vec<char> = right.chars().collect();
    let mut prev: Vec<usize> = (0..=right_chars.len()).collect();
    let mut curr = vec![0; right_chars.len() + 1];

    for (i, lch) in left_chars.iter().enumerate() {
        curr[0] = i + 1;
        for (j, rch) in right_chars.iter().enumerate() {
            let cost = usize::from(lch != rch);
            curr[j + 1] = (prev[j + 1] + 1).min(curr[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[right_chars.len()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::analyze_option_descriptors;
    use std::collections::HashSet;

    #[test]
    fn every_valid_path_applies() {
        let mut options = AnalyzeOptions::default();
        let overrides = [
            "queueing.trigger_permille=400",
            "blocking.min_nonzero_samples_for_signal=5",
            "blocking.strong_p95_threshold=13",
            "blocking.strong_peak_threshold=21",
            "blocking.strong_nonzero_share_permille=800",
            "blocking.strong_min_samples=40",
            "executor.min_global_queue_p95_for_signal=2",
            "downstream.min_stage_samples=4",
            "downstream.blocking_correlated_stage_patterns=foo, bar",
            "downstream.blocking_correlation_score_margin=3",
            "confidence.medium_score_threshold=70",
            "confidence.high_score_threshold=90",
            "confidence.ambiguity_min_score=61",
            "confidence.ambiguity_score_gap=5",
            "evidence.low_completed_request_threshold=30",
            "route.min_request_count=4",
            "route.breakdown_limit=12",
            "route.emit_on_divergent_suspects=false",
            "route.slowest_to_fastest_p95_ratio_numerator=4",
            "route.slowest_to_fastest_p95_ratio_denominator=2",
            "route.slowest_to_global_p95_ratio_numerator=6",
            "route.slowest_to_global_p95_ratio_denominator=4",
            "temporal.min_request_count=30",
            "temporal.min_segment_request_count=10",
            "temporal.share_shift_permille=300",
            "temporal.p95_shift_ratio_numerator=4",
            "temporal.p95_shift_ratio_denominator=2",
            "temporal.emit_on_suspect_shift=false",
            "temporal.suppress_runtime_sparse_suspect_shift_without_supporting_movement=false",
        ];
        options
            .apply_overrides(overrides)
            .expect("all valid overrides apply");
    }

    #[test]
    fn descriptor_paths_and_override_paths_match() {
        let descriptor_paths: HashSet<&str> = analyze_option_descriptors()
            .iter()
            .map(|d| d.path)
            .collect();
        let override_paths: HashSet<&str> = AnalyzeOptions::valid_override_paths()
            .iter()
            .copied()
            .collect();
        assert_eq!(descriptor_paths, override_paths);
    }

    #[test]
    fn valid_override_paths_have_no_duplicates() {
        let unique: HashSet<&str> = AnalyzeOptions::valid_override_paths()
            .iter()
            .copied()
            .collect();
        assert_eq!(unique.len(), AnalyzeOptions::valid_override_paths().len());
    }

    #[test]
    fn missing_equals_fails() {
        let err = AnalyzeOptions::default()
            .apply_override("queueing.trigger_permille")
            .unwrap_err();
        assert!(matches!(
            err,
            AnalyzeConfigError::InvalidOverrideSyntax { .. }
        ));
    }

    #[test]
    fn unknown_path_fails() {
        let err = AnalyzeOptions::default()
            .apply_override("unknown.path=1")
            .unwrap_err();
        assert!(matches!(
            err,
            AnalyzeConfigError::UnknownOverridePath { .. }
        ));
    }

    #[test]
    fn misspelled_path_gets_suggestion() {
        let err = AnalyzeOptions::default()
            .apply_override("queuing.trigger_permille=300")
            .unwrap_err();
        assert_eq!(
            err,
            AnalyzeConfigError::UnknownOverridePath {
                path: "queuing.trigger_permille".to_owned(),
                suggestion: Some("queueing.trigger_permille"),
            }
        );
    }

    #[test]
    fn invalid_unsigned_integer_fails() {
        let err = AnalyzeOptions::default()
            .apply_override("queueing.trigger_permille=abc")
            .unwrap_err();
        assert!(matches!(
            err,
            AnalyzeConfigError::InvalidOverrideValue { .. }
        ));
    }

    #[test]
    fn invalid_u8_overflow_fails() {
        let err = AnalyzeOptions::default()
            .apply_override("confidence.medium_score_threshold=300")
            .unwrap_err();
        assert!(matches!(
            err,
            AnalyzeConfigError::InvalidOverrideValue { .. }
        ));
    }

    #[test]
    fn invalid_bool_fails() {
        let err = AnalyzeOptions::default()
            .apply_override("route.emit_on_divergent_suspects=yes")
            .unwrap_err();
        assert!(matches!(
            err,
            AnalyzeConfigError::InvalidOverrideValue { .. }
        ));
    }

    #[test]
    fn list_entries_are_trimmed() {
        let mut options = AnalyzeOptions::default();
        options
            .apply_override("downstream.blocking_correlated_stage_patterns=  foo , bar  ")
            .unwrap();
        assert_eq!(
            options.downstream.blocking_correlated_stage_patterns,
            vec!["foo", "bar"]
        );
    }

    #[test]
    fn list_rejects_empty_entries() {
        let err = AnalyzeOptions::default()
            .apply_override("downstream.blocking_correlated_stage_patterns=foo, ,bar")
            .unwrap_err();
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
                "queueing.trigger_permille=301",
                "queueing.trigger_permille=302",
            ])
            .unwrap();
        assert_eq!(options.queueing.trigger_permille, 302);
    }

    #[test]
    fn invalid_combined_config_fails_validation() {
        let mut options = AnalyzeOptions::default();
        let err = options
            .apply_overrides([
                "temporal.min_request_count=10",
                "temporal.min_segment_request_count=8",
            ])
            .unwrap_err();
        assert!(matches!(err, AnalyzeConfigError::InvalidConfigValue { .. }));
    }

    #[test]
    fn apply_overrides_stops_on_first_error() {
        let mut options = AnalyzeOptions::default();
        let err = options
            .apply_overrides([
                "queueing.trigger_permille=350",
                "route.emit_on_divergent_suspects=not-bool",
                "queueing.trigger_permille=360",
            ])
            .unwrap_err();
        assert!(matches!(
            err,
            AnalyzeConfigError::InvalidOverrideValue { .. }
        ));
        assert_eq!(options.queueing.trigger_permille, 350);
    }
}
