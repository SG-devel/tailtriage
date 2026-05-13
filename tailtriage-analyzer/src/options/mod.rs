use serde::Serialize;
use std::error::Error;
use std::fmt::{Display, Formatter};

mod descriptors;
pub use descriptors::analyze_option_descriptors;

/// Semantic analyzer options grouped by triage domain.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct AnalyzeOptions {
    /// Queue-pressure thresholds used to rank queue-saturation suspects during triage.
    pub queueing: QueueingOptions,
    /// Blocking-pool heuristics used to rank blocking-pressure suspects during triage.
    pub blocking: BlockingOptions,
    /// Executor-pressure thresholds used when runtime queue evidence is available.
    pub executor: ExecutorOptions,
    /// Downstream-stage heuristics used to compare stage dominance against blocking evidence.
    pub downstream: DownstreamOptions,
    /// Score thresholds that map suspect scores into confidence buckets and ambiguity warnings.
    pub confidence: ConfidenceOptions,
    /// Evidence-quality thresholds that control low-sample warnings and confidence downgrades.
    pub evidence: EvidenceOptions,
    /// Route-level thresholds for optional route triage breakdown summaries.
    pub route: RouteOptions,
    /// Temporal-shift thresholds for optional early/late triage segment summaries.
    pub temporal: TemporalOptions,
}

