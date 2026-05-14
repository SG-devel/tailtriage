use super::{
    AnalyzeConfigError, AnalyzeOptions, BlockingOptions, ConfidenceOptions, DownstreamOptions,
    EvidenceOptions, ExecutorOptions, QueueingOptions, RouteOptions, TemporalOptions,
};
use serde::Deserialize;

const SUPPORTED_SCHEMA_VERSION: u64 = 1;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AnalyzerTomlConfig {
    schema_version: Option<u64>,
    queueing: Option<QueueingOptionsToml>,
    blocking: Option<BlockingOptionsToml>,
    executor: Option<ExecutorOptionsToml>,
    downstream: Option<DownstreamOptionsToml>,
    confidence: Option<ConfidenceOptionsToml>,
    evidence: Option<EvidenceOptionsToml>,
    route: Option<RouteOptionsToml>,
    temporal: Option<TemporalOptionsToml>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct QueueingOptionsToml {
    trigger_permille: Option<u64>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BlockingOptionsToml {
    min_nonzero_samples_for_signal: Option<usize>,
    strong_p95_threshold: Option<u64>,
    strong_peak_threshold: Option<u64>,
    strong_nonzero_share_permille: Option<u64>,
    strong_min_samples: Option<usize>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExecutorOptionsToml {
    min_global_queue_p95_for_signal: Option<u64>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DownstreamOptionsToml {
    min_stage_samples: Option<usize>,
    blocking_correlated_stage_patterns: Option<Vec<String>>,
    blocking_correlation_score_margin: Option<u8>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfidenceOptionsToml {
    medium_score_threshold: Option<u8>,
    high_score_threshold: Option<u8>,
    ambiguity_min_score: Option<u8>,
    ambiguity_score_gap: Option<u8>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EvidenceOptionsToml {
    low_completed_request_threshold: Option<usize>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RouteOptionsToml {
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
struct TemporalOptionsToml {
    min_request_count: Option<usize>,
    min_segment_request_count: Option<usize>,
    share_shift_permille: Option<u64>,
    p95_shift_ratio_numerator: Option<u64>,
    p95_shift_ratio_denominator: Option<u64>,
    emit_on_suspect_shift: Option<bool>,
    suppress_runtime_sparse_suspect_shift_without_supporting_movement: Option<bool>,
}

impl AnalyzeOptions {
    /// Parses strict analyzer TOML from a shared config string.
    ///
    /// # Errors
    /// Returns [`AnalyzeConfigError`] when `[analyzer]` is missing, schema rules fail,
    /// TOML decode fails, or parsed values fail semantic validation.
    pub fn from_toml_str(input: &str) -> Result<Self, AnalyzeConfigError> {
        Self::default().merge_toml_str(input)
    }

    /// Merges strict analyzer TOML into existing options.
    ///
    /// # Errors
    /// Returns [`AnalyzeConfigError`] when `[analyzer]` is missing, schema rules fail,
    /// TOML decode fails, or merged values fail semantic validation.
    pub fn merge_toml_str(mut self, input: &str) -> Result<Self, AnalyzeConfigError> {
        let root: toml::Value =
            toml::from_str(input).map_err(|e| AnalyzeConfigError::InvalidToml {
                message: e.to_string(),
            })?;
        let Some(analyzer_value) = root.get("analyzer") else {
            return Err(AnalyzeConfigError::MissingAnalyzerTable);
        };
        let config: AnalyzerTomlConfig =
            analyzer_value
                .clone()
                .try_into()
                .map_err(|e: toml::de::Error| AnalyzeConfigError::InvalidToml {
                    message: e.to_string(),
                })?;
        let Some(schema_version) = config.schema_version else {
            return Err(AnalyzeConfigError::MissingSchemaVersion);
        };
        if schema_version != SUPPORTED_SCHEMA_VERSION {
            return Err(AnalyzeConfigError::UnsupportedSchemaVersion {
                found: schema_version,
                supported: SUPPORTED_SCHEMA_VERSION,
            });
        }
        if let Some(p) = &config.queueing {
            apply_queueing(&mut self.queueing, p);
        }
        if let Some(p) = &config.blocking {
            apply_blocking(&mut self.blocking, p);
        }
        if let Some(p) = &config.executor {
            apply_executor(&mut self.executor, p);
        }
        if let Some(p) = &config.downstream {
            apply_downstream(&mut self.downstream, p);
        }
        if let Some(p) = &config.confidence {
            apply_confidence(&mut self.confidence, p);
        }
        if let Some(p) = &config.evidence {
            apply_evidence(&mut self.evidence, p);
        }
        if let Some(p) = &config.route {
            apply_route(&mut self.route, p);
        }
        if let Some(p) = &config.temporal {
            apply_temporal(&mut self.temporal, p);
        }
        self.validate()?;
        Ok(self)
    }
}
fn apply_queueing(out: &mut QueueingOptions, p: &QueueingOptionsToml) {
    if let Some(v) = p.trigger_permille {
        out.trigger_permille = v;
    }
}
fn apply_blocking(out: &mut BlockingOptions, p: &BlockingOptionsToml) {
    if let Some(v) = p.min_nonzero_samples_for_signal {
        out.min_nonzero_samples_for_signal = v;
    }
    if let Some(v) = p.strong_p95_threshold {
        out.strong_p95_threshold = v;
    }
    if let Some(v) = p.strong_peak_threshold {
        out.strong_peak_threshold = v;
    }
    if let Some(v) = p.strong_nonzero_share_permille {
        out.strong_nonzero_share_permille = v;
    }
    if let Some(v) = p.strong_min_samples {
        out.strong_min_samples = v;
    }
}
fn apply_executor(out: &mut ExecutorOptions, p: &ExecutorOptionsToml) {
    if let Some(v) = p.min_global_queue_p95_for_signal {
        out.min_global_queue_p95_for_signal = v;
    }
}
fn apply_downstream(out: &mut DownstreamOptions, p: &DownstreamOptionsToml) {
    if let Some(v) = p.min_stage_samples {
        out.min_stage_samples = v;
    }
    if let Some(v) = &p.blocking_correlated_stage_patterns {
        out.blocking_correlated_stage_patterns.clone_from(v);
    }
    if let Some(v) = p.blocking_correlation_score_margin {
        out.blocking_correlation_score_margin = v;
    }
}
fn apply_confidence(out: &mut ConfidenceOptions, p: &ConfidenceOptionsToml) {
    if let Some(v) = p.medium_score_threshold {
        out.medium_score_threshold = v;
    }
    if let Some(v) = p.high_score_threshold {
        out.high_score_threshold = v;
    }
    if let Some(v) = p.ambiguity_min_score {
        out.ambiguity_min_score = v;
    }
    if let Some(v) = p.ambiguity_score_gap {
        out.ambiguity_score_gap = v;
    }
}
fn apply_evidence(out: &mut EvidenceOptions, p: &EvidenceOptionsToml) {
    if let Some(v) = p.low_completed_request_threshold {
        out.low_completed_request_threshold = v;
    }
}
fn apply_route(out: &mut RouteOptions, p: &RouteOptionsToml) {
    if let Some(v) = p.min_request_count {
        out.min_request_count = v;
    }
    if let Some(v) = p.breakdown_limit {
        out.breakdown_limit = v;
    }
    if let Some(v) = p.emit_on_divergent_suspects {
        out.emit_on_divergent_suspects = v;
    }
    if let Some(v) = p.slowest_to_fastest_p95_ratio_numerator {
        out.slowest_to_fastest_p95_ratio_numerator = v;
    }
    if let Some(v) = p.slowest_to_fastest_p95_ratio_denominator {
        out.slowest_to_fastest_p95_ratio_denominator = v;
    }
    if let Some(v) = p.slowest_to_global_p95_ratio_numerator {
        out.slowest_to_global_p95_ratio_numerator = v;
    }
    if let Some(v) = p.slowest_to_global_p95_ratio_denominator {
        out.slowest_to_global_p95_ratio_denominator = v;
    }
}
fn apply_temporal(out: &mut TemporalOptions, p: &TemporalOptionsToml) {
    if let Some(v) = p.min_request_count {
        out.min_request_count = v;
    }
    if let Some(v) = p.min_segment_request_count {
        out.min_segment_request_count = v;
    }
    if let Some(v) = p.share_shift_permille {
        out.share_shift_permille = v;
    }
    if let Some(v) = p.p95_shift_ratio_numerator {
        out.p95_shift_ratio_numerator = v;
    }
    if let Some(v) = p.p95_shift_ratio_denominator {
        out.p95_shift_ratio_denominator = v;
    }
    if let Some(v) = p.emit_on_suspect_shift {
        out.emit_on_suspect_shift = v;
    }
    if let Some(v) = p.suppress_runtime_sparse_suspect_shift_without_supporting_movement {
        out.suppress_runtime_sparse_suspect_shift_without_supporting_movement = v;
    }
}
