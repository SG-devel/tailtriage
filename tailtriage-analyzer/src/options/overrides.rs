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

type Applier = fn(&mut AnalyzeOptions, &str) -> Result<(), AnalyzeConfigError>;

impl AnalyzeOptions {
    /// Applies one `group.field=value` override and validates the resulting options.
    ///
    /// # Errors
    /// Returns [`AnalyzeConfigError`] for invalid syntax, unknown paths, parse failures, or semantic validation failures.
    pub fn apply_override(&mut self, raw: &str) -> Result<(), AnalyzeConfigError> {
        let Some((path, value)) = raw.split_once('=') else {
            return Err(AnalyzeConfigError::InvalidOverrideSyntax {
                raw: raw.to_string(),
            });
        };
        let Some(applier) = applier_for_path(path) else {
            return Err(AnalyzeConfigError::UnknownOverridePath {
                path: path.to_string(),
                suggestion: nearest_override_path(path),
            });
        };
        applier(self, value)?;
        self.validate()
    }

    /// Applies overrides in order and stops at the first error.
    ///
    /// # Errors
    /// Returns the first [`AnalyzeConfigError`] encountered while applying ordered overrides.
    pub fn apply_overrides<I, S>(&mut self, overrides: I) -> Result<(), AnalyzeConfigError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        for raw in overrides {
            self.apply_override(raw.as_ref())?;
        }
        Ok(())
    }

    /// Returns all valid analyzer override paths.
    #[must_use]
    pub fn valid_override_paths() -> &'static [&'static str] {
        &VALID_OVERRIDE_PATHS
    }
}

