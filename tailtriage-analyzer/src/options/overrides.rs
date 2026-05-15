#[cfg(test)]
use super::analyze_option_descriptors;
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
    /// Returns every supported v1 CLI override path (`group.field`).
    #[must_use]
    pub fn valid_override_paths() -> &'static [&'static str] {
        &VALID_OVERRIDE_PATHS
    }

    /// Applies CLI overrides in order as one transactional batch.
    ///
    /// # Errors
    /// Returns the first syntax, path, parse, or semantic validation error.
    pub fn apply_overrides<I, S>(&mut self, overrides: I) -> Result<(), AnalyzeConfigError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut candidate = self.clone();
        for raw in overrides {
            candidate.apply_override(raw.as_ref())?;
        }
        *self = candidate;
        Ok(())
    }

    /// Applies one `group.field=value` override and validates the full config.
    ///
    /// # Errors
    /// Returns syntax, path, value-parse, or semantic validation errors.
    pub fn apply_override(&mut self, raw: &str) -> Result<(), AnalyzeConfigError> {
        if raw.matches('=').count() != 1 {
            return Err(AnalyzeConfigError::InvalidOverrideSyntax {
                raw: raw.to_string(),
            });
        }
        let Some((path, value)) = raw.split_once('=') else {
            return Err(AnalyzeConfigError::InvalidOverrideSyntax {
                raw: raw.to_string(),
            });
        };
        let mut candidate = self.clone();
        apply_override_path(&mut candidate, path, value)?;
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }
}

