use std::fmt;

use serde::Serialize;

pub mod descriptors;

pub use descriptors::{analyze_option_descriptors, AnalyzeOptionDescriptor};

/// Semantic analyzer options grouped by diagnostic concern.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AnalyzeOptions {
    pub queueing: QueueingOptions,
    pub blocking: BlockingOptions,
    pub executor: ExecutorOptions,
    pub downstream: DownstreamOptions,
    pub confidence: ConfidenceOptions,
    pub evidence: EvidenceOptions,
    pub route: RouteOptions,
    pub temporal: TemporalOptions,
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct QueueingOptions {
    pub trigger_permille: u64,
}
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BlockingOptions {
    pub min_nonzero_samples_for_signal: usize,
    pub strong_p95_threshold: u64,
    pub strong_peak_threshold: u64,
    pub strong_nonzero_share_permille: u64,
    pub strong_min_samples: usize,
}
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExecutorOptions {
    pub min_global_queue_p95_for_signal: u64,
}
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DownstreamOptions {
    pub min_stage_samples: usize,
    pub blocking_correlated_stage_patterns: Vec<String>,
    pub blocking_correlation_score_margin: u8,
}
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConfidenceOptions {
    pub medium_score_threshold: u8,
    pub high_score_threshold: u8,
    pub ambiguity_min_score: u8,
    pub ambiguity_score_gap: u8,
}
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EvidenceOptions {
    pub low_completed_request_threshold: usize,
}
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RouteOptions {
    pub min_request_count: usize,
    pub breakdown_limit: usize,
    pub emit_on_divergent_suspects: bool,
    pub slowest_to_fastest_p95_ratio_numerator: u64,
    pub slowest_to_fastest_p95_ratio_denominator: u64,
    pub slowest_to_global_p95_ratio_numerator: u64,
    pub slowest_to_global_p95_ratio_denominator: u64,
}
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TemporalOptions {
    pub min_request_count: usize,
    pub min_segment_request_count: usize,
    pub share_shift_permille: u64,
    pub p95_shift_ratio_numerator: u64,
    pub p95_shift_ratio_denominator: u64,
    pub emit_on_suspect_shift: bool,
    pub suppress_runtime_sparse_suspect_shift_without_supporting_movement: bool,
}

impl Default for AnalyzeOptions {
    fn default() -> Self {
        Self {
            queueing: QueueingOptions::default(),
            blocking: BlockingOptions::default(),
            executor: ExecutorOptions::default(),
            downstream: DownstreamOptions::default(),
            confidence: ConfidenceOptions::default(),
            evidence: EvidenceOptions::default(),
            route: RouteOptions::default(),
            temporal: TemporalOptions::default(),
        }
    }
}
impl Default for QueueingOptions {
    fn default() -> Self {
        Self {
            trigger_permille: 300,
        }
    }
}
impl Default for BlockingOptions {
    fn default() -> Self {
        Self {
            min_nonzero_samples_for_signal: 2,
            strong_p95_threshold: 12,
            strong_peak_threshold: 20,
            strong_nonzero_share_permille: 700,
            strong_min_samples: 30,
        }
    }
}
impl Default for ExecutorOptions {
    fn default() -> Self {
        Self {
            min_global_queue_p95_for_signal: 1,
        }
    }
}
impl Default for DownstreamOptions {
    fn default() -> Self {
        Self {
            min_stage_samples: 3,
            blocking_correlated_stage_patterns: vec![
                "spawn_blocking".into(),
                "blocking_path".into(),
                "blocking".into(),
            ],
            blocking_correlation_score_margin: 2,
        }
    }
}
impl Default for ConfidenceOptions {
    fn default() -> Self {
        Self {
            medium_score_threshold: 65,
            high_score_threshold: 85,
            ambiguity_min_score: 60,
            ambiguity_score_gap: 4,
        }
    }
}
impl Default for EvidenceOptions {
    fn default() -> Self {
        Self {
            low_completed_request_threshold: 20,
        }
    }
}
impl Default for RouteOptions {
    fn default() -> Self {
        Self {
            min_request_count: 3,
            breakdown_limit: 10,
            emit_on_divergent_suspects: true,
            slowest_to_fastest_p95_ratio_numerator: 3,
            slowest_to_fastest_p95_ratio_denominator: 2,
            slowest_to_global_p95_ratio_numerator: 5,
            slowest_to_global_p95_ratio_denominator: 4,
        }
    }
}
impl Default for TemporalOptions {
    fn default() -> Self {
        Self {
            min_request_count: 20,
            min_segment_request_count: 8,
            share_shift_permille: 200,
            p95_shift_ratio_numerator: 3,
            p95_shift_ratio_denominator: 2,
            emit_on_suspect_shift: true,
            suppress_runtime_sparse_suspect_shift_without_supporting_movement: true,
        }
    }
}

