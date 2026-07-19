use super::{registry, AnalyzeConfigError, AnalyzeOptions};
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
        let assignments = config.assignments();
        let mut candidate = self.clone();
        for (path, value) in assignments {
            if let Some(spec) = registry::find_spec(path) {
                spec.set_value(&mut candidate, value);
            }
        }
        candidate.validate()?;
        self = candidate;
        Ok(self)
    }
}

impl AnalyzerTomlConfig {
    #[allow(clippy::too_many_lines)]
    fn assignments(&self) -> Vec<(&'static str, registry::OptionValue)> {
        let mut out = Vec::new();
        if let Some(p) = &self.queueing {
            push(
                &mut out,
                "queueing.trigger_permille",
                p.trigger_permille.map(registry::OptionValue::U64),
            );
        }
        if let Some(p) = &self.blocking {
            push(
                &mut out,
                "blocking.min_nonzero_samples_for_signal",
                p.min_nonzero_samples_for_signal
                    .map(registry::OptionValue::Usize),
            );
            push(
                &mut out,
                "blocking.strong_p95_threshold",
                p.strong_p95_threshold.map(registry::OptionValue::U64),
            );
            push(
                &mut out,
                "blocking.strong_peak_threshold",
                p.strong_peak_threshold.map(registry::OptionValue::U64),
            );
            push(
                &mut out,
                "blocking.strong_nonzero_share_permille",
                p.strong_nonzero_share_permille
                    .map(registry::OptionValue::U64),
            );
            push(
                &mut out,
                "blocking.strong_min_samples",
                p.strong_min_samples.map(registry::OptionValue::Usize),
            );
        }
        if let Some(p) = &self.executor {
            push(
                &mut out,
                "executor.min_global_queue_p95_for_signal",
                p.min_global_queue_p95_for_signal
                    .map(registry::OptionValue::U64),
            );
        }
        if let Some(p) = &self.downstream {
            push(
                &mut out,
                "downstream.min_stage_samples",
                p.min_stage_samples.map(registry::OptionValue::Usize),
            );
            push(
                &mut out,
                "downstream.blocking_correlated_stage_patterns",
                p.blocking_correlated_stage_patterns
                    .clone()
                    .map(registry::OptionValue::StringList),
            );
            push(
                &mut out,
                "downstream.blocking_correlation_score_margin",
                p.blocking_correlation_score_margin
                    .map(registry::OptionValue::U8),
            );
        }
        if let Some(p) = &self.confidence {
            push(
                &mut out,
                "confidence.medium_score_threshold",
                p.medium_score_threshold.map(registry::OptionValue::U8),
            );
            push(
                &mut out,
                "confidence.high_score_threshold",
                p.high_score_threshold.map(registry::OptionValue::U8),
            );
            push(
                &mut out,
                "confidence.ambiguity_min_score",
                p.ambiguity_min_score.map(registry::OptionValue::U8),
            );
            push(
                &mut out,
                "confidence.ambiguity_score_gap",
                p.ambiguity_score_gap.map(registry::OptionValue::U8),
            );
        }
        if let Some(p) = &self.evidence {
            push(
                &mut out,
                "evidence.low_completed_request_threshold",
                p.low_completed_request_threshold
                    .map(registry::OptionValue::Usize),
            );
        }
        if let Some(p) = &self.route {
            push(
                &mut out,
                "route.min_request_count",
                p.min_request_count.map(registry::OptionValue::Usize),
            );
            push(
                &mut out,
                "route.breakdown_limit",
                p.breakdown_limit.map(registry::OptionValue::Usize),
            );
            push(
                &mut out,
                "route.emit_on_divergent_suspects",
                p.emit_on_divergent_suspects
                    .map(registry::OptionValue::Bool),
            );
            push(
                &mut out,
                "route.slowest_to_fastest_p95_ratio_numerator",
                p.slowest_to_fastest_p95_ratio_numerator
                    .map(registry::OptionValue::U64),
            );
            push(
                &mut out,
                "route.slowest_to_fastest_p95_ratio_denominator",
                p.slowest_to_fastest_p95_ratio_denominator
                    .map(registry::OptionValue::U64),
            );
            push(
                &mut out,
                "route.slowest_to_global_p95_ratio_numerator",
                p.slowest_to_global_p95_ratio_numerator
                    .map(registry::OptionValue::U64),
            );
            push(
                &mut out,
                "route.slowest_to_global_p95_ratio_denominator",
                p.slowest_to_global_p95_ratio_denominator
                    .map(registry::OptionValue::U64),
            );
        }
        if let Some(p) = &self.temporal {
            push(
                &mut out,
                "temporal.min_request_count",
                p.min_request_count.map(registry::OptionValue::Usize),
            );
            push(
                &mut out,
                "temporal.min_segment_request_count",
                p.min_segment_request_count
                    .map(registry::OptionValue::Usize),
            );
            push(
                &mut out,
                "temporal.share_shift_permille",
                p.share_shift_permille.map(registry::OptionValue::U64),
            );
            push(
                &mut out,
                "temporal.p95_shift_ratio_numerator",
                p.p95_shift_ratio_numerator.map(registry::OptionValue::U64),
            );
            push(
                &mut out,
                "temporal.p95_shift_ratio_denominator",
                p.p95_shift_ratio_denominator
                    .map(registry::OptionValue::U64),
            );
            push(
                &mut out,
                "temporal.emit_on_suspect_shift",
                p.emit_on_suspect_shift.map(registry::OptionValue::Bool),
            );
            push(
                &mut out,
                "temporal.suppress_runtime_sparse_suspect_shift_without_supporting_movement",
                p.suppress_runtime_sparse_suspect_shift_without_supporting_movement
                    .map(registry::OptionValue::Bool),
            );
        }
        out
    }
}

fn push(
    out: &mut Vec<(&'static str, registry::OptionValue)>,
    path: &'static str,
    value: Option<registry::OptionValue>,
) {
    if let Some(value) = value {
        out.push((path, value));
    }
}
