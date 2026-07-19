use super::registry::{find_entry, suggest_path, OPTION_ENTRIES};
use super::{AnalyzeConfigError, AnalyzeOptions};
use std::sync::LazyLock;

static VALID_OVERRIDE_PATHS: LazyLock<Vec<&'static str>> =
    LazyLock::new(|| OPTION_ENTRIES.iter().map(|entry| entry.path).collect());

impl AnalyzeOptions {
    /// Returns every supported v1 CLI override path (`group.field`).
    #[must_use]
    pub fn valid_override_paths() -> &'static [&'static str] {
        VALID_OVERRIDE_PATHS.as_slice()
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
        let Some(entry) = find_entry(path) else {
            return Err(AnalyzeConfigError::UnknownOverridePath {
                path: path.to_string(),
                suggestion: suggest_path(path),
            });
        };
        let parsed = entry.parse_cli(value)?;
        let mut candidate = self.clone();
        entry.apply(&mut candidate, parsed);
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyze_option_descriptors;
    use std::collections::HashSet;

    const EXPECTED_PATHS: &[&str] = &[
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

    #[derive(Clone, Copy)]
    struct Case {
        path: &'static str,
        cli: &'static str,
        toml: &'static str,
        summary: &'static str,
    }

    const CASES: &[Case] = &[
        Case {
            path: "queueing.trigger_permille",
            cli: "250",
            toml: "250",
            summary: "250",
        },
        Case {
            path: "blocking.min_nonzero_samples_for_signal",
            cli: "3",
            toml: "3",
            summary: "3",
        },
        Case {
            path: "blocking.strong_p95_threshold",
            cli: "15",
            toml: "15",
            summary: "15",
        },
        Case {
            path: "blocking.strong_peak_threshold",
            cli: "25",
            toml: "25",
            summary: "25",
        },
        Case {
            path: "blocking.strong_nonzero_share_permille",
            cli: "750",
            toml: "750",
            summary: "750",
        },
        Case {
            path: "blocking.strong_min_samples",
            cli: "40",
            toml: "40",
            summary: "40",
        },
        Case {
            path: "executor.min_global_queue_p95_for_signal",
            cli: "2",
            toml: "2",
            summary: "2",
        },
        Case {
            path: "downstream.min_stage_samples",
            cli: "5",
            toml: "5",
            summary: "5",
        },
        Case {
            path: "downstream.blocking_correlated_stage_patterns",
            cli: "db, cache",
            toml: "[\"db\", \"cache\"]",
            summary: "db,cache",
        },
        Case {
            path: "downstream.blocking_correlation_score_margin",
            cli: "3",
            toml: "3",
            summary: "3",
        },
        Case {
            path: "confidence.medium_score_threshold",
            cli: "70",
            toml: "70",
            summary: "70",
        },
        Case {
            path: "confidence.high_score_threshold",
            cli: "90",
            toml: "90",
            summary: "90",
        },
        Case {
            path: "confidence.ambiguity_min_score",
            cli: "65",
            toml: "65",
            summary: "65",
        },
        Case {
            path: "confidence.ambiguity_score_gap",
            cli: "5",
            toml: "5",
            summary: "5",
        },
        Case {
            path: "evidence.low_completed_request_threshold",
            cli: "30",
            toml: "30",
            summary: "30",
        },
        Case {
            path: "route.min_request_count",
            cli: "4",
            toml: "4",
            summary: "4",
        },
        Case {
            path: "route.breakdown_limit",
            cli: "12",
            toml: "12",
            summary: "12",
        },
        Case {
            path: "route.emit_on_divergent_suspects",
            cli: "false",
            toml: "false",
            summary: "false",
        },
        Case {
            path: "route.slowest_to_fastest_p95_ratio_numerator",
            cli: "4",
            toml: "4",
            summary: "4",
        },
        Case {
            path: "route.slowest_to_fastest_p95_ratio_denominator",
            cli: "3",
            toml: "3",
            summary: "3",
        },
        Case {
            path: "route.slowest_to_global_p95_ratio_numerator",
            cli: "6",
            toml: "6",
            summary: "6",
        },
        Case {
            path: "route.slowest_to_global_p95_ratio_denominator",
            cli: "5",
            toml: "5",
            summary: "5",
        },
        Case {
            path: "temporal.min_request_count",
            cli: "24",
            toml: "24",
            summary: "24",
        },
        Case {
            path: "temporal.min_segment_request_count",
            cli: "9",
            toml: "9",
            summary: "9",
        },
        Case {
            path: "temporal.share_shift_permille",
            cli: "250",
            toml: "250",
            summary: "250",
        },
        Case {
            path: "temporal.p95_shift_ratio_numerator",
            cli: "4",
            toml: "4",
            summary: "4",
        },
        Case {
            path: "temporal.p95_shift_ratio_denominator",
            cli: "3",
            toml: "3",
            summary: "3",
        },
        Case {
            path: "temporal.emit_on_suspect_shift",
            cli: "false",
            toml: "false",
            summary: "false",
        },
        Case {
            path: "temporal.suppress_runtime_sparse_suspect_shift_without_supporting_movement",
            cli: "false",
            toml: "false",
            summary: "false",
        },
    ];

    fn toml_for(path: &str, value: &str) -> String {
        let (group, field) = path.split_once('.').expect("path has group");
        format!("[analyzer]\nschema_version = 1\n[analyzer.{group}]\n{field} = {value}\n")
    }

    #[test]
    fn registry_paths_are_unique_and_exact() {
        let paths = AnalyzeOptions::valid_override_paths();
        let unique: HashSet<_> = paths.iter().copied().collect();
        assert_eq!(paths.len(), 29);
        assert_eq!(paths.len(), unique.len());
        assert_eq!(paths, EXPECTED_PATHS);
    }

    #[test]
    fn descriptor_paths_and_valid_override_paths_are_identical() {
        let descriptor_paths: Vec<_> = analyze_option_descriptors()
            .iter()
            .map(|descriptor| descriptor.path)
            .collect();
        assert_eq!(descriptor_paths, AnalyzeOptions::valid_override_paths());
    }

    #[test]
    fn every_registered_path_accepts_equivalent_cli_and_toml_value() {
        assert_eq!(CASES.len(), 29);
        for case in CASES {
            let mut cli = AnalyzeOptions::default();
            cli.apply_override(&format!("{}={}", case.path, case.cli))
                .expect(case.path);
            let toml =
                AnalyzeOptions::from_toml_str(&toml_for(case.path, case.toml)).expect(case.path);
            assert_eq!(cli, toml, "{}", case.path);
        }
    }

    #[test]
    fn every_registered_path_produces_one_expected_summary_entry() {
        for case in CASES {
            let mut options = AnalyzeOptions::default();
            options
                .apply_override(&format!("{}={}", case.path, case.cli))
                .expect(case.path);
            let summaries = options.non_default_overrides();
            assert_eq!(summaries.len(), 1, "{}", case.path);
            assert_eq!(summaries[0].path, case.path);
            assert_eq!(summaries[0].value, case.summary);
        }
    }

    #[test]
    fn multiple_non_default_summaries_are_deterministically_sorted() {
        let mut options = AnalyzeOptions::default();
        options
            .apply_overrides([
                "temporal.share_shift_permille=250",
                "queueing.trigger_permille=250",
                "blocking.strong_p95_threshold=15",
            ])
            .expect("valid overrides");
        let paths: Vec<_> = options
            .non_default_overrides()
            .into_iter()
            .map(|summary| summary.path)
            .collect();
        assert_eq!(
            paths,
            vec![
                "blocking.strong_p95_threshold",
                "queueing.trigger_permille",
                "temporal.share_shift_permille",
            ]
        );
    }

    #[test]
    fn toml_unknown_groups_and_fields_remain_errors() {
        assert!(AnalyzeOptions::from_toml_str(
            "[analyzer]\nschema_version=1\n[analyzer.nope]\nfield=1\n"
        )
        .is_err());
        assert!(AnalyzeOptions::from_toml_str(
            "[analyzer]\nschema_version=1\n[analyzer.queueing]\nunknown=1\n"
        )
        .is_err());
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
    fn toml_string_list_preserves_commas_inside_items() {
        let options = AnalyzeOptions::from_toml_str(
            "[analyzer]\nschema_version=1\n[analyzer.downstream]\nblocking_correlated_stage_patterns = ['db,primary', 'cache']\n",
        )
        .expect("valid typed TOML list");
        assert_eq!(
            options.downstream.blocking_correlated_stage_patterns,
            vec!["db,primary", "cache"]
        );
    }

    #[test]
    fn descriptor_defaults_and_value_types_remain_public_contract() {
        let actual: Vec<_> = analyze_option_descriptors()
            .iter()
            .map(|d| (d.path, d.default_value, d.value_type))
            .collect();
        let expected: Vec<_> = [
            ("queueing.trigger_permille", "300", "u64"),
            ("blocking.min_nonzero_samples_for_signal", "2", "usize"),
            ("blocking.strong_p95_threshold", "12", "u64"),
            ("blocking.strong_peak_threshold", "20", "u64"),
            ("blocking.strong_nonzero_share_permille", "700", "u64"),
            ("blocking.strong_min_samples", "30", "usize"),
            ("executor.min_global_queue_p95_for_signal", "1", "u64"),
            ("downstream.min_stage_samples", "3", "usize"),
            (
                "downstream.blocking_correlated_stage_patterns",
                "[\"spawn_blocking\", \"blocking_path\", \"blocking\"]",
                "Vec<String>",
            ),
            ("downstream.blocking_correlation_score_margin", "2", "u8"),
            ("confidence.medium_score_threshold", "65", "u8"),
            ("confidence.high_score_threshold", "85", "u8"),
            ("confidence.ambiguity_min_score", "60", "u8"),
            ("confidence.ambiguity_score_gap", "4", "u8"),
            ("evidence.low_completed_request_threshold", "20", "usize"),
            ("route.min_request_count", "3", "usize"),
            ("route.breakdown_limit", "10", "usize"),
            ("route.emit_on_divergent_suspects", "true", "bool"),
            ("route.slowest_to_fastest_p95_ratio_numerator", "3", "u64"),
            ("route.slowest_to_fastest_p95_ratio_denominator", "2", "u64"),
            ("route.slowest_to_global_p95_ratio_numerator", "5", "u64"),
            ("route.slowest_to_global_p95_ratio_denominator", "4", "u64"),
            ("temporal.min_request_count", "20", "usize"),
            ("temporal.min_segment_request_count", "8", "usize"),
            ("temporal.share_shift_permille", "200", "u64"),
            ("temporal.p95_shift_ratio_numerator", "3", "u64"),
            ("temporal.p95_shift_ratio_denominator", "2", "u64"),
            ("temporal.emit_on_suspect_shift", "true", "bool"),
            (
                "temporal.suppress_runtime_sparse_suspect_shift_without_supporting_movement",
                "true",
                "bool",
            ),
        ]
        .to_vec();
        assert_eq!(actual, expected);
    }

    #[test]
    fn missing_equals_is_invalid_override_syntax() {
        let err = AnalyzeOptions::default()
            .apply_override("queueing.trigger_permille")
            .expect_err("missing equals must fail");
        assert_eq!(
            err,
            AnalyzeConfigError::InvalidOverrideSyntax {
                raw: "queueing.trigger_permille".to_string(),
            }
        );
    }

    #[test]
    fn multiple_equals_is_invalid_override_syntax() {
        let err = AnalyzeOptions::default()
            .apply_override("queueing.trigger_permille=1=2")
            .expect_err("multiple equals must fail");
        assert_eq!(
            err,
            AnalyzeConfigError::InvalidOverrideSyntax {
                raw: "queueing.trigger_permille=1=2".to_string(),
            }
        );
    }

    #[test]
    fn unknown_path_returns_unknown_override_path() {
        let err = AnalyzeOptions::default()
            .apply_override("queueing.nope=1")
            .expect_err("unknown path must fail");
        assert_eq!(
            err,
            AnalyzeConfigError::UnknownOverridePath {
                path: "queueing.nope".to_string(),
                suggestion: None,
            }
        );
    }

    #[test]
    fn negative_unsigned_override_is_invalid_override_value() {
        let err = AnalyzeOptions::default()
            .apply_override("queueing.trigger_permille=-1")
            .expect_err("negative unsigned value must fail");
        assert!(matches!(
            err,
            AnalyzeConfigError::InvalidOverrideValue {
                path: "queueing.trigger_permille",
                value,
                expected: "base-10 unsigned integer (u64)",
            } if value == "-1"
        ));
    }

    #[test]
    fn u8_overflow_is_invalid_override_value() {
        let err = AnalyzeOptions::default()
            .apply_override("confidence.high_score_threshold=256")
            .expect_err("u8 overflow must fail");
        assert!(matches!(
            err,
            AnalyzeConfigError::InvalidOverrideValue {
                path: "confidence.high_score_threshold",
                value,
                expected: "base-10 unsigned integer in range 0..=255 (u8)",
            } if value == "256"
        ));
    }

    #[test]
    fn invalid_bool_text_is_invalid_override_value() {
        let err = AnalyzeOptions::default()
            .apply_override("route.emit_on_divergent_suspects=yes")
            .expect_err("invalid bool must fail");
        assert!(matches!(
            err,
            AnalyzeConfigError::InvalidOverrideValue {
                path: "route.emit_on_divergent_suspects",
                value,
                expected: "'true' or 'false'",
            } if value == "yes"
        ));
    }

    #[test]
    fn comma_separated_cli_lists_trim_entries() {
        let mut options = AnalyzeOptions::default();
        options
            .apply_override("downstream.blocking_correlated_stage_patterns=db, cache ,worker")
            .expect("valid list");
        assert_eq!(
            options.downstream.blocking_correlated_stage_patterns,
            vec!["db", "cache", "worker"]
        );
    }

    #[test]
    fn empty_cli_list_entries_are_rejected() {
        let err = AnalyzeOptions::default()
            .apply_override("downstream.blocking_correlated_stage_patterns=db,,cache")
            .expect_err("empty list entry must fail");
        assert!(matches!(
            err,
            AnalyzeConfigError::InvalidOverrideValue {
                path: "downstream.blocking_correlated_stage_patterns",
                value,
                expected: "comma-separated non-empty entries (Vec<String>)",
            } if value == "db,,cache"
        ));
    }

    #[test]
    fn repeated_valid_overrides_use_last_value() {
        let mut options = AnalyzeOptions::default();
        options
            .apply_overrides([
                "queueing.trigger_permille=250",
                "queueing.trigger_permille=450",
            ])
            .expect("valid repeated overrides");
        assert_eq!(options.queueing.trigger_permille, 450);
    }

    #[test]
    fn semantically_invalid_single_override_leaves_original_options_unchanged() {
        let mut options = AnalyzeOptions::default();
        let before = options.clone();
        let err = options
            .apply_override("queueing.trigger_permille=1001")
            .expect_err("semantic validation must fail");
        assert!(matches!(
            err,
            AnalyzeConfigError::InvalidConfigValue {
                path: "queueing.trigger_permille",
                ..
            }
        ));
        assert_eq!(options, before);
    }

    #[test]
    fn invalid_path_leaves_original_options_unchanged() {
        let mut options = AnalyzeOptions::default();
        let before = options.clone();
        let err = options
            .apply_override("bad.path=1")
            .expect_err("invalid path must fail");
        assert!(matches!(
            err,
            AnalyzeConfigError::UnknownOverridePath { .. }
        ));
        assert_eq!(options, before);
    }

    #[test]
    fn batch_processing_stops_without_applying_later_entries() {
        let mut options = AnalyzeOptions::default();
        let before = options.clone();
        let err = options
            .apply_overrides([
                "queueing.trigger_permille=450",
                "queueing.trigger_permille=1001",
                "confidence.high_score_threshold=90",
            ])
            .expect_err("later invalid item must fail the batch");
        assert!(matches!(
            err,
            AnalyzeConfigError::InvalidConfigValue {
                path: "queueing.trigger_permille",
                ..
            }
        ));
        assert_eq!(options, before);
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
}
