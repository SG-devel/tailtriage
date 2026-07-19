use super::registry::{apply_typed_path, OptionValue};
use super::{AnalyzeConfigError, AnalyzeOptions};
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
        let mut updates = Vec::new();
        collect_updates(&config, &mut updates);
        for (path, value) in updates {
            apply_typed_path(&mut self, path, value)?;
        }
        self.validate()?;
        Ok(self)
    }
}

#[allow(clippy::too_many_lines)]
fn collect_updates(config: &AnalyzerTomlConfig, updates: &mut Vec<(&'static str, OptionValue)>) {
    if let Some(p) = &config.queueing {
        push_update(
            updates,
            "queueing.trigger_permille",
            p.trigger_permille.map(OptionValue::U64),
        );
    }
    if let Some(p) = &config.blocking {
        push_update(
            updates,
            "blocking.min_nonzero_samples_for_signal",
            p.min_nonzero_samples_for_signal.map(OptionValue::Usize),
        );
        push_update(
            updates,
            "blocking.strong_p95_threshold",
            p.strong_p95_threshold.map(OptionValue::U64),
        );
        push_update(
            updates,
            "blocking.strong_peak_threshold",
            p.strong_peak_threshold.map(OptionValue::U64),
        );
        push_update(
            updates,
            "blocking.strong_nonzero_share_permille",
            p.strong_nonzero_share_permille.map(OptionValue::U64),
        );
        push_update(
            updates,
            "blocking.strong_min_samples",
            p.strong_min_samples.map(OptionValue::Usize),
        );
    }
    if let Some(p) = &config.executor {
        push_update(
            updates,
            "executor.min_global_queue_p95_for_signal",
            p.min_global_queue_p95_for_signal.map(OptionValue::U64),
        );
    }
    if let Some(p) = &config.downstream {
        push_update(
            updates,
            "downstream.min_stage_samples",
            p.min_stage_samples.map(OptionValue::Usize),
        );
        push_update(
            updates,
            "downstream.blocking_correlated_stage_patterns",
            p.blocking_correlated_stage_patterns
                .clone()
                .map(OptionValue::StringList),
        );
        push_update(
            updates,
            "downstream.blocking_correlation_score_margin",
            p.blocking_correlation_score_margin.map(OptionValue::U8),
        );
    }
    if let Some(p) = &config.confidence {
        push_update(
            updates,
            "confidence.medium_score_threshold",
            p.medium_score_threshold.map(OptionValue::U8),
        );
        push_update(
            updates,
            "confidence.high_score_threshold",
            p.high_score_threshold.map(OptionValue::U8),
        );
        push_update(
            updates,
            "confidence.ambiguity_min_score",
            p.ambiguity_min_score.map(OptionValue::U8),
        );
        push_update(
            updates,
            "confidence.ambiguity_score_gap",
            p.ambiguity_score_gap.map(OptionValue::U8),
        );
    }
    if let Some(p) = &config.evidence {
        push_update(
            updates,
            "evidence.low_completed_request_threshold",
            p.low_completed_request_threshold.map(OptionValue::Usize),
        );
    }
    if let Some(p) = &config.route {
        push_update(
            updates,
            "route.min_request_count",
            p.min_request_count.map(OptionValue::Usize),
        );
        push_update(
            updates,
            "route.breakdown_limit",
            p.breakdown_limit.map(OptionValue::Usize),
        );
        push_update(
            updates,
            "route.emit_on_divergent_suspects",
            p.emit_on_divergent_suspects.map(OptionValue::Bool),
        );
        push_update(
            updates,
            "route.slowest_to_fastest_p95_ratio_numerator",
            p.slowest_to_fastest_p95_ratio_numerator
                .map(OptionValue::U64),
        );
        push_update(
            updates,
            "route.slowest_to_fastest_p95_ratio_denominator",
            p.slowest_to_fastest_p95_ratio_denominator
                .map(OptionValue::U64),
        );
        push_update(
            updates,
            "route.slowest_to_global_p95_ratio_numerator",
            p.slowest_to_global_p95_ratio_numerator
                .map(OptionValue::U64),
        );
        push_update(
            updates,
            "route.slowest_to_global_p95_ratio_denominator",
            p.slowest_to_global_p95_ratio_denominator
                .map(OptionValue::U64),
        );
    }
    if let Some(p) = &config.temporal {
        push_update(
            updates,
            "temporal.min_request_count",
            p.min_request_count.map(OptionValue::Usize),
        );
        push_update(
            updates,
            "temporal.min_segment_request_count",
            p.min_segment_request_count.map(OptionValue::Usize),
        );
        push_update(
            updates,
            "temporal.share_shift_permille",
            p.share_shift_permille.map(OptionValue::U64),
        );
        push_update(
            updates,
            "temporal.p95_shift_ratio_numerator",
            p.p95_shift_ratio_numerator.map(OptionValue::U64),
        );
        push_update(
            updates,
            "temporal.p95_shift_ratio_denominator",
            p.p95_shift_ratio_denominator.map(OptionValue::U64),
        );
        push_update(
            updates,
            "temporal.emit_on_suspect_shift",
            p.emit_on_suspect_shift.map(OptionValue::Bool),
        );
        push_update(
            updates,
            "temporal.suppress_runtime_sparse_suspect_shift_without_supporting_movement",
            p.suppress_runtime_sparse_suspect_shift_without_supporting_movement
                .map(OptionValue::Bool),
        );
    }
}

fn push_update(
    updates: &mut Vec<(&'static str, OptionValue)>,
    path: &'static str,
    value: Option<OptionValue>,
) {
    if let Some(value) = value {
        updates.push((path, value));
    }
}
