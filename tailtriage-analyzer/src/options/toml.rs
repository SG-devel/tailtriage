use super::{
    AnalyzeConfigError, AnalyzeOptions, BlockingOptions, ConfidenceOptions, DownstreamOptions,
    EvidenceOptions, ExecutorOptions, QueueingOptions, RouteOptions, TemporalOptions,
};
use serde::Deserialize;

const SUPPORTED_SCHEMA_VERSION: u64 = 1;

#[derive(Debug, Deserialize)]
struct AnalyzerTomlRoot {
    analyzer: Option<AnalyzerTomlConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AnalyzerTomlConfig {
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

fn parse_toml_config(input: &str) -> Result<AnalyzerTomlConfig, AnalyzeConfigError> {
    let root: AnalyzerTomlRoot =
        toml::from_str(input).map_err(|err| AnalyzeConfigError::InvalidToml {
            message: err.to_string(),
        })?;
    let analyzer = root
        .analyzer
        .ok_or(AnalyzeConfigError::MissingAnalyzerTable)?;
    let schema_version = analyzer
        .schema_version
        .ok_or(AnalyzeConfigError::MissingSchemaVersion)?;
    if schema_version != SUPPORTED_SCHEMA_VERSION {
        return Err(AnalyzeConfigError::UnsupportedSchemaVersion {
            found: schema_version,
            supported: SUPPORTED_SCHEMA_VERSION,
        });
    }
    Ok(analyzer)
}

fn apply_toml(mut options: AnalyzeOptions, parsed: AnalyzerTomlConfig) -> AnalyzeOptions {
    if let Some(ref queueing) = parsed.queueing {
        apply_queueing(&mut options.queueing, queueing);
    }
    if let Some(ref blocking) = parsed.blocking {
        apply_blocking(&mut options.blocking, blocking);
    }
    if let Some(ref executor) = parsed.executor {
        apply_executor(&mut options.executor, executor);
    }
    if let Some(downstream) = parsed.downstream {
        apply_downstream(&mut options.downstream, downstream);
    }
    if let Some(ref confidence) = parsed.confidence {
        apply_confidence(&mut options.confidence, confidence);
    }
    if let Some(ref evidence) = parsed.evidence {
        apply_evidence(&mut options.evidence, evidence);
    }
    if let Some(ref route) = parsed.route {
        apply_route(&mut options.route, route);
    }
    if let Some(ref temporal) = parsed.temporal {
        apply_temporal(&mut options.temporal, temporal);
    }
    options
}

impl AnalyzeOptions {
    /// Parses analyzer TOML configuration from a string and returns validated options.
    ///
    /// # Errors
    ///
    /// Returns [`AnalyzeConfigError`] when parsing, schema checks, or semantic validation fails.
    pub fn from_toml_str(input: &str) -> Result<Self, AnalyzeConfigError> {
        AnalyzeOptions::default().merge_toml_str(input)
    }

    /// Merges analyzer TOML configuration from a string onto existing options and validates the result.
    ///
    /// # Errors
    ///
    /// Returns [`AnalyzeConfigError`] when parsing, schema checks, or semantic validation fails.
    pub fn merge_toml_str(self, input: &str) -> Result<Self, AnalyzeConfigError> {
        let parsed = parse_toml_config(input)?;
        let merged = apply_toml(self, parsed);
        merged.validate()?;
        Ok(merged)
    }
}

fn apply_queueing(base: &mut QueueingOptions, parsed: &QueueingToml) {
    if let Some(v) = parsed.trigger_permille {
        base.trigger_permille = v;
    }
}
fn apply_blocking(base: &mut BlockingOptions, parsed: &BlockingToml) {
    if let Some(v) = parsed.min_nonzero_samples_for_signal {
        base.min_nonzero_samples_for_signal = v;
    }
    if let Some(v) = parsed.strong_p95_threshold {
        base.strong_p95_threshold = v;
    }
    if let Some(v) = parsed.strong_peak_threshold {
        base.strong_peak_threshold = v;
    }
    if let Some(v) = parsed.strong_nonzero_share_permille {
        base.strong_nonzero_share_permille = v;
    }
    if let Some(v) = parsed.strong_min_samples {
        base.strong_min_samples = v;
    }
}
fn apply_executor(base: &mut ExecutorOptions, parsed: &ExecutorToml) {
    if let Some(v) = parsed.min_global_queue_p95_for_signal {
        base.min_global_queue_p95_for_signal = v;
    }
}
fn apply_downstream(base: &mut DownstreamOptions, parsed: DownstreamToml) {
    if let Some(v) = parsed.min_stage_samples {
        base.min_stage_samples = v;
    }
    if let Some(v) = parsed.blocking_correlated_stage_patterns {
        base.blocking_correlated_stage_patterns = v;
    }
    if let Some(v) = parsed.blocking_correlation_score_margin {
        base.blocking_correlation_score_margin = v;
    }
}
fn apply_confidence(base: &mut ConfidenceOptions, parsed: &ConfidenceToml) {
    if let Some(v) = parsed.medium_score_threshold {
        base.medium_score_threshold = v;
    }
    if let Some(v) = parsed.high_score_threshold {
        base.high_score_threshold = v;
    }
    if let Some(v) = parsed.ambiguity_min_score {
        base.ambiguity_min_score = v;
    }
    if let Some(v) = parsed.ambiguity_score_gap {
        base.ambiguity_score_gap = v;
    }
}
fn apply_evidence(base: &mut EvidenceOptions, parsed: &EvidenceToml) {
    if let Some(v) = parsed.low_completed_request_threshold {
        base.low_completed_request_threshold = v;
    }
}
fn apply_route(base: &mut RouteOptions, parsed: &RouteToml) {
    if let Some(v) = parsed.min_request_count {
        base.min_request_count = v;
    }
    if let Some(v) = parsed.breakdown_limit {
        base.breakdown_limit = v;
    }
    if let Some(v) = parsed.emit_on_divergent_suspects {
        base.emit_on_divergent_suspects = v;
    }
    if let Some(v) = parsed.slowest_to_fastest_p95_ratio_numerator {
        base.slowest_to_fastest_p95_ratio_numerator = v;
    }
    if let Some(v) = parsed.slowest_to_fastest_p95_ratio_denominator {
        base.slowest_to_fastest_p95_ratio_denominator = v;
    }
    if let Some(v) = parsed.slowest_to_global_p95_ratio_numerator {
        base.slowest_to_global_p95_ratio_numerator = v;
    }
    if let Some(v) = parsed.slowest_to_global_p95_ratio_denominator {
        base.slowest_to_global_p95_ratio_denominator = v;
    }
}
fn apply_temporal(base: &mut TemporalOptions, parsed: &TemporalToml) {
    if let Some(v) = parsed.min_request_count {
        base.min_request_count = v;
    }
    if let Some(v) = parsed.min_segment_request_count {
        base.min_segment_request_count = v;
    }
    if let Some(v) = parsed.share_shift_permille {
        base.share_shift_permille = v;
    }
    if let Some(v) = parsed.p95_shift_ratio_numerator {
        base.p95_shift_ratio_numerator = v;
    }
    if let Some(v) = parsed.p95_shift_ratio_denominator {
        base.p95_shift_ratio_denominator = v;
    }
    if let Some(v) = parsed.emit_on_suspect_shift {
        base.emit_on_suspect_shift = v;
    }
    if let Some(v) = parsed.suppress_runtime_sparse_suspect_shift_without_supporting_movement {
        base.suppress_runtime_sparse_suspect_shift_without_supporting_movement = v;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_analyzer_toml_parses() {
        let input = include_str!("../../../examples/analyzer-config.toml");
        let parsed = AnalyzeOptions::from_toml_str(input).expect("parse full analyzer toml");
        assert_eq!(parsed.queueing.trigger_permille, 400);
        assert_eq!(
            parsed.downstream.blocking_correlated_stage_patterns,
            vec!["spawn_blocking", "blocking", "sync_io"]
        );
    }

    #[test]
    fn sparse_analyzer_toml_preserves_defaults() {
        let input = "[analyzer]\nschema_version=1\n[analyzer.queueing]\ntrigger_permille=450\n";
        let parsed = AnalyzeOptions::from_toml_str(input).expect("parse sparse");
        let defaults = AnalyzeOptions::default();
        assert_eq!(parsed.queueing.trigger_permille, 450);
        assert_eq!(parsed.blocking, defaults.blocking);
    }

    #[test]
    fn merge_toml_str_applies_over_non_default_base() {
        let base = AnalyzeOptions::default().with_queueing(|q| q.trigger_permille = 600);
        let merged = base
            .merge_toml_str("[analyzer]\nschema_version=1\n[analyzer.executor]\nmin_global_queue_p95_for_signal=3\n")
            .expect("merge");
        assert_eq!(merged.queueing.trigger_permille, 600);
        assert_eq!(merged.executor.min_global_queue_p95_for_signal, 3);
    }

    #[test]
    fn missing_analyzer_fails() {
        assert_eq!(
            AnalyzeOptions::from_toml_str("[controller]\nmode='x'\n").unwrap_err(),
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
    fn unsupported_schema_version_fails() {
        assert_eq!(
            AnalyzeOptions::from_toml_str("[analyzer]\nschema_version=2\n").unwrap_err(),
            AnalyzeConfigError::UnsupportedSchemaVersion {
                found: 2,
                supported: 1
            }
        );
    }

    #[test]
    fn unknown_top_level_sibling_is_ignored() {
        let parsed = AnalyzeOptions::from_toml_str(
            "[controller]\nmode='light'\n[analyzer]\nschema_version=1\n",
        )
        .expect("parse with sibling");
        assert_eq!(parsed, AnalyzeOptions::default());
    }

    #[test]
    fn unknown_field_under_analyzer_fails() {
        let err =
            AnalyzeOptions::from_toml_str("[analyzer]\nschema_version=1\nunknown_direct = true\n")
                .unwrap_err();
        assert!(matches!(err, AnalyzeConfigError::InvalidToml { .. }));
    }

    #[test]
    fn unknown_analyzer_subgroup_fails() {
        let err = AnalyzeOptions::from_toml_str(
            "[analyzer]\nschema_version=1\n[analyzer.unknown]\nvalue=1\n",
        )
        .unwrap_err();
        assert!(matches!(err, AnalyzeConfigError::InvalidToml { .. }));
    }

    #[test]
    fn unknown_field_in_known_subgroup_fails() {
        let err = AnalyzeOptions::from_toml_str(
            "[analyzer]\nschema_version=1\n[analyzer.queueing]\nunknown=1\n",
        )
        .unwrap_err();
        assert!(matches!(err, AnalyzeConfigError::InvalidToml { .. }));
    }

    #[test]
    fn invalid_type_fails() {
        let err = AnalyzeOptions::from_toml_str(
            "[analyzer]\nschema_version=1\n[analyzer.queueing]\ntrigger_permille='x'\n",
        )
        .unwrap_err();
        assert!(matches!(err, AnalyzeConfigError::InvalidToml { .. }));
    }

    #[test]
    fn invalid_range_fails_validation() {
        let err = AnalyzeOptions::from_toml_str(
            "[analyzer]\nschema_version=1\n[analyzer.queueing]\ntrigger_permille=1001\n",
        )
        .unwrap_err();
        assert_eq!(
            err,
            AnalyzeConfigError::InvalidConfigValue {
                path: "queueing.trigger_permille",
                message: "must be <= 1000".to_string()
            }
        );
    }

    #[test]
    fn example_file_parses() {
        let input = include_str!("../../../examples/analyzer-config.toml");
        AnalyzeOptions::from_toml_str(input).expect("example parses");
    }

    #[test]
    fn downstream_pattern_list_parses() {
        let parsed = AnalyzeOptions::from_toml_str(
            "[analyzer]\nschema_version=1\n[analyzer.downstream]\nblocking_correlated_stage_patterns=['a','b']\n",
        )
        .expect("list parses");
        assert_eq!(
            parsed.downstream.blocking_correlated_stage_patterns,
            vec!["a", "b"]
        );
    }

    #[test]
    fn empty_downstream_pattern_fails_validation() {
        let err = AnalyzeOptions::from_toml_str(
            "[analyzer]\nschema_version=1\n[analyzer.downstream]\nblocking_correlated_stage_patterns=['ok', '  ']\n",
        )
        .unwrap_err();
        assert_eq!(
            err,
            AnalyzeConfigError::InvalidConfigValue {
                path: "downstream.blocking_correlated_stage_patterns",
                message: "entries must be non-empty after trim".to_string()
            }
        );
    }
}