impl AnalyzeOptions {
    pub fn with_queueing(mut self, f: impl FnOnce(&mut QueueingOptions)) -> Self {
        f(&mut self.queueing);
        self
    }
    pub fn with_blocking(mut self, f: impl FnOnce(&mut BlockingOptions)) -> Self {
        f(&mut self.blocking);
        self
    }
    pub fn with_executor(mut self, f: impl FnOnce(&mut ExecutorOptions)) -> Self {
        f(&mut self.executor);
        self
    }
    pub fn with_downstream(mut self, f: impl FnOnce(&mut DownstreamOptions)) -> Self {
        f(&mut self.downstream);
        self
    }
    pub fn with_confidence(mut self, f: impl FnOnce(&mut ConfidenceOptions)) -> Self {
        f(&mut self.confidence);
        self
    }
    pub fn with_evidence(mut self, f: impl FnOnce(&mut EvidenceOptions)) -> Self {
        f(&mut self.evidence);
        self
    }
    pub fn with_route(mut self, f: impl FnOnce(&mut RouteOptions)) -> Self {
        f(&mut self.route);
        self
    }
    pub fn with_temporal(mut self, f: impl FnOnce(&mut TemporalOptions)) -> Self {
        f(&mut self.temporal);
        self
    }

    pub fn validate(&self) -> Result<(), AnalyzeConfigError> {
        let invalid = |path: &'static str, message: &str| AnalyzeConfigError::InvalidConfigValue {
            path,
            message: message.to_string(),
        };