#[allow(clippy::too_many_lines)]
fn applier_for_path(path: &str) -> Option<Applier> {
    Some(match path {
        "queueing.trigger_permille" => |o, v| {
            o.queueing.trigger_permille = parse_u64("queueing.trigger_permille", v)?;
            Ok(())
        },
        "blocking.min_nonzero_samples_for_signal" => |o, v| {
            o.blocking.min_nonzero_samples_for_signal =
                parse_usize("blocking.min_nonzero_samples_for_signal", v)?;
            Ok(())
        },
        "blocking.strong_p95_threshold" => |o, v| {
            o.blocking.strong_p95_threshold = parse_u64("blocking.strong_p95_threshold", v)?;
            Ok(())
        },
        "blocking.strong_peak_threshold" => |o, v| {
            o.blocking.strong_peak_threshold = parse_u64("blocking.strong_peak_threshold", v)?;
            Ok(())
        },
        "blocking.strong_nonzero_share_permille" => |o, v| {
            o.blocking.strong_nonzero_share_permille =
                parse_u64("blocking.strong_nonzero_share_permille", v)?;
            Ok(())
        },
        "blocking.strong_min_samples" => |o, v| {
            o.blocking.strong_min_samples = parse_usize("blocking.strong_min_samples", v)?;
            Ok(())
        },
        "executor.min_global_queue_p95_for_signal" => |o, v| {
            o.executor.min_global_queue_p95_for_signal =
                parse_u64("executor.min_global_queue_p95_for_signal", v)?;
            Ok(())
        },
        "downstream.min_stage_samples" => |o, v| {
            o.downstream.min_stage_samples = parse_usize("downstream.min_stage_samples", v)?;
            Ok(())
        },
        "downstream.blocking_correlated_stage_patterns" => |o, v| {
            o.downstream.blocking_correlated_stage_patterns =
                parse_string_list("downstream.blocking_correlated_stage_patterns", v)?;
            Ok(())
        },
        "downstream.blocking_correlation_score_margin" => |o, v| {
            o.downstream.blocking_correlation_score_margin =
                parse_u8("downstream.blocking_correlation_score_margin", v)?;
            Ok(())
        },
        "confidence.medium_score_threshold" => |o, v| {
            o.confidence.medium_score_threshold = parse_u8("confidence.medium_score_threshold", v)?;
            Ok(())
        },
        "confidence.high_score_threshold" => |o, v| {
            o.confidence.high_score_threshold = parse_u8("confidence.high_score_threshold", v)?;
            Ok(())
        },
        "confidence.ambiguity_min_score" => |o, v| {
            o.confidence.ambiguity_min_score = parse_u8("confidence.ambiguity_min_score", v)?;
            Ok(())
        },
        "confidence.ambiguity_score_gap" => |o, v| {
            o.confidence.ambiguity_score_gap = parse_u8("confidence.ambiguity_score_gap", v)?;
            Ok(())
        },
        "evidence.low_completed_request_threshold" => |o, v| {
            o.evidence.low_completed_request_threshold =
                parse_usize("evidence.low_completed_request_threshold", v)?;
            Ok(())
        },
        "route.min_request_count" => |o, v| {
            o.route.min_request_count = parse_usize("route.min_request_count", v)?;
            Ok(())
        },
        "route.breakdown_limit" => |o, v| {
            o.route.breakdown_limit = parse_usize("route.breakdown_limit", v)?;
            Ok(())
        },
        "route.emit_on_divergent_suspects" => |o, v| {
            o.route.emit_on_divergent_suspects = parse_bool("route.emit_on_divergent_suspects", v)?;
            Ok(())
        },
        "route.slowest_to_fastest_p95_ratio_numerator" => |o, v| {
            o.route.slowest_to_fastest_p95_ratio_numerator =
                parse_u64("route.slowest_to_fastest_p95_ratio_numerator", v)?;
            Ok(())
        },
        "route.slowest_to_fastest_p95_ratio_denominator" => |o, v| {
            o.route.slowest_to_fastest_p95_ratio_denominator =
                parse_u64("route.slowest_to_fastest_p95_ratio_denominator", v)?;
            Ok(())
        },
        "route.slowest_to_global_p95_ratio_numerator" => |o, v| {
            o.route.slowest_to_global_p95_ratio_numerator =
                parse_u64("route.slowest_to_global_p95_ratio_numerator", v)?;
            Ok(())
        },
        "route.slowest_to_global_p95_ratio_denominator" => |o, v| {
            o.route.slowest_to_global_p95_ratio_denominator =
                parse_u64("route.slowest_to_global_p95_ratio_denominator", v)?;
            Ok(())
        },
        "temporal.min_request_count" => |o, v| {
            o.temporal.min_request_count = parse_usize("temporal.min_request_count", v)?;
            Ok(())
        },
        "temporal.min_segment_request_count" => |o, v| {
            o.temporal.min_segment_request_count =
                parse_usize("temporal.min_segment_request_count", v)?;
            Ok(())
        },
        "temporal.share_shift_permille" => |o, v| {
            o.temporal.share_shift_permille = parse_u64("temporal.share_shift_permille", v)?;
            Ok(())
        },
        "temporal.p95_shift_ratio_numerator" => |o, v| {
            o.temporal.p95_shift_ratio_numerator =
                parse_u64("temporal.p95_shift_ratio_numerator", v)?;
            Ok(())
        },
        "temporal.p95_shift_ratio_denominator" => |o, v| {
            o.temporal.p95_shift_ratio_denominator =
                parse_u64("temporal.p95_shift_ratio_denominator", v)?;
            Ok(())
        },
        "temporal.emit_on_suspect_shift" => |o, v| {
            o.temporal.emit_on_suspect_shift = parse_bool("temporal.emit_on_suspect_shift", v)?;
            Ok(())
        },
        "temporal.suppress_runtime_sparse_suspect_shift_without_supporting_movement" => {
            |o, v| {
                o.temporal.suppress_runtime_sparse_suspect_shift_without_supporting_movement=parse_bool("temporal.suppress_runtime_sparse_suspect_shift_without_supporting_movement", v)?;
                Ok(())
            }
        }
        _ => return None,
    })
}
fn parse_u64(path: &'static str, v: &str) -> Result<u64, AnalyzeConfigError> {
    v.parse()
        .map_err(|_| AnalyzeConfigError::InvalidOverrideValue {
            path,
            value: v.to_string(),
            expected: "unsigned integer (u64)",
        })
}
fn parse_usize(path: &'static str, v: &str) -> Result<usize, AnalyzeConfigError> {
    v.parse()
        .map_err(|_| AnalyzeConfigError::InvalidOverrideValue {
            path,
            value: v.to_string(),
            expected: "unsigned integer (usize)",
        })
}
fn parse_u8(path: &'static str, v: &str) -> Result<u8, AnalyzeConfigError> {
    v.parse()
        .map_err(|_| AnalyzeConfigError::InvalidOverrideValue {
            path,
            value: v.to_string(),
            expected: "unsigned integer (u8, 0..=255)",
        })
}
fn parse_bool(path: &'static str, v: &str) -> Result<bool, AnalyzeConfigError> {
    match v {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(AnalyzeConfigError::InvalidOverrideValue {
            path,
            value: v.to_string(),
            expected: "boolean literal 'true' or 'false'",
        }),
    }
}
fn parse_string_list(path: &'static str, v: &str) -> Result<Vec<String>, AnalyzeConfigError> {
    let mut out = Vec::new();
    for e in v.split(',') {
        let t = e.trim();
        if t.is_empty() {
            return Err(AnalyzeConfigError::InvalidOverrideValue {
                path,
                value: v.to_string(),
                expected: "comma-separated non-empty entries",
            });
        }
        out.push(t.to_string());
    }
    Ok(out)
}
fn nearest_override_path(path: &str) -> Option<&'static str> {
    let mut best = None;
    for c in VALID_OVERRIDE_PATHS {
        let d = edit_distance(path, c);
        if best.is_none_or(|(_, bd)| d < bd) {
            best = Some((c, d));
        }
    }
    best.and_then(|(c, d)| (d <= 3).then_some(c))
}
fn edit_distance(a: &str, b: &str) -> usize {
    let (a, b) = (a.chars().collect::<Vec<_>>(), b.chars().collect::<Vec<_>>());
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur[j + 1] = (prev[j + 1] + 1).min(cur[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}
