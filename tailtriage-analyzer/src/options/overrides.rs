#[cfg(test)]
use super::analyze_option_descriptors;
use super::{AnalyzeConfigError, AnalyzeOptions};

use super::registry;
use std::sync::OnceLock;

impl AnalyzeOptions {
    /// Returns every supported v1 CLI override path (`group.field`).
    #[must_use]
    pub fn valid_override_paths() -> &'static [&'static str] {
        static PATHS: OnceLock<Box<[&'static str]>> = OnceLock::new();
        PATHS
            .get_or_init(|| registry::valid_override_paths().into_boxed_slice())
            .as_ref()
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
        registry::apply_path(&mut candidate, path, value)?;
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn supported_path_values() -> Vec<(&'static str, &'static str, &'static str)> {
        vec![
            ("queueing.trigger_permille", "250", "250"),
            ("blocking.min_nonzero_samples_for_signal", "3", "3"),
            ("blocking.strong_p95_threshold", "15", "15"),
            ("blocking.strong_peak_threshold", "25", "25"),
            ("blocking.strong_nonzero_share_permille", "750", "750"),
            ("blocking.strong_min_samples", "40", "40"),
            ("executor.min_global_queue_p95_for_signal", "2", "2"),
            ("downstream.min_stage_samples", "5", "5"),
            (
                "downstream.blocking_correlated_stage_patterns",
                "[\"db\", \"cache\"]",
                "db,cache",
            ),
            ("downstream.blocking_correlation_score_margin", "3", "3"),
            ("confidence.medium_score_threshold", "70", "70"),
            ("confidence.high_score_threshold", "90", "90"),
            ("confidence.ambiguity_min_score", "65", "65"),
            ("confidence.ambiguity_score_gap", "5", "5"),
            ("evidence.low_completed_request_threshold", "30", "30"),
            ("route.min_request_count", "4", "4"),
            ("route.breakdown_limit", "12", "12"),
            ("route.emit_on_divergent_suspects", "false", "false"),
            ("route.slowest_to_fastest_p95_ratio_numerator", "4", "4"),
            ("route.slowest_to_fastest_p95_ratio_denominator", "1", "1"),
            ("route.slowest_to_global_p95_ratio_numerator", "6", "6"),
            ("route.slowest_to_global_p95_ratio_denominator", "3", "3"),
            ("temporal.min_request_count", "24", "24"),
            ("temporal.min_segment_request_count", "10", "10"),
            ("temporal.share_shift_permille", "250", "250"),
            ("temporal.p95_shift_ratio_numerator", "4", "4"),
            ("temporal.p95_shift_ratio_denominator", "1", "1"),
            ("temporal.emit_on_suspect_shift", "false", "false"),
            (
                "temporal.suppress_runtime_sparse_suspect_shift_without_supporting_movement",
                "false",
                "false",
            ),
        ]
    }

    fn toml_for(path: &str, value: &str) -> String {
        let (group, field) = path.split_once('.').expect("path has group");
        format!("[analyzer]\nschema_version = 1\n\n[analyzer.{group}]\n{field} = {value}\n")
    }

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
    fn full_current_path_set_remains_supported() {
        let expected = supported_path_values()
            .into_iter()
            .map(|(path, _, _)| path)
            .collect::<Vec<_>>();
        assert_eq!(AnalyzeOptions::valid_override_paths(), expected.as_slice());
    }

    #[test]
    fn toml_and_cli_produce_identical_options_for_each_supported_path() {
        for (path, toml_value, cli_value) in supported_path_values() {
            let toml_options = AnalyzeOptions::from_toml_str(&toml_for(path, toml_value))
                .unwrap_or_else(|err| panic!("TOML failed for {path}: {err}"));
            let mut cli_options = AnalyzeOptions::default();
            cli_options
                .apply_override(&format!("{path}={cli_value}"))
                .unwrap_or_else(|err| panic!("CLI failed for {path}: {err}"));
            assert_eq!(toml_options, cli_options, "path {path}");
        }
    }

    #[test]
    fn non_default_summaries_cover_every_changed_path() {
        for (path, _, cli_value) in supported_path_values() {
            let mut options = AnalyzeOptions::default();
            options
                .apply_override(&format!("{path}={cli_value}"))
                .unwrap_or_else(|err| panic!("CLI failed for {path}: {err}"));
            let summaries = options.non_default_overrides();
            assert!(
                summaries.iter().any(|summary| summary.path == path),
                "missing summary for {path}: {summaries:?}"
            );
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
