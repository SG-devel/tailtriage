#![allow(missing_docs)]
use serde::Serialize;
use std::error::Error;
use std::fmt::{Display, Formatter};

mod descriptors;
pub use descriptors::analyze_option_descriptors;

/// Semantic analyzer options grouped by triage domain.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
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

macro_rules! opt_struct { ($name:ident { $($f:ident : $t:ty = $d:expr),+ $(,)? }) => {
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct $name { $(pub $f: $t),+ }
impl Default for $name { fn default() -> Self { Self { $($f: $d),+ } } }
}; }
opt_struct!(QueueingOptions {
    trigger_permille: u64 = 300
});
opt_struct!(BlockingOptions {
    min_nonzero_samples_for_signal: usize = 2,
    strong_p95_threshold: u64 = 12,
    strong_peak_threshold: u64 = 20,
    strong_nonzero_share_permille: u64 = 700,
    strong_min_samples: usize = 30
});
opt_struct!(ExecutorOptions {
    min_global_queue_p95_for_signal: u64 = 1
});
opt_struct!(DownstreamOptions { min_stage_samples: usize = 3, blocking_correlated_stage_patterns: Vec<String> = vec!["spawn_blocking".to_owned(), "blocking_path".to_owned(), "blocking".to_owned()], blocking_correlation_score_margin: u8 = 2 });
opt_struct!(ConfidenceOptions {
    medium_score_threshold: u8 = 65,
    high_score_threshold: u8 = 85,
    ambiguity_min_score: u8 = 60,
    ambiguity_score_gap: u8 = 4
});
opt_struct!(EvidenceOptions {
    low_completed_request_threshold: usize = 20
});
opt_struct!(RouteOptions {
    min_request_count: usize = 3,
    breakdown_limit: usize = 10,
    emit_on_divergent_suspects: bool = true,
    slowest_to_fastest_p95_ratio_numerator: u64 = 3,
    slowest_to_fastest_p95_ratio_denominator: u64 = 2,
    slowest_to_global_p95_ratio_numerator: u64 = 5,
    slowest_to_global_p95_ratio_denominator: u64 = 4
});
opt_struct!(TemporalOptions {
    min_request_count: usize = 20,
    min_segment_request_count: usize = 8,
    share_shift_permille: u64 = 200,
    p95_shift_ratio_numerator: u64 = 3,
    p95_shift_ratio_denominator: u64 = 2,
    emit_on_suspect_shift: bool = true,
    suppress_runtime_sparse_suspect_shift_without_supporting_movement: bool = true
});

