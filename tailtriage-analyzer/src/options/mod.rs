use serde::Serialize;
use std::error::Error;
use std::fmt;

mod descriptors;

pub use descriptors::{analyze_option_descriptors, AnalyzeOptionDescriptor};

/// Options for heuristic run analysis.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct AnalyzeOptions {
    /// Queueing-related analyzer thresholds.
    pub queueing: QueueingOptions,
    /// Blocking-pool-related analyzer thresholds.
    pub blocking: BlockingOptions,
    /// Executor-pressure-related analyzer thresholds.
    pub executor: ExecutorOptions,
    /// Downstream-stage-related analyzer thresholds.
    pub downstream: DownstreamOptions,
    /// Confidence bucketing and ambiguity thresholds.
    pub confidence: ConfidenceOptions,
    /// Evidence quality thresholds.
    pub evidence: EvidenceOptions,
    /// Route breakdown thresholds and toggles.
    pub route: RouteOptions,
    /// Temporal segment thresholds and toggles.
    pub temporal: TemporalOptions,
}

impl AnalyzeOptions {
    #[must_use]
    pub fn with_queueing(mut self, configure: impl FnOnce(&mut QueueingOptions)) -> Self {
        configure(&mut self.queueing);
        self
    }
    #[must_use]
    pub fn with_blocking(mut self, configure: impl FnOnce(&mut BlockingOptions)) -> Self {
        configure(&mut self.blocking);
        self
    }
    #[must_use]
    pub fn with_executor(mut self, configure: impl FnOnce(&mut ExecutorOptions)) -> Self {
        configure(&mut self.executor);
        self
    }
    #[must_use]
    pub fn with_downstream(mut self, configure: impl FnOnce(&mut DownstreamOptions)) -> Self {
        configure(&mut self.downstream);
        self
    }
    #[must_use]
    pub fn with_confidence(mut self, configure: impl FnOnce(&mut ConfidenceOptions)) -> Self {
        configure(&mut self.confidence);
        self
    }
    #[must_use]
    pub fn with_evidence(mut self, configure: impl FnOnce(&mut EvidenceOptions)) -> Self {
        configure(&mut self.evidence);
        self
    }
    #[must_use]
    pub fn with_route(mut self, configure: impl FnOnce(&mut RouteOptions)) -> Self {
        configure(&mut self.route);
        self
    }
    #[must_use]
    pub fn with_temporal(mut self, configure: impl FnOnce(&mut TemporalOptions)) -> Self {
        configure(&mut self.temporal);
        self
    }