macro_rules! opt_struct { ($(#[$smeta:meta])* $name:ident { $($(#[$fmeta:meta])* $f:ident : $t:ty = $d:expr),+ $(,)? }) => {
$(#[$smeta])*
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct $name { $($(#[$fmeta])* pub $f: $t),+ }
impl Default for $name { fn default() -> Self { Self { $($f: $d),+ } } }
}; }
opt_struct!(
    /// Queue-saturation suspect thresholds.
    QueueingOptions {
    /// Minimum p95 queue-share permille needed before queue-saturation suspect ranking can trigger.
    trigger_permille: u64 = 300
});
opt_struct!(
    /// Blocking-pressure suspect thresholds and minimum-sample guards.
    BlockingOptions {
    /// Minimum number of non-zero blocking queue samples needed before blocking signal can trigger.
    min_nonzero_samples_for_signal: usize = 2,
    /// Blocking queue-depth p95 threshold used for stronger blocking-pressure suspect evidence.
    strong_p95_threshold: u64 = 12,
    /// Blocking queue-depth peak threshold used for stronger blocking-pressure suspect evidence.
    strong_peak_threshold: u64 = 20,
    /// Minimum non-zero blocking sample share (permille) for stronger blocking-pressure suspect evidence.
    strong_nonzero_share_permille: u64 = 700,
    /// Minimum blocking sample count before strong blocking heuristics can trigger.
    strong_min_samples: usize = 30
});
opt_struct!(
    /// Executor-pressure suspect thresholds derived from runtime queue pressure.
    ExecutorOptions {
    /// Minimum runtime global-queue p95 needed before executor-pressure suspect ranking can trigger.
    min_global_queue_p95_for_signal: u64 = 1
});
opt_struct!(
    /// Downstream-stage suspect thresholds and blocking-correlation heuristics.
    DownstreamOptions { /// Minimum stage sample count required before downstream-stage suspect ranking can trigger.
    min_stage_samples: usize = 3, /// Stage-name substrings used to detect downstream evidence that may correlate with blocking work.
    blocking_correlated_stage_patterns: Vec<String> = vec!["spawn_blocking".to_owned(), "blocking_path".to_owned(), "blocking".to_owned()], /// Minimum score margin required before favoring downstream-stage suspects over blocking-correlated interpretations.
    blocking_correlation_score_margin: u8 = 2 });
opt_struct!(
    /// Confidence-bucket and ambiguity-warning score thresholds.
    ConfidenceOptions {
    /// Minimum suspect score treated as medium confidence.
    medium_score_threshold: u8 = 65,
    /// Minimum suspect score treated as high confidence.
    high_score_threshold: u8 = 85,
    /// Minimum top-suspect score before ambiguity heuristics may emit a warning.
    ambiguity_min_score: u8 = 60,
    /// Maximum score gap considered a near-tie for ambiguity warning heuristics.
    ambiguity_score_gap: u8 = 4
});
opt_struct!(
    /// Evidence-quality thresholds used for low-sample warnings.
    EvidenceOptions {
    /// Completed-request threshold below which low-sample evidence warnings apply.
    low_completed_request_threshold: usize = 20
});
opt_struct!(
    /// Route-breakdown thresholds used for route-level suspect comparisons.
    RouteOptions {
    /// Minimum per-route completed requests required for route breakdown inclusion.
    min_request_count: usize = 3,
    /// Maximum number of route breakdown entries emitted in one report.
    breakdown_limit: usize = 10,
    /// Whether to emit a warning when route-level primary suspects diverge from each other.
    emit_on_divergent_suspects: bool = true,
    /// Numerator for slowest-to-fastest route p95 ratio heuristic threshold.
    slowest_to_fastest_p95_ratio_numerator: u64 = 3,
    /// Denominator for slowest-to-fastest route p95 ratio heuristic threshold.
    slowest_to_fastest_p95_ratio_denominator: u64 = 2,
    /// Numerator for slowest-route to global p95 ratio heuristic threshold.
    slowest_to_global_p95_ratio_numerator: u64 = 5,
    /// Denominator for slowest-route to global p95 ratio heuristic threshold.
    slowest_to_global_p95_ratio_denominator: u64 = 4
});
opt_struct!(
    /// Temporal-shift thresholds used for early/late suspect comparisons.
    TemporalOptions {
    /// Minimum completed requests required before temporal segmentation heuristics run.
    min_request_count: usize = 20,
    /// Minimum completed requests required in each temporal segment for suspect comparison.
    min_segment_request_count: usize = 8,
    /// Minimum queue/service-share movement (permille) required to flag temporal suspect shift evidence.
    share_shift_permille: u64 = 200,
    /// Numerator for temporal p95 ratio movement heuristic threshold.
    p95_shift_ratio_numerator: u64 = 3,
    /// Denominator for temporal p95 ratio movement heuristic threshold.
    p95_shift_ratio_denominator: u64 = 2,
    /// Whether to emit temporal suspect-shift warnings when movement heuristics trigger.
    emit_on_suspect_shift: bool = true,
    /// Whether to suppress runtime-sparse suspect-shift warnings when supporting movement evidence is absent.
    suppress_runtime_sparse_suspect_shift_without_supporting_movement: bool = true
});

impl AnalyzeOptions {
    /// Applies queueing-option edits and returns updated options for fluent setup.
    #[must_use]
    pub fn with_queueing(mut self, f: impl FnOnce(&mut QueueingOptions)) -> Self {
        f(&mut self.queueing);
        self
    }
    /// Applies blocking-option edits and returns updated options for fluent setup.
    #[must_use]
    pub fn with_blocking(mut self, f: impl FnOnce(&mut BlockingOptions)) -> Self {
        f(&mut self.blocking);
        self
    }
    /// Applies executor-option edits and returns updated options for fluent setup.
    #[must_use]
    pub fn with_executor(mut self, f: impl FnOnce(&mut ExecutorOptions)) -> Self {
        f(&mut self.executor);
        self
    }
    /// Applies downstream-option edits and returns updated options for fluent setup.
    #[must_use]
    pub fn with_downstream(mut self, f: impl FnOnce(&mut DownstreamOptions)) -> Self {
        f(&mut self.downstream);
        self
    }
    /// Applies confidence-option edits and returns updated options for fluent setup.
    #[must_use]
    pub fn with_confidence(mut self, f: impl FnOnce(&mut ConfidenceOptions)) -> Self {
        f(&mut self.confidence);
        self
    }
    /// Applies evidence-option edits and returns updated options for fluent setup.
    #[must_use]
    pub fn with_evidence(mut self, f: impl FnOnce(&mut EvidenceOptions)) -> Self {
        f(&mut self.evidence);
        self
    }
    /// Applies route-option edits and returns updated options for fluent setup.
    #[must_use]
    pub fn with_route(mut self, f: impl FnOnce(&mut RouteOptions)) -> Self {
        f(&mut self.route);
        self
    }
    /// Applies temporal-option edits and returns updated options for fluent setup.
    #[must_use]
    pub fn with_temporal(mut self, f: impl FnOnce(&mut TemporalOptions)) -> Self {
        f(&mut self.temporal);
        self
    }
    /// Validates semantic analyzer thresholds and heuristic invariants before triage.
    ///
    /// # Errors
    ///
    /// Returns [`AnalyzeConfigError::InvalidConfigValue`] when any threshold or ratio is invalid.
    #[allow(clippy::too_many_lines)]
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
        for (num_path, den_path, num, den) in [
            (
                "route.slowest_to_fastest_p95_ratio_numerator",
                "route.slowest_to_fastest_p95_ratio_denominator",
                self.route.slowest_to_fastest_p95_ratio_numerator,
                self.route.slowest_to_fastest_p95_ratio_denominator,
            ),
            (
                "route.slowest_to_global_p95_ratio_numerator",
                "route.slowest_to_global_p95_ratio_denominator",
                self.route.slowest_to_global_p95_ratio_numerator,
                self.route.slowest_to_global_p95_ratio_denominator,
            ),
        ] {
            if num == 0 {
                return invalid(num_path, "must be > 0".into());
            }
            if den == 0 {
                return invalid(den_path, "must be > 0".into());
            }
            if num < den {
                return invalid(num_path, format!("must be >= {den_path}"));
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
        if self.temporal.p95_shift_ratio_numerator == 0 {
            return invalid("temporal.p95_shift_ratio_numerator", "must be > 0".into());
        }
        if self.temporal.p95_shift_ratio_denominator == 0 {
            return invalid("temporal.p95_shift_ratio_denominator", "must be > 0".into());
        }
        if self.temporal.p95_shift_ratio_numerator < self.temporal.p95_shift_ratio_denominator {
            return invalid(
                "temporal.p95_shift_ratio_numerator",
                "must be >= temporal.p95_shift_ratio_denominator".into(),
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

/// Validation and configuration errors for analyzer options and checked triage APIs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnalyzeConfigError {
    /// Invalid override assignment syntax.
    InvalidOverrideSyntax {
        /// Raw override string that failed `path=value` syntax parsing.
        raw: String,
    },
    /// Unknown semantic override path.
    UnknownOverridePath {
        /// Unknown semantic option path provided by the caller.
        path: String,
        /// Optional nearest known path hint.
        suggestion: Option<&'static str>,
    },
    /// Override value could not be parsed for its path type.
    InvalidOverrideValue {
        /// Option path that rejected the provided value.
        path: &'static str,
        /// Raw value string that could not be parsed for this path.
        value: String,
        /// Human-readable expected value shape for this path.
        expected: &'static str,
    },
    /// Semantic option value failed validation.
    InvalidConfigValue {
        /// Option path containing an invalid threshold or heuristic invariant.
        path: &'static str,
        /// Validation message describing why the value is invalid.
        message: String,
    },
    /// Missing `[analyzer]` table in configuration input.
    MissingAnalyzerTable,
    /// Missing `schema_version` in configuration input.
    MissingSchemaVersion,
    /// Unsupported `schema_version` in configuration input.
    UnsupportedSchemaVersion {
        /// Encountered schema version from input configuration.
        found: u64,
        /// Highest schema version supported by this analyzer build.
        supported: u64,
    },
    /// Invalid TOML error.
    InvalidToml {
        /// TOML parsing or decoding error details.
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
/// Human-readable metadata for one semantic analyzer option path.
pub struct AnalyzeOptionDescriptor {
    /// Stable analyzer option path name.
    pub path: &'static str,
    /// Default value string for this option path.
    pub default_value: &'static str,
    /// Rust type name for this option value.
    pub value_type: &'static str,
    /// Short label describing which triage heuristic area this option affects.
    pub affects: &'static str,
    /// Bounded explanation of this option's role in suspect ranking heuristics.
    pub description: &'static str,
    /// Effect summary when this threshold increases, if directional wording applies.
    pub increasing: Option<&'static str>,
    /// Effect summary when this threshold decreases, if directional wording applies.
    pub decreasing: Option<&'static str>,
}
impl AnalyzeOptionDescriptor {
    /// Creates a static descriptor entry for one semantic analyzer option path.
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