impl AnalyzeOptions {
    #[must_use]
    pub fn with_queueing(mut self, f: impl FnOnce(&mut QueueingOptions)) -> Self {
        f(&mut self.queueing);
        self
    }
    #[must_use]
    pub fn with_blocking(mut self, f: impl FnOnce(&mut BlockingOptions)) -> Self {
        f(&mut self.blocking);
        self
    }
    #[must_use]
    pub fn with_executor(mut self, f: impl FnOnce(&mut ExecutorOptions)) -> Self {
        f(&mut self.executor);
        self
    }
    #[must_use]
    pub fn with_downstream(mut self, f: impl FnOnce(&mut DownstreamOptions)) -> Self {
        f(&mut self.downstream);
        self
    }
    #[must_use]
    pub fn with_confidence(mut self, f: impl FnOnce(&mut ConfidenceOptions)) -> Self {
        f(&mut self.confidence);
        self
    }
    #[must_use]
    pub fn with_evidence(mut self, f: impl FnOnce(&mut EvidenceOptions)) -> Self {
        f(&mut self.evidence);
        self
    }
    #[must_use]
    pub fn with_route(mut self, f: impl FnOnce(&mut RouteOptions)) -> Self {
        f(&mut self.route);
        self
    }
    #[must_use]
    pub fn with_temporal(mut self, f: impl FnOnce(&mut TemporalOptions)) -> Self {
        f(&mut self.temporal);
        self
    }
    #[allow(clippy::missing_errors_doc, clippy::too_many_lines)]
    pub fn validate(&self) -> Result<(), AnalyzeConfigError> {
        let invalid =
            |path, message: String| Err(AnalyzeConfigError::InvalidConfigValue { path, message });
        if self.queueing.trigger_permille > 1000 {
            return invalid("queueing.trigger_permille", "must be <= 1000".into());
        }
        if self.blocking.strong_nonzero_share_permille > 1000 {
            return invalid(
                "blocking.strong_nonzero_share_permille",
                "must be <= 1000".into(),
            );
        }
        if self.confidence.medium_score_threshold > self.confidence.high_score_threshold {
            return invalid(
                "confidence.medium_score_threshold",
                "must be <= confidence.high_score_threshold".into(),
            );
        }
        if self.confidence.high_score_threshold > 100 {
            return invalid("confidence.high_score_threshold", "must be <= 100".into());
        }
        if self.confidence.ambiguity_min_score > 100 {
            return invalid("confidence.ambiguity_min_score", "must be <= 100".into());
        }
        if self.confidence.ambiguity_score_gap > 100 {
            return invalid("confidence.ambiguity_score_gap", "must be <= 100".into());
        }
        if self.downstream.blocking_correlation_score_margin > 100 {
            return invalid(
                "downstream.blocking_correlation_score_margin",
                "must be <= 100".into(),
            );
        }
        if self.route.breakdown_limit == 0 {
            return invalid("route.breakdown_limit", "must be > 0".into());
        }
        for (path, num, den) in [
            (
                "route.slowest_to_fastest_p95_ratio",
                self.route.slowest_to_fastest_p95_ratio_numerator,
                self.route.slowest_to_fastest_p95_ratio_denominator,
            ),
            (
                "route.slowest_to_global_p95_ratio",
                self.route.slowest_to_global_p95_ratio_numerator,
                self.route.slowest_to_global_p95_ratio_denominator,
            ),
        ] {
            if num == 0 || den == 0 {
                return invalid(path, "numerator and denominator must be > 0".into());
            }
            if num < den {
                return invalid(path, "numerator must be >= denominator".into());
            }
        }
        if self.temporal.min_segment_request_count == 0 {
            return invalid("temporal.min_segment_request_count", "must be > 0".into());
        }
        if self.temporal.min_segment_request_count.saturating_mul(2)
            > self.temporal.min_request_count
        {
            return invalid(
                "temporal.min_segment_request_count",
                "min_segment_request_count * 2 must be <= temporal.min_request_count".into(),
            );
        }
        if self.temporal.share_shift_permille > 1000 {
            return invalid("temporal.share_shift_permille", "must be <= 1000".into());
        }
        if self.temporal.p95_shift_ratio_numerator == 0
            || self.temporal.p95_shift_ratio_denominator == 0
        {
            return invalid(
                "temporal.p95_shift_ratio",
                "numerator and denominator must be > 0".into(),
            );
        }
        if self.temporal.p95_shift_ratio_numerator < self.temporal.p95_shift_ratio_denominator {
            return invalid(
                "temporal.p95_shift_ratio",
                "numerator must be >= denominator".into(),
            );
        }
        if self
            .downstream
            .blocking_correlated_stage_patterns
            .is_empty()
        {
            return invalid(
                "downstream.blocking_correlated_stage_patterns",
                "must not be empty".into(),
            );
        }
        if self
            .downstream
            .blocking_correlated_stage_patterns
            .iter()
            .any(|p| p.trim().is_empty())
        {
            return invalid(
                "downstream.blocking_correlated_stage_patterns",
                "entries must be non-empty after trim".into(),
            );
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
impl Display for AnalyzeConfigError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidOverrideSyntax { raw } => write!(f, "invalid override syntax: {raw}"),
            Self::UnknownOverridePath { path, suggestion } => {
                if let Some(s) = suggestion {
                    write!(f, "unknown override path '{path}', did you mean '{s}'?")
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
            Self::MissingAnalyzerTable => write!(f, "missing [analyzer] table"),
            Self::MissingSchemaVersion => write!(f, "missing schema_version"),
            Self::UnsupportedSchemaVersion { found, supported } => write!(
                f,
                "unsupported schema_version {found}; supported {supported}"
            ),
            Self::InvalidToml { message } => write!(f, "invalid toml: {message}"),
        }
    }
}
impl Error for AnalyzeConfigError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AnalyzeOptionDescriptor {
    pub path: &'static str,
    pub default_value: &'static str,
    pub value_type: &'static str,
    pub affects: &'static str,
    pub description: &'static str,
    pub increasing: Option<&'static str>,
    pub decreasing: Option<&'static str>,
}
impl AnalyzeOptionDescriptor {
    #[must_use]
    pub const fn new(
        path: &'static str,
        default_value: &'static str,
        value_type: &'static str,
        affects: &'static str,
        description: &'static str,
        increasing: Option<&'static str>,
        decreasing: Option<&'static str>,
    ) -> Self {
        Self {
            path,
            default_value,
            value_type,
            affects,
            description,
            increasing,
            decreasing,
        }
    }
}