    /// Validates this semantic analyzer options set.
    ///
    /// # Errors
    ///
    /// Returns [`AnalyzeConfigError::InvalidConfigValue`] when any value violates a
    /// v1 semantic constraint.
    pub fn validate(&self) -> Result<(), AnalyzeConfigError> {
        validate_permille("queueing.trigger_permille", self.queueing.trigger_permille)?;
        validate_permille(
            "blocking.strong_nonzero_share_permille",
            self.blocking.strong_nonzero_share_permille,
        )?;
        if self.confidence.medium_score_threshold > self.confidence.high_score_threshold {
            return Err(invalid(
                "confidence.medium_score_threshold",
                "must be <= confidence.high_score_threshold",
            ));
        }
        validate_u8(
            "confidence.high_score_threshold",
            self.confidence.high_score_threshold,
        )?;
        validate_u8(
            "confidence.ambiguity_min_score",
            self.confidence.ambiguity_min_score,
        )?;
        validate_u8(
            "confidence.ambiguity_score_gap",
            self.confidence.ambiguity_score_gap,
        )?;
        validate_u8(
            "downstream.blocking_correlation_score_margin",
            self.downstream.blocking_correlation_score_margin,
        )?;
        if self.route.breakdown_limit == 0 {
            return Err(invalid("route.breakdown_limit", "must be > 0"));
        }
        validate_ratio(
            "route.slowest_to_fastest_p95_ratio_numerator",
            self.route.slowest_to_fastest_p95_ratio_numerator,
            "route.slowest_to_fastest_p95_ratio_denominator",
            self.route.slowest_to_fastest_p95_ratio_denominator,
        )?;
        validate_ratio(
            "route.slowest_to_global_p95_ratio_numerator",
            self.route.slowest_to_global_p95_ratio_numerator,
            "route.slowest_to_global_p95_ratio_denominator",
            self.route.slowest_to_global_p95_ratio_denominator,
        )?;
        if self.temporal.min_segment_request_count == 0 {
            return Err(invalid("temporal.min_segment_request_count", "must be > 0"));
        }
        if self.temporal.min_segment_request_count.saturating_mul(2)
            > self.temporal.min_request_count
        {
            return Err(invalid(
                "temporal.min_segment_request_count",
                "* 2 must be <= temporal.min_request_count",
            ));
        }
        validate_permille(
            "temporal.share_shift_permille",
            self.temporal.share_shift_permille,
        )?;
        validate_ratio(
            "temporal.p95_shift_ratio_numerator",
            self.temporal.p95_shift_ratio_numerator,
            "temporal.p95_shift_ratio_denominator",
            self.temporal.p95_shift_ratio_denominator,
        )?;
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

fn invalid(path: &'static str, message: &str) -> AnalyzeConfigError {
    AnalyzeConfigError::InvalidConfigValue {
        path,
        message: message.to_string(),
    }
}
fn validate_permille(path: &'static str, value: u64) -> Result<(), AnalyzeConfigError> {
    if value <= 1000 {
        Ok(())
    } else {
        Err(invalid(path, "must be <= 1000"))
    }
}
fn validate_u8(path: &'static str, value: u8) -> Result<(), AnalyzeConfigError> {
    if value <= 100 {
        Ok(())
    } else {
        Err(invalid(path, "must be <= 100"))
    }
}
fn validate_ratio(
    np: &'static str,
    n: u64,
    dp: &'static str,
    d: u64,
) -> Result<(), AnalyzeConfigError> {
    if n == 0 {
        return Err(invalid(np, "must be > 0"));
    }
    if d == 0 {
        return Err(invalid(dp, "must be > 0"));
    }
    if n < d {
        return Err(invalid(np, &format!("must be >= {dp}")));
    }
    Ok(())
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct QueueingOptions {
    pub trigger_permille: u64,
}
impl Default for QueueingOptions {
    fn default() -> Self {
        Self {
            trigger_permille: 300,
        }
    }
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
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExecutorOptions {
    pub min_global_queue_p95_for_signal: u64,
}
impl Default for ExecutorOptions {
    fn default() -> Self {
        Self {
            min_global_queue_p95_for_signal: 1,
        }
    }
}
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DownstreamOptions {
    pub min_stage_samples: usize,
    pub blocking_correlated_stage_patterns: Vec<String>,
    pub blocking_correlation_score_margin: u8,
}
impl Default for DownstreamOptions {
    fn default() -> Self {
        Self {
            min_stage_samples: 3,
            blocking_correlated_stage_patterns: vec![
                "spawn_blocking".to_string(),
                "blocking_path".to_string(),
                "blocking".to_string(),
            ],
            blocking_correlation_score_margin: 2,
        }
    }
}
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConfidenceOptions {
    pub medium_score_threshold: u8,
    pub high_score_threshold: u8,
    pub ambiguity_min_score: u8,
    pub ambiguity_score_gap: u8,
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
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EvidenceOptions {
    pub low_completed_request_threshold: usize,
}
impl Default for EvidenceOptions {
    fn default() -> Self {
        Self {
            low_completed_request_threshold: 20,
        }
    }
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
                write!(f, "unknown override path: {path}")?;
                if let Some(s) = suggestion {
                    write!(f, " (did you mean {s}?)")?;
                }
                Ok(())
            }
            Self::InvalidOverrideValue {
                path,
                value,
                expected,
            } => write!(
                f,
                "invalid override value for {path}: {value} (expected {expected})"
            ),
            Self::InvalidConfigValue { path, message } => {
                write!(f, "invalid config value for {path}: {message}")
            }
            Self::MissingAnalyzerTable => write!(f, "missing analyzer table"),
            Self::MissingSchemaVersion => write!(f, "missing schema version"),
            Self::UnsupportedSchemaVersion { found, supported } => write!(
                f,
                "unsupported schema version {found}; supported version is {supported}"
            ),
            Self::InvalidToml { message } => write!(f, "invalid toml: {message}"),
        }
    }
}
impl Error for AnalyzeConfigError {}
