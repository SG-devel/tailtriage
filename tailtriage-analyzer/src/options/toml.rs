use super::{
    AnalyzeConfigError, AnalyzeOptions, BlockingOptions, ConfidenceOptions, DownstreamOptions,
    EvidenceOptions, ExecutorOptions, QueueingOptions, RouteOptions, TemporalOptions,
};
use serde::Deserialize;

const ANALYZER_SCHEMA_VERSION: u64 = 1;

#[derive(Debug, Deserialize)]
struct AnalyzerTomlRoot {
    analyzer: Option<AnalyzerToml>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AnalyzerToml {
    schema_version: Option<u64>,
    queueing: Option<QueueingToml>,
    blocking: Option<BlockingToml>,
    executor: Option<ExecutorToml>,
    downstream: Option<DownstreamToml>,
    confidence: Option<ConfidenceToml>,
    evidence: Option<EvidenceToml>,
    route: Option<RouteToml>,
    temporal: Option<TemporalToml>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct QueueingToml {
    trigger_permille: Option<u64>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BlockingToml {
    min_nonzero_samples_for_signal: Option<usize>,
    strong_p95_threshold: Option<u64>,
    strong_peak_threshold: Option<u64>,
    strong_nonzero_share_permille: Option<u64>,
    strong_min_samples: Option<usize>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExecutorToml {
    min_global_queue_p95_for_signal: Option<u64>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DownstreamToml {
    min_stage_samples: Option<usize>,
    blocking_correlated_stage_patterns: Option<Vec<String>>,
    blocking_correlation_score_margin: Option<u8>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfidenceToml {
    medium_score_threshold: Option<u8>,
    high_score_threshold: Option<u8>,
    ambiguity_min_score: Option<u8>,
    ambiguity_score_gap: Option<u8>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EvidenceToml {
    low_completed_request_threshold: Option<usize>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RouteToml {
    min_request_count: Option<usize>,
    breakdown_limit: Option<usize>,
    emit_on_divergent_suspects: Option<bool>,
    slowest_to_fastest_p95_ratio_numerator: Option<u64>,
    slowest_to_fastest_p95_ratio_denominator: Option<u64>,
    slowest_to_global_p95_ratio_numerator: Option<u64>,
    slowest_to_global_p95_ratio_denominator: Option<u64>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TemporalToml {
    min_request_count: Option<usize>,
    min_segment_request_count: Option<usize>,
    share_shift_permille: Option<u64>,
    p95_shift_ratio_numerator: Option<u64>,
    p95_shift_ratio_denominator: Option<u64>,
    emit_on_suspect_shift: Option<bool>,
    suppress_runtime_sparse_suspect_shift_without_supporting_movement: Option<bool>,
}

impl AnalyzeOptions {
    /// Parses analyzer-specific TOML from `[analyzer]` and validates the merged options.
    ///
    /// # Errors
    ///
    /// Returns [`AnalyzeConfigError`] when schema fields are missing/unsupported, TOML is invalid,
    /// or merged values fail semantic validation.
    pub fn from_toml_str(input: &str) -> Result<Self, AnalyzeConfigError> {
        AnalyzeOptions::default().merge_toml_str(input)
    }

    /// Merges analyzer-specific TOML from `[analyzer]` over existing options and validates the result.
    ///
    /// # Errors
    ///
    /// Returns [`AnalyzeConfigError`] when schema fields are missing/unsupported, TOML is invalid,
    /// or merged values fail semantic validation.
    pub fn merge_toml_str(mut self, input: &str) -> Result<Self, AnalyzeConfigError> {
        let root: AnalyzerTomlRoot =
            toml::from_str(input).map_err(|e| AnalyzeConfigError::InvalidToml {
                message: e.to_string(),
            })?;
        let analyzer = root
            .analyzer
            .ok_or(AnalyzeConfigError::MissingAnalyzerTable)?;
        let schema_version = analyzer
            .schema_version
            .ok_or(AnalyzeConfigError::MissingSchemaVersion)?;
        if schema_version != ANALYZER_SCHEMA_VERSION {
            return Err(AnalyzeConfigError::UnsupportedSchemaVersion {
                found: schema_version,
                supported: ANALYZER_SCHEMA_VERSION,
            });
        }

        if let Some(v) = analyzer.queueing.as_ref() {
            apply_queueing(&mut self.queueing, v);
        }
        if let Some(v) = analyzer.blocking.as_ref() {
            apply_blocking(&mut self.blocking, v);
        }
        if let Some(v) = analyzer.executor.as_ref() {
            apply_executor(&mut self.executor, v);
        }
        if let Some(v) = analyzer.downstream.as_ref() {
            apply_downstream(&mut self.downstream, v);
        }
        if let Some(v) = analyzer.confidence.as_ref() {
            apply_confidence(&mut self.confidence, v);
        }
        if let Some(v) = analyzer.evidence.as_ref() {
            apply_evidence(&mut self.evidence, v);
        }
        if let Some(v) = analyzer.route.as_ref() {
            apply_route(&mut self.route, v);
        }
        if let Some(v) = analyzer.temporal.as_ref() {
            apply_temporal(&mut self.temporal, v);
        }
        self.validate()?;
        Ok(self)
    }
}

fn apply_queueing(dst: &mut QueueingOptions, src: &QueueingToml) {
    if let Some(v) = src.trigger_permille {
        dst.trigger_permille = v;
    }
}
fn apply_blocking(dst: &mut BlockingOptions, src: &BlockingToml) {
    if let Some(v) = src.min_nonzero_samples_for_signal {
        dst.min_nonzero_samples_for_signal = v;
    }
    if let Some(v) = src.strong_p95_threshold {
        dst.strong_p95_threshold = v;
    }
    if let Some(v) = src.strong_peak_threshold {
        dst.strong_peak_threshold = v;
    }
    if let Some(v) = src.strong_nonzero_share_permille {
        dst.strong_nonzero_share_permille = v;
    }
    if let Some(v) = src.strong_min_samples {
        dst.strong_min_samples = v;
    }
}
fn apply_executor(dst: &mut ExecutorOptions, src: &ExecutorToml) {
    if let Some(v) = src.min_global_queue_p95_for_signal {
        dst.min_global_queue_p95_for_signal = v;
    }
}
fn apply_downstream(dst: &mut DownstreamOptions, src: &DownstreamToml) {
    if let Some(v) = src.min_stage_samples {
        dst.min_stage_samples = v;
    }
    if let Some(v) = &src.blocking_correlated_stage_patterns {
        dst.blocking_correlated_stage_patterns.clone_from(v);
    }
    if let Some(v) = src.blocking_correlation_score_margin {
        dst.blocking_correlation_score_margin = v;
    }
}
fn apply_confidence(dst: &mut ConfidenceOptions, src: &ConfidenceToml) {
    if let Some(v) = src.medium_score_threshold {
        dst.medium_score_threshold = v;
    }
    if let Some(v) = src.high_score_threshold {
        dst.high_score_threshold = v;
    }
    if let Some(v) = src.ambiguity_min_score {
        dst.ambiguity_min_score = v;
    }
    if let Some(v) = src.ambiguity_score_gap {
        dst.ambiguity_score_gap = v;
    }
}
fn apply_evidence(dst: &mut EvidenceOptions, src: &EvidenceToml) {
    if let Some(v) = src.low_completed_request_threshold {
        dst.low_completed_request_threshold = v;
    }
}
fn apply_route(dst: &mut RouteOptions, src: &RouteToml) {
    if let Some(v) = src.min_request_count {
        dst.min_request_count = v;
    }
    if let Some(v) = src.breakdown_limit {
        dst.breakdown_limit = v;
    }
    if let Some(v) = src.emit_on_divergent_suspects {
        dst.emit_on_divergent_suspects = v;
    }
    if let Some(v) = src.slowest_to_fastest_p95_ratio_numerator {
        dst.slowest_to_fastest_p95_ratio_numerator = v;
    }
    if let Some(v) = src.slowest_to_fastest_p95_ratio_denominator {
        dst.slowest_to_fastest_p95_ratio_denominator = v;
    }
    if let Some(v) = src.slowest_to_global_p95_ratio_numerator {
        dst.slowest_to_global_p95_ratio_numerator = v;
    }
    if let Some(v) = src.slowest_to_global_p95_ratio_denominator {
        dst.slowest_to_global_p95_ratio_denominator = v;
    }
}
fn apply_temporal(dst: &mut TemporalOptions, src: &TemporalToml) {
    if let Some(v) = src.min_request_count {
        dst.min_request_count = v;
    }
    if let Some(v) = src.min_segment_request_count {
        dst.min_segment_request_count = v;
    }
    if let Some(v) = src.share_shift_permille {
        dst.share_shift_permille = v;
    }
    if let Some(v) = src.p95_shift_ratio_numerator {
        dst.p95_shift_ratio_numerator = v;
    }
    if let Some(v) = src.p95_shift_ratio_denominator {
        dst.p95_shift_ratio_denominator = v;
    }
    if let Some(v) = src.emit_on_suspect_shift {
        dst.emit_on_suspect_shift = v;
    }
    if let Some(v) = src.suppress_runtime_sparse_suspect_shift_without_supporting_movement {
        dst.suppress_runtime_sparse_suspect_shift_without_supporting_movement = v;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_analyzer_toml_parses() {
        let input = include_str!("../../../examples/analyzer-config.toml");
        let opts = AnalyzeOptions::from_toml_str(input).unwrap();
        assert_eq!(opts.queueing.trigger_permille, 400);
        assert_eq!(opts.temporal.min_segment_request_count, 8);
    }

    #[test]
    fn sparse_toml_preserves_defaults() {
        let input = "[analyzer]\nschema_version=1\n[analyzer.queueing]\ntrigger_permille=401\n";
        let opts = AnalyzeOptions::from_toml_str(input).unwrap();
        assert_eq!(opts.queueing.trigger_permille, 401);
        assert_eq!(opts.blocking, AnalyzeOptions::default().blocking);
    }

    #[test]
    fn merge_applies_over_base() {
        let base = AnalyzeOptions::default().with_queueing(|o| o.trigger_permille = 500);
        let merged = base
            .merge_toml_str(
                "[analyzer]\nschema_version=1\n[analyzer.queueing]\ntrigger_permille=410\n",
            )
            .unwrap();
        assert_eq!(merged.queueing.trigger_permille, 410);
    }

    #[test]
    fn missing_analyzer_fails() {
        assert_eq!(
            AnalyzeOptions::from_toml_str("[controller]\nx=1").unwrap_err(),
            AnalyzeConfigError::MissingAnalyzerTable
        );
    }
    #[test]
    fn missing_schema_version_fails() {
        assert_eq!(
            AnalyzeOptions::from_toml_str("[analyzer]\n").unwrap_err(),
            AnalyzeConfigError::MissingSchemaVersion
        );
    }
    #[test]
    fn unsupported_schema_fails() {
        assert_eq!(
            AnalyzeOptions::from_toml_str("[analyzer]\nschema_version=2").unwrap_err(),
            AnalyzeConfigError::UnsupportedSchemaVersion {
                found: 2,
                supported: 1
            }
        );
    }
    #[test]
    fn unknown_top_level_sibling_ignored() {
        assert!(AnalyzeOptions::from_toml_str(
            "[analyzer]\nschema_version=1\n[controller]\nmode='x'\n"
        )
        .is_ok());
    }
    #[test]
    fn unknown_field_under_analyzer_fails() {
        assert!(matches!(
            AnalyzeOptions::from_toml_str("[analyzer]\nschema_version=1\nfoo=1\n"),
            Err(AnalyzeConfigError::InvalidToml { .. })
        ));
    }
    #[test]
    fn unknown_subgroup_fails() {
        assert!(matches!(
            AnalyzeOptions::from_toml_str(
                "[analyzer]\nschema_version=1\n[analyzer.unknown]\na=1\n"
            ),
            Err(AnalyzeConfigError::InvalidToml { .. })
        ));
    }
    #[test]
    fn unknown_field_in_group_fails() {
        assert!(matches!(
            AnalyzeOptions::from_toml_str(
                "[analyzer]\nschema_version=1\n[analyzer.queueing]\nunknown=1\n"
            ),
            Err(AnalyzeConfigError::InvalidToml { .. })
        ));
    }
    #[test]
    fn invalid_type_fails() {
        assert!(matches!(
            AnalyzeOptions::from_toml_str(
                "[analyzer]\nschema_version=1\n[analyzer.queueing]\ntrigger_permille='bad'\n"
            ),
            Err(AnalyzeConfigError::InvalidToml { .. })
        ));
    }
    #[test]
    fn invalid_range_fails_validation() {
        assert!(matches!(
            AnalyzeOptions::from_toml_str(
                "[analyzer]\nschema_version=1\n[analyzer.queueing]\ntrigger_permille=1001\n"
            ),
            Err(AnalyzeConfigError::InvalidConfigValue { .. })
        ));
    }
    #[test]
    fn example_file_parses() {
        let input = include_str!("../../../examples/analyzer-config.toml");
        assert!(AnalyzeOptions::from_toml_str(input).is_ok());
    }
    #[test]
    fn downstream_pattern_list_parses() {
        let opts=AnalyzeOptions::from_toml_str("[analyzer]\nschema_version=1\n[analyzer.downstream]\nblocking_correlated_stage_patterns=['a','b']\n").unwrap();
        assert_eq!(
            opts.downstream.blocking_correlated_stage_patterns,
            vec!["a".to_string(), "b".to_string()]
        );
    }
    #[test]
    fn empty_pattern_fails_validation() {
        assert!(matches!(AnalyzeOptions::from_toml_str("[analyzer]\nschema_version=1\n[analyzer.downstream]\nblocking_correlated_stage_patterns=['']\n"),Err(AnalyzeConfigError::InvalidConfigValue{..})));
    }
}
