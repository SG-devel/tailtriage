use super::{
    AnalyzeConfigError, AnalyzeOptions, BlockingOptions, ConfidenceOptions, DownstreamOptions,
    EvidenceOptions, ExecutorOptions, QueueingOptions, RouteOptions, TemporalOptions,
};
use serde::Deserialize;

const SUPPORTED_SCHEMA_VERSION: u64 = 1;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AnalyzerToml {
    schema_version: Option<u64>,
    queueing: Option<QueueingPartial>,
    blocking: Option<BlockingPartial>,
    executor: Option<ExecutorPartial>,
    downstream: Option<DownstreamPartial>,
    confidence: Option<ConfidencePartial>,
    evidence: Option<EvidencePartial>,
    route: Option<RoutePartial>,
    temporal: Option<TemporalPartial>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct QueueingPartial {
    trigger_permille: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BlockingPartial {
    min_nonzero_samples_for_signal: Option<usize>,
    strong_p95_threshold: Option<u64>,
    strong_peak_threshold: Option<u64>,
    strong_nonzero_share_permille: Option<u64>,
    strong_min_samples: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExecutorPartial {
    min_global_queue_p95_for_signal: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DownstreamPartial {
    min_stage_samples: Option<usize>,
    blocking_correlated_stage_patterns: Option<Vec<String>>,
    blocking_correlation_score_margin: Option<u8>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfidencePartial {
    medium_score_threshold: Option<u8>,
    high_score_threshold: Option<u8>,
    ambiguity_min_score: Option<u8>,
    ambiguity_score_gap: Option<u8>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EvidencePartial {
    low_completed_request_threshold: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RoutePartial {
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
struct TemporalPartial {
    min_request_count: Option<usize>,
    min_segment_request_count: Option<usize>,
    share_shift_permille: Option<u64>,
    p95_shift_ratio_numerator: Option<u64>,
    p95_shift_ratio_denominator: Option<u64>,
    emit_on_suspect_shift: Option<bool>,
    suppress_runtime_sparse_suspect_shift_without_supporting_movement: Option<bool>,
}

pub(super) fn parse_from_toml_str(input: &str) -> Result<AnalyzeOptions, AnalyzeConfigError> {
    AnalyzeOptions::default().merge_toml_str(input)
}

pub(super) fn merge_toml_str(
    mut options: AnalyzeOptions,
    input: &str,
) -> Result<AnalyzeOptions, AnalyzeConfigError> {
    let parsed: toml::Value =
        toml::from_str(input).map_err(|error| AnalyzeConfigError::InvalidToml {
            message: error.to_string(),
        })?;
    let analyzer_value = parsed
        .as_table()
        .and_then(|root| root.get("analyzer"))
        .ok_or(AnalyzeConfigError::MissingAnalyzerTable)?;
    let analyzer: AnalyzerToml =
        analyzer_value
            .clone()
            .try_into()
            .map_err(|error: toml::de::Error| AnalyzeConfigError::InvalidToml {
                message: error.to_string(),
            })?;

    let schema_version = analyzer
        .schema_version
        .ok_or(AnalyzeConfigError::MissingSchemaVersion)?;
    if schema_version != SUPPORTED_SCHEMA_VERSION {
        return Err(AnalyzeConfigError::UnsupportedSchemaVersion {
            found: schema_version,
            supported: SUPPORTED_SCHEMA_VERSION,
        });
    }

    if let Some(queueing) = analyzer.queueing.as_ref() {
        apply_queueing(&mut options.queueing, queueing);
    }
    if let Some(blocking) = analyzer.blocking.as_ref() {
        apply_blocking(&mut options.blocking, blocking);
    }
    if let Some(executor) = analyzer.executor.as_ref() {
        apply_executor(&mut options.executor, executor);
    }
    if let Some(downstream) = analyzer.downstream.as_ref() {
        apply_downstream(&mut options.downstream, downstream);
    }
    if let Some(confidence) = analyzer.confidence.as_ref() {
        apply_confidence(&mut options.confidence, confidence);
    }
    if let Some(evidence) = analyzer.evidence.as_ref() {
        apply_evidence(&mut options.evidence, evidence);
    }
    if let Some(route) = analyzer.route.as_ref() {
        apply_route(&mut options.route, route);
    }
    if let Some(temporal) = analyzer.temporal.as_ref() {
        apply_temporal(&mut options.temporal, temporal);
    }

    options.validate()?;
    Ok(options)
}

fn apply_queueing(target: &mut QueueingOptions, source: &QueueingPartial) {
    if let Some(v) = source.trigger_permille {
        target.trigger_permille = v;
    }
}
fn apply_blocking(target: &mut BlockingOptions, source: &BlockingPartial) {
    if let Some(v) = source.min_nonzero_samples_for_signal {
        target.min_nonzero_samples_for_signal = v;
    }
    if let Some(v) = source.strong_p95_threshold {
        target.strong_p95_threshold = v;
    }
    if let Some(v) = source.strong_peak_threshold {
        target.strong_peak_threshold = v;
    }
    if let Some(v) = source.strong_nonzero_share_permille {
        target.strong_nonzero_share_permille = v;
    }
    if let Some(v) = source.strong_min_samples {
        target.strong_min_samples = v;
    }
}
fn apply_executor(target: &mut ExecutorOptions, source: &ExecutorPartial) {
    if let Some(v) = source.min_global_queue_p95_for_signal {
        target.min_global_queue_p95_for_signal = v;
    }
}
fn apply_downstream(target: &mut DownstreamOptions, source: &DownstreamPartial) {
    if let Some(v) = source.min_stage_samples {
        target.min_stage_samples = v;
    }
    if let Some(v) = &source.blocking_correlated_stage_patterns {
        target.blocking_correlated_stage_patterns.clone_from(v);
    }
    if let Some(v) = source.blocking_correlation_score_margin {
        target.blocking_correlation_score_margin = v;
    }
}
fn apply_confidence(target: &mut ConfidenceOptions, source: &ConfidencePartial) {
    if let Some(v) = source.medium_score_threshold {
        target.medium_score_threshold = v;
    }
    if let Some(v) = source.high_score_threshold {
        target.high_score_threshold = v;
    }
    if let Some(v) = source.ambiguity_min_score {
        target.ambiguity_min_score = v;
    }
    if let Some(v) = source.ambiguity_score_gap {
        target.ambiguity_score_gap = v;
    }
}
fn apply_evidence(target: &mut EvidenceOptions, source: &EvidencePartial) {
    if let Some(v) = source.low_completed_request_threshold {
        target.low_completed_request_threshold = v;
    }
}
fn apply_route(target: &mut RouteOptions, source: &RoutePartial) {
    if let Some(v) = source.min_request_count {
        target.min_request_count = v;
    }
    if let Some(v) = source.breakdown_limit {
        target.breakdown_limit = v;
    }
    if let Some(v) = source.emit_on_divergent_suspects {
        target.emit_on_divergent_suspects = v;
    }
    if let Some(v) = source.slowest_to_fastest_p95_ratio_numerator {
        target.slowest_to_fastest_p95_ratio_numerator = v;
    }
    if let Some(v) = source.slowest_to_fastest_p95_ratio_denominator {
        target.slowest_to_fastest_p95_ratio_denominator = v;
    }
    if let Some(v) = source.slowest_to_global_p95_ratio_numerator {
        target.slowest_to_global_p95_ratio_numerator = v;
    }
    if let Some(v) = source.slowest_to_global_p95_ratio_denominator {
        target.slowest_to_global_p95_ratio_denominator = v;
    }
}
fn apply_temporal(target: &mut TemporalOptions, source: &TemporalPartial) {
    if let Some(v) = source.min_request_count {
        target.min_request_count = v;
    }
    if let Some(v) = source.min_segment_request_count {
        target.min_segment_request_count = v;
    }
    if let Some(v) = source.share_shift_permille {
        target.share_shift_permille = v;
    }
    if let Some(v) = source.p95_shift_ratio_numerator {
        target.p95_shift_ratio_numerator = v;
    }
    if let Some(v) = source.p95_shift_ratio_denominator {
        target.p95_shift_ratio_denominator = v;
    }
    if let Some(v) = source.emit_on_suspect_shift {
        target.emit_on_suspect_shift = v;
    }
    if let Some(v) = source.suppress_runtime_sparse_suspect_shift_without_supporting_movement {
        target.suppress_runtime_sparse_suspect_shift_without_supporting_movement = v;
    }
}