#[allow(clippy::too_many_lines)]
fn apply_override_path(
    options: &mut AnalyzeOptions,
    path: &str,
    value: &str,
) -> Result<(), AnalyzeConfigError> {
    match path {
        "queueing.trigger_permille" => options.queueing.trigger_permille = parse_u64(path, value)?,
        "blocking.min_nonzero_samples_for_signal" => {
            options.blocking.min_nonzero_samples_for_signal = parse_usize(path, value)?;
        }
        "blocking.strong_p95_threshold" => {
            options.blocking.strong_p95_threshold = parse_u64(path, value)?;
        }
        "blocking.strong_peak_threshold" => {
            options.blocking.strong_peak_threshold = parse_u64(path, value)?;
        }
        "blocking.strong_nonzero_share_permille" => {
            options.blocking.strong_nonzero_share_permille = parse_u64(path, value)?;
        }
        "blocking.strong_min_samples" => {
            options.blocking.strong_min_samples = parse_usize(path, value)?;
        }
        "executor.min_global_queue_p95_for_signal" => {
            options.executor.min_global_queue_p95_for_signal = parse_u64(path, value)?;
        }
        "downstream.min_stage_samples" => {
            options.downstream.min_stage_samples = parse_usize(path, value)?;
        }
        "downstream.blocking_correlated_stage_patterns" => {
            options.downstream.blocking_correlated_stage_patterns = parse_string_list(path, value)?;
        }
        "downstream.blocking_correlation_score_margin" => {
            options.downstream.blocking_correlation_score_margin = parse_u8(path, value)?;
        }
        "confidence.medium_score_threshold" => {
            options.confidence.medium_score_threshold = parse_u8(path, value)?;
        }
        "confidence.high_score_threshold" => {
            options.confidence.high_score_threshold = parse_u8(path, value)?;
        }
        "confidence.ambiguity_min_score" => {
            options.confidence.ambiguity_min_score = parse_u8(path, value)?;
        }
        "confidence.ambiguity_score_gap" => {
            options.confidence.ambiguity_score_gap = parse_u8(path, value)?;
        }
        "evidence.low_completed_request_threshold" => {
            options.evidence.low_completed_request_threshold = parse_usize(path, value)?;
        }
        "route.min_request_count" => options.route.min_request_count = parse_usize(path, value)?,
        "route.breakdown_limit" => options.route.breakdown_limit = parse_usize(path, value)?,
        "route.emit_on_divergent_suspects" => {
            options.route.emit_on_divergent_suspects = parse_bool(path, value)?;
        }
        "route.slowest_to_fastest_p95_ratio_numerator" => {
            options.route.slowest_to_fastest_p95_ratio_numerator = parse_u64(path, value)?;
        }
        "route.slowest_to_fastest_p95_ratio_denominator" => {
            options.route.slowest_to_fastest_p95_ratio_denominator = parse_u64(path, value)?;
        }
        "route.slowest_to_global_p95_ratio_numerator" => {
            options.route.slowest_to_global_p95_ratio_numerator = parse_u64(path, value)?;
        }
        "route.slowest_to_global_p95_ratio_denominator" => {
            options.route.slowest_to_global_p95_ratio_denominator = parse_u64(path, value)?;
        }
        "temporal.min_request_count" => {
            options.temporal.min_request_count = parse_usize(path, value)?;
        }
        "temporal.min_segment_request_count" => {
            options.temporal.min_segment_request_count = parse_usize(path, value)?;
        }
        "temporal.share_shift_permille" => {
            options.temporal.share_shift_permille = parse_u64(path, value)?;
        }
        "temporal.p95_shift_ratio_numerator" => {
            options.temporal.p95_shift_ratio_numerator = parse_u64(path, value)?;
        }
        "temporal.p95_shift_ratio_denominator" => {
            options.temporal.p95_shift_ratio_denominator = parse_u64(path, value)?;
        }
        "temporal.emit_on_suspect_shift" => {
            options.temporal.emit_on_suspect_shift = parse_bool(path, value)?;
        }
        "temporal.suppress_runtime_sparse_suspect_shift_without_supporting_movement" => {
            options
                .temporal
                .suppress_runtime_sparse_suspect_shift_without_supporting_movement =
                parse_bool(path, value)?;
        }
        _ => {
            return Err(AnalyzeConfigError::UnknownOverridePath {
                path: path.to_string(),
                suggestion: suggest_path(path),
            })
        }
    }
    Ok(())
}
fn parse_u64(path: &str, value: &str) -> Result<u64, AnalyzeConfigError> {
    parse_num(path, value, "base-10 unsigned integer (u64)")
}
fn parse_usize(path: &str, value: &str) -> Result<usize, AnalyzeConfigError> {
    parse_num(path, value, "base-10 unsigned integer (usize)")
}
fn parse_u8(path: &str, value: &str) -> Result<u8, AnalyzeConfigError> {
    parse_num(
        path,
        value,
        "base-10 unsigned integer in range 0..=255 (u8)",
    )
}
fn parse_num<T: std::str::FromStr>(
    path: &str,
    value: &str,
    expected: &'static str,
) -> Result<T, AnalyzeConfigError> {
    value
        .parse()
        .map_err(|_| AnalyzeConfigError::InvalidOverrideValue {
            path: path_to_static(path),
            value: value.to_string(),
            expected,
        })
}
fn parse_bool(path: &str, value: &str) -> Result<bool, AnalyzeConfigError> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(AnalyzeConfigError::InvalidOverrideValue {
            path: path_to_static(path),
            value: value.to_string(),
            expected: "'true' or 'false'",
        }),
    }
}
fn parse_string_list(path: &str, value: &str) -> Result<Vec<String>, AnalyzeConfigError> {
    let mut out = Vec::new();
    for entry in value.split(',') {
        let trimmed = entry.trim();
        if trimmed.is_empty() {
            return Err(AnalyzeConfigError::InvalidOverrideValue {
                path: path_to_static(path),
                value: value.to_string(),
                expected: "comma-separated non-empty entries (Vec<String>)",
            });
        }
        out.push(trimmed.to_string());
    }
    Ok(out)
}
fn suggest_path(path: &str) -> Option<&'static str> {
    VALID_OVERRIDE_PATHS
        .iter()
        .map(|candidate| (*candidate, edit_distance(path, candidate)))
        .min_by_key(|(_, d)| *d)
        .and_then(|(c, d)| (d <= 3).then_some(c))
}
fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut curr = vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        curr[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            curr[j + 1] = (prev[j + 1] + 1).min(curr[j] + 1).min(prev[j] + cost);
        }
        prev.clone_from(&curr);
    }
    prev[b.len()]
}
fn path_to_static(path: &str) -> &'static str {
    VALID_OVERRIDE_PATHS
        .iter()
        .copied()
        .find(|candidate| *candidate == path)
        .unwrap_or("<unknown-path>")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    #[test]
    fn valid_paths_descriptors_and_duplicates() {
        let descriptor_paths: HashSet<&'static str> = analyze_option_descriptors()
            .iter()
            .map(|d| d.path)
            .collect();
        let override_paths = AnalyzeOptions::valid_override_paths();
        let unique: HashSet<&'static str> = override_paths.iter().copied().collect();
        assert_eq!(override_paths.len(), unique.len());
        for path in override_paths {
            assert!(descriptor_paths.contains(path));
        }
        for path in descriptor_paths {
            assert!(unique.contains(path));
        }
    }
    #[test]
    fn every_valid_path_can_be_applied() {
        let mut opts = AnalyzeOptions::default();
        for (path, value) in [
            ("queueing.trigger_permille", "250"),
            ("blocking.min_nonzero_samples_for_signal", "3"),
            ("blocking.strong_p95_threshold", "15"),
            ("blocking.strong_peak_threshold", "25"),
            ("blocking.strong_nonzero_share_permille", "750"),
            ("blocking.strong_min_samples", "40"),
            ("executor.min_global_queue_p95_for_signal", "2"),
            ("downstream.min_stage_samples", "5"),
            ("downstream.blocking_correlated_stage_patterns", "db, cache"),
            ("downstream.blocking_correlation_score_margin", "3"),
            ("confidence.medium_score_threshold", "70"),
            ("confidence.high_score_threshold", "90"),
            ("confidence.ambiguity_min_score", "65"),
            ("confidence.ambiguity_score_gap", "5"),
            ("evidence.low_completed_request_threshold", "30"),
            ("route.min_request_count", "4"),
            ("route.breakdown_limit", "12"),
            ("route.emit_on_divergent_suspects", "false"),
            ("route.slowest_to_fastest_p95_ratio_numerator", "4"),
            ("route.slowest_to_fastest_p95_ratio_denominator", "2"),
            ("route.slowest_to_global_p95_ratio_numerator", "6"),
            ("route.slowest_to_global_p95_ratio_denominator", "4"),
            ("temporal.min_request_count", "24"),
            ("temporal.min_segment_request_count", "10"),
            ("temporal.share_shift_permille", "250"),
            ("temporal.p95_shift_ratio_numerator", "4"),
            ("temporal.p95_shift_ratio_denominator", "2"),
            ("temporal.emit_on_suspect_shift", "false"),
            (
                "temporal.suppress_runtime_sparse_suspect_shift_without_supporting_movement",
                "false",
            ),
        ] {
            opts.apply_override(&format!("{path}={value}")).expect(path);
        }
    }
    #[test]
    fn missing_equals_fails() {
        assert!(matches!(
            AnalyzeOptions::default().apply_override("queueing.trigger_permille"),
            Err(AnalyzeConfigError::InvalidOverrideSyntax { .. })
        ));
    }

    #[test]
    fn extra_equals_fails() {
        assert!(matches!(
            AnalyzeOptions::default().apply_override("queueing.trigger_permille=1=2"),
            Err(AnalyzeConfigError::InvalidOverrideSyntax { .. })
        ));
    }
    #[test]
    fn unknown_path_fails() {
        assert!(matches!(
            AnalyzeOptions::default().apply_override("nope.field=1"),
            Err(AnalyzeConfigError::UnknownOverridePath { .. })
        ));
    }
    #[test]
    fn misspelled_path_has_suggestion() {
        match AnalyzeOptions::default().apply_override("queuing.trigger_permille=1") {
            Err(AnalyzeConfigError::UnknownOverridePath { suggestion, .. }) => {
                assert_eq!(suggestion, Some("queueing.trigger_permille"));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }
    #[test]
    fn invalid_unsigned_integer_fails() {
        assert!(matches!(
            AnalyzeOptions::default().apply_override("queueing.trigger_permille=-1"),
            Err(AnalyzeConfigError::InvalidOverrideValue { .. })
        ));
    }
    #[test]
    fn invalid_u8_overflow_fails() {
        assert!(matches!(
            AnalyzeOptions::default().apply_override("confidence.high_score_threshold=999"),
            Err(AnalyzeConfigError::InvalidOverrideValue { .. })
        ));
    }
    #[test]
    fn invalid_bool_fails() {
        assert!(matches!(
            AnalyzeOptions::default().apply_override("route.emit_on_divergent_suspects=yes"),
            Err(AnalyzeConfigError::InvalidOverrideValue { .. })
        ));
    }

    #[test]
    fn valid_bool_override_works() {
        let mut opts = AnalyzeOptions::default();
        opts.apply_override("route.emit_on_divergent_suspects=false")
            .expect("valid bool");
        assert!(!opts.route.emit_on_divergent_suspects);
    }
    #[test]
    fn list_parsing_trims_entries() {
        let mut opts = AnalyzeOptions::default();
        opts.apply_override("downstream.blocking_correlated_stage_patterns=alpha,beta")
            .expect("valid list");
        assert_eq!(
            opts.downstream.blocking_correlated_stage_patterns,
            vec!["alpha", "beta"]
        );
    }
    #[test]
    fn list_parsing_rejects_empty_entries() {
        assert!(matches!(
            AnalyzeOptions::default()
                .apply_override("downstream.blocking_correlated_stage_patterns=db,,cache"),
            Err(AnalyzeConfigError::InvalidOverrideValue { .. })
        ));
    }
    #[test]
    fn repeated_override_last_wins() {
        let mut opts = AnalyzeOptions::default();
        opts.apply_override("queueing.trigger_permille=350")
            .expect("first");
        opts.apply_override("queueing.trigger_permille=450")
            .expect("second");
        assert_eq!(opts.queueing.trigger_permille, 450);
    }
    #[test]
    fn override_invalid_config_fails_validation() {
        assert!(matches!(
            AnalyzeOptions::default().apply_override("route.breakdown_limit=0"),
            Err(AnalyzeConfigError::InvalidConfigValue {
                path: "route.breakdown_limit",
                ..
            })
        ));
    }

    #[test]
    fn apply_override_invalid_semantic_value_is_transactional() {
        let mut opts = AnalyzeOptions::default();
        let before = opts.route.breakdown_limit;
        let err = opts
            .apply_override("route.breakdown_limit=0")
            .expect_err("must fail");
        assert!(matches!(
            err,
            AnalyzeConfigError::InvalidConfigValue {
                path: "route.breakdown_limit",
                ..
            }
        ));
        assert_eq!(opts.route.breakdown_limit, before);
    }

    #[test]
    fn apply_override_invalid_path_is_transactional() {
        let mut opts = AnalyzeOptions::default();
        let before = opts.clone();
        let err = opts.apply_override("bad.path=1").expect_err("must fail");
        assert!(matches!(
            err,
            AnalyzeConfigError::UnknownOverridePath { .. }
        ));
        assert_eq!(opts, before);
    }

    #[test]
    fn apply_overrides_is_transactional_when_later_override_fails() {
        let mut opts = AnalyzeOptions::default();
        let before = opts.clone();
        let err = opts
            .apply_overrides(["queueing.trigger_permille=450", "bad.path=1"])
            .expect_err("must fail");
        assert!(matches!(
            err,
            AnalyzeConfigError::UnknownOverridePath { .. }
        ));
        assert_eq!(opts, before);
    }

    #[test]
    fn apply_overrides_repeated_valid_override_last_wins() {
        let mut opts = AnalyzeOptions::default();
        opts.apply_overrides([
            "queueing.trigger_permille=350",
            "queueing.trigger_permille=450",
        ])
        .expect("valid overrides");
        assert_eq!(opts.queueing.trigger_permille, 450);
    }

    #[test]
    fn apply_overrides_stops_on_first_error() {
        let mut opts = AnalyzeOptions::default();
        let err = opts
            .apply_overrides([
                "queueing.trigger_permille=450",
                "bad.path=1",
                "confidence.high_score_threshold=90",
            ])
            .expect_err("must fail");
        assert!(matches!(
            err,
            AnalyzeConfigError::UnknownOverridePath { .. }
        ));
        assert_eq!(opts.queueing.trigger_permille, 300);
        assert_eq!(opts.confidence.high_score_threshold, 85);
    }
}