        if self.queueing.trigger_permille > 1000 {
            return Err(invalid("queueing.trigger_permille", "must be <= 1000"));
        }
        if self.blocking.strong_nonzero_share_permille > 1000 {
            return Err(invalid(
                "blocking.strong_nonzero_share_permille",
                "must be <= 1000",
            ));
        }
        if self.confidence.medium_score_threshold > self.confidence.high_score_threshold {
            return Err(invalid(
                "confidence.medium_score_threshold",
                "must be <= confidence.high_score_threshold",
            ));
        }
        if self.confidence.high_score_threshold > 100 {
            return Err(invalid("confidence.high_score_threshold", "must be <= 100"));
        }
        if self.confidence.ambiguity_min_score > 100 {
            return Err(invalid("confidence.ambiguity_min_score", "must be <= 100"));
        }
        if self.confidence.ambiguity_score_gap > 100 {
            return Err(invalid("confidence.ambiguity_score_gap", "must be <= 100"));
        }
        if self.downstream.blocking_correlation_score_margin > 100 {
            return Err(invalid(
                "downstream.blocking_correlation_score_margin",
                "must be <= 100",
            ));
        }
        if self.route.breakdown_limit == 0 {
            return Err(invalid("route.breakdown_limit", "must be > 0"));
        }
        if self.route.slowest_to_fastest_p95_ratio_numerator == 0
            || self.route.slowest_to_fastest_p95_ratio_denominator == 0
        {
            return Err(invalid(
                "route.slowest_to_fastest_p95_ratio",
                "numerator and denominator must be > 0",
            ));
        }
        if self.route.slowest_to_fastest_p95_ratio_numerator
            < self.route.slowest_to_fastest_p95_ratio_denominator
        {
            return Err(invalid(
                "route.slowest_to_fastest_p95_ratio_numerator",
                "must be >= route.slowest_to_fastest_p95_ratio_denominator",
            ));
        }
        if self.route.slowest_to_global_p95_ratio_numerator == 0
            || self.route.slowest_to_global_p95_ratio_denominator == 0
        {
            return Err(invalid(
                "route.slowest_to_global_p95_ratio",
                "numerator and denominator must be > 0",
            ));
        }
        if self.route.slowest_to_global_p95_ratio_numerator
            < self.route.slowest_to_global_p95_ratio_denominator
        {
            return Err(invalid(
                "route.slowest_to_global_p95_ratio_numerator",
                "must be >= route.slowest_to_global_p95_ratio_denominator",
            ));
        }
        if self.temporal.min_segment_request_count == 0 {
            return Err(invalid("temporal.min_segment_request_count", "must be > 0"));
        }
        if self.temporal.min_segment_request_count.saturating_mul(2)
            > self.temporal.min_request_count
        {
            return Err(invalid(
                "temporal.min_segment_request_count",
                "must satisfy min_segment_request_count * 2 <= temporal.min_request_count",
            ));
        }
        if self.temporal.share_shift_permille > 1000 {
            return Err(invalid("temporal.share_shift_permille", "must be <= 1000"));
        }
        if self.temporal.p95_shift_ratio_numerator == 0
            || self.temporal.p95_shift_ratio_denominator == 0
        {
            return Err(invalid(
                "temporal.p95_shift_ratio",
                "numerator and denominator must be > 0",
            ));
        }
        if self.temporal.p95_shift_ratio_numerator < self.temporal.p95_shift_ratio_denominator {
            return Err(invalid(
                "temporal.p95_shift_ratio_numerator",
                "must be >= temporal.p95_shift_ratio_denominator",
            ));
        }
        if self
            .downstream
            .blocking_correlated_stage_patterns
            .is_empty()
        {
            return Err(invalid(
                "downstream.blocking_correlated_stage_patterns",
                "must not be empty",
            ));
        }
        if self
            .downstream
            .blocking_correlated_stage_patterns
            .iter()
            .any(|p| p.trim().is_empty())
        {
            return Err(invalid(
                "downstream.blocking_correlated_stage_patterns",
                "entries must be non-empty after trim",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnalyzeConfigError {
    InvalidOverrideSyntax {
        raw: String,
    },
    UnknownOverridePath {
        path: String,
        suggestion: Option<&'static str>,
    },
    InvalidOverrideValue {
        path: &'static str,
        value: String,
        expected: &'static str,
    },
    InvalidConfigValue {
        path: &'static str,
        message: String,
    },
    MissingAnalyzerTable,
    MissingSchemaVersion,
    UnsupportedSchemaVersion {
        found: u64,
        supported: u64,
    },
    InvalidToml {
        message: String,
    },
}
impl fmt::Display for AnalyzeConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidOverrideSyntax { raw } => write!(f, "invalid override syntax: {raw}"),
            Self::UnknownOverridePath { path, suggestion } => {
                if let Some(suggestion) = suggestion {
                    write!(
                        f,
                        "unknown override path '{path}', did you mean '{suggestion}'?"
                    )
                } else {
                    write!(f, "unknown override path '{path}'")
                }
            }
            Self::InvalidOverrideValue {
                path,
                value,
                expected,
            } => write!(
                f,
                "invalid override value for '{path}': '{value}' (expected {expected})"
            ),
            Self::InvalidConfigValue { path, message } => {
                write!(f, "invalid config value at '{path}': {message}")
            }
            Self::MissingAnalyzerTable => write!(f, "missing analyzer table"),
            Self::MissingSchemaVersion => write!(f, "missing schema version"),
            Self::UnsupportedSchemaVersion { found, supported } => write!(
                f,
                "unsupported schema version {found} (supported {supported})"
            ),
            Self::InvalidToml { message } => write!(f, "invalid TOML: {message}"),
        }
    }
}
impl std::error::Error for AnalyzeConfigError {}
