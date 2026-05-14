use crate::AnalyzeConfigOverrideSummary;
use serde::Serialize;
use std::error::Error;
use std::fmt::{Display, Formatter};

mod descriptors;
mod toml;
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

/// Queue-saturation suspect thresholds.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct QueueingOptions {
    /// Minimum p95 queue-share permille needed before queue-saturation suspect ranking can trigger.
    pub trigger_permille: u64,
}

impl Default for QueueingOptions {
    fn default() -> Self {
        Self {
            trigger_permille: 300,
        }
    }
}

/// Blocking-pressure suspect thresholds and minimum-sample guards.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BlockingOptions {
    /// Minimum number of non-zero blocking queue samples needed before blocking signal can trigger.
    pub min_nonzero_samples_for_signal: usize,
    /// Blocking queue-depth p95 threshold used for stronger blocking-pressure suspect evidence.
    pub strong_p95_threshold: u64,
    /// Blocking queue-depth peak threshold used for stronger blocking-pressure suspect evidence.
    pub strong_peak_threshold: u64,
    /// Minimum non-zero blocking sample share (permille) for stronger blocking-pressure suspect evidence.
    pub strong_nonzero_share_permille: u64,
    /// Minimum blocking sample count before strong blocking heuristics can trigger.
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

/// Executor-pressure suspect thresholds derived from runtime queue pressure.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExecutorOptions {
    /// Minimum runtime global-queue p95 needed before executor-pressure suspect ranking can trigger.
    pub min_global_queue_p95_for_signal: u64,
}

impl Default for ExecutorOptions {
    fn default() -> Self {
        Self {
            min_global_queue_p95_for_signal: 1,
        }
    }
}

/// Downstream-stage suspect thresholds and blocking-correlation heuristics.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DownstreamOptions {
    /// Minimum stage sample count required before downstream-stage suspect ranking can trigger.
    pub min_stage_samples: usize,
    /// Stage-name substrings used to detect downstream evidence that may correlate with blocking work.
    pub blocking_correlated_stage_patterns: Vec<String>,
    /// Minimum score margin required before favoring downstream-stage suspects over blocking-correlated interpretations.
    pub blocking_correlation_score_margin: u8,
}

impl Default for DownstreamOptions {
    fn default() -> Self {
        Self {
            min_stage_samples: 3,
            blocking_correlated_stage_patterns: vec![
                "spawn_blocking".to_owned(),
                "blocking_path".to_owned(),
                "blocking".to_owned(),
            ],
            blocking_correlation_score_margin: 2,
        }
    }
}

/// Confidence-bucket and ambiguity-warning score thresholds.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConfidenceOptions {
    /// Minimum suspect score treated as medium confidence.
    pub medium_score_threshold: u8,
    /// Minimum suspect score treated as high confidence.
    pub high_score_threshold: u8,
    /// Minimum top-suspect score before ambiguity heuristics may emit a warning.
    pub ambiguity_min_score: u8,
    /// Maximum score gap considered a near-tie for ambiguity warning heuristics.
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

/// Evidence-quality thresholds used for low-sample warnings.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EvidenceOptions {
    /// Completed-request threshold below which low-sample evidence warnings apply.
    pub low_completed_request_threshold: usize,
}

impl Default for EvidenceOptions {
    fn default() -> Self {
        Self {
            low_completed_request_threshold: 20,
        }
    }
}

/// Route-breakdown thresholds used for route-level suspect comparisons.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RouteOptions {
    /// Minimum per-route completed requests required for route breakdown inclusion.
    pub min_request_count: usize,
    /// Maximum number of route breakdown entries emitted in one report.
    pub breakdown_limit: usize,
    /// Whether to emit a warning when route-level primary suspects diverge from each other.
    pub emit_on_divergent_suspects: bool,
    /// Numerator for slowest-to-fastest route p95 ratio heuristic threshold.
    pub slowest_to_fastest_p95_ratio_numerator: u64,
    /// Denominator for slowest-to-fastest route p95 ratio heuristic threshold.
    pub slowest_to_fastest_p95_ratio_denominator: u64,
    /// Numerator for slowest-route to global p95 ratio heuristic threshold.
    pub slowest_to_global_p95_ratio_numerator: u64,
    /// Denominator for slowest-route to global p95 ratio heuristic threshold.
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

/// Temporal-shift thresholds used for early/late suspect comparisons.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TemporalOptions {
    /// Minimum completed requests required before temporal segmentation heuristics run.
    pub min_request_count: usize,
    /// Minimum completed requests required in each temporal segment for suspect comparison.
    pub min_segment_request_count: usize,
    /// Minimum queue/service-share movement (permille) required to flag temporal suspect shift evidence.
    pub share_shift_permille: u64,
    /// Numerator for temporal p95 ratio movement heuristic threshold.
    pub p95_shift_ratio_numerator: u64,
    /// Denominator for temporal p95 ratio movement heuristic threshold.
    pub p95_shift_ratio_denominator: u64,
    /// Whether to emit temporal suspect-shift warnings when movement heuristics trigger.
    pub emit_on_suspect_shift: bool,
    /// Whether to suppress runtime-sparse suspect-shift warnings when supporting movement evidence is absent.
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

fn push_non_default_override(
    out: &mut Vec<AnalyzeConfigOverrideSummary>,
    path: &str,
    value: String,
) {
    out.push(AnalyzeConfigOverrideSummary {
        path: path.to_string(),
        value,
    });
}

fn queueing_non_default_overrides(
    out: &mut Vec<AnalyzeConfigOverrideSummary>,
    options: &AnalyzeOptions,
    defaults: &AnalyzeOptions,
) {
    if options.queueing.trigger_permille != defaults.queueing.trigger_permille {
        push_non_default_override(
            out,
            "queueing.trigger_permille",
            options.queueing.trigger_permille.to_string(),
        );
    }
}

fn blocking_non_default_overrides(
    out: &mut Vec<AnalyzeConfigOverrideSummary>,
    options: &AnalyzeOptions,
    defaults: &AnalyzeOptions,
) {
    if options.blocking.min_nonzero_samples_for_signal
        != defaults.blocking.min_nonzero_samples_for_signal
    {
        push_non_default_override(
            out,
            "blocking.min_nonzero_samples_for_signal",
            options.blocking.min_nonzero_samples_for_signal.to_string(),
        );
    }
    if options.blocking.strong_p95_threshold != defaults.blocking.strong_p95_threshold {
        push_non_default_override(
            out,
            "blocking.strong_p95_threshold",
            options.blocking.strong_p95_threshold.to_string(),
        );
    }
    if options.blocking.strong_peak_threshold != defaults.blocking.strong_peak_threshold {
        push_non_default_override(
            out,
            "blocking.strong_peak_threshold",
            options.blocking.strong_peak_threshold.to_string(),
        );
    }
    if options.blocking.strong_nonzero_share_permille
        != defaults.blocking.strong_nonzero_share_permille
    {
        push_non_default_override(
            out,
            "blocking.strong_nonzero_share_permille",
            options.blocking.strong_nonzero_share_permille.to_string(),
        );
    }
    if options.blocking.strong_min_samples != defaults.blocking.strong_min_samples {
        push_non_default_override(
            out,
            "blocking.strong_min_samples",
            options.blocking.strong_min_samples.to_string(),
        );
    }
}

fn executor_non_default_overrides(
    out: &mut Vec<AnalyzeConfigOverrideSummary>,
    options: &AnalyzeOptions,
    defaults: &AnalyzeOptions,
) {
    if options.executor.min_global_queue_p95_for_signal
        != defaults.executor.min_global_queue_p95_for_signal
    {
        push_non_default_override(
            out,
            "executor.min_global_queue_p95_for_signal",
            options.executor.min_global_queue_p95_for_signal.to_string(),
        );
    }
}

fn downstream_non_default_overrides(
    out: &mut Vec<AnalyzeConfigOverrideSummary>,
    options: &AnalyzeOptions,
    defaults: &AnalyzeOptions,
) {
    if options.downstream.min_stage_samples != defaults.downstream.min_stage_samples {
        push_non_default_override(
            out,
            "downstream.min_stage_samples",
            options.downstream.min_stage_samples.to_string(),
        );
    }
    if options.downstream.blocking_correlated_stage_patterns
        != defaults.downstream.blocking_correlated_stage_patterns
    {
        push_non_default_override(
            out,
            "downstream.blocking_correlated_stage_patterns",
            options
                .downstream
                .blocking_correlated_stage_patterns
                .join(","),
        );
    }
    if options.downstream.blocking_correlation_score_margin
        != defaults.downstream.blocking_correlation_score_margin
    {
        push_non_default_override(
            out,
            "downstream.blocking_correlation_score_margin",
            options
                .downstream
                .blocking_correlation_score_margin
                .to_string(),
        );
    }
}

fn confidence_non_default_overrides(
    out: &mut Vec<AnalyzeConfigOverrideSummary>,
    options: &AnalyzeOptions,
    defaults: &AnalyzeOptions,
) {
    if options.confidence.medium_score_threshold != defaults.confidence.medium_score_threshold {
        push_non_default_override(
            out,
            "confidence.medium_score_threshold",
            options.confidence.medium_score_threshold.to_string(),
        );
    }
    if options.confidence.high_score_threshold != defaults.confidence.high_score_threshold {
        push_non_default_override(
            out,
            "confidence.high_score_threshold",
            options.confidence.high_score_threshold.to_string(),
        );
    }
    if options.confidence.ambiguity_min_score != defaults.confidence.ambiguity_min_score {
        push_non_default_override(
            out,
            "confidence.ambiguity_min_score",
            options.confidence.ambiguity_min_score.to_string(),
        );
    }
    if options.confidence.ambiguity_score_gap != defaults.confidence.ambiguity_score_gap {
        push_non_default_override(
            out,
            "confidence.ambiguity_score_gap",
            options.confidence.ambiguity_score_gap.to_string(),
        );
    }
}

fn evidence_non_default_overrides(
    out: &mut Vec<AnalyzeConfigOverrideSummary>,
    options: &AnalyzeOptions,
    defaults: &AnalyzeOptions,
) {
    if options.evidence.low_completed_request_threshold
        != defaults.evidence.low_completed_request_threshold
    {
        push_non_default_override(
            out,
            "evidence.low_completed_request_threshold",
            options.evidence.low_completed_request_threshold.to_string(),
        );
    }
}

fn route_non_default_overrides(
    out: &mut Vec<AnalyzeConfigOverrideSummary>,
    options: &AnalyzeOptions,
    defaults: &AnalyzeOptions,
) {
    if options.route.min_request_count != defaults.route.min_request_count {
        push_non_default_override(
            out,
            "route.min_request_count",
            options.route.min_request_count.to_string(),
        );
    }
    if options.route.breakdown_limit != defaults.route.breakdown_limit {
        push_non_default_override(
            out,
            "route.breakdown_limit",
            options.route.breakdown_limit.to_string(),
        );
    }
    if options.route.emit_on_divergent_suspects != defaults.route.emit_on_divergent_suspects {
        push_non_default_override(
            out,
            "route.emit_on_divergent_suspects",
            options.route.emit_on_divergent_suspects.to_string(),
        );
    }
    if options.route.slowest_to_fastest_p95_ratio_numerator
        != defaults.route.slowest_to_fastest_p95_ratio_numerator
    {
        push_non_default_override(
            out,
            "route.slowest_to_fastest_p95_ratio_numerator",
            options
                .route
                .slowest_to_fastest_p95_ratio_numerator
                .to_string(),
        );
    }
    if options.route.slowest_to_fastest_p95_ratio_denominator
        != defaults.route.slowest_to_fastest_p95_ratio_denominator
    {
        push_non_default_override(
            out,
            "route.slowest_to_fastest_p95_ratio_denominator",
            options
                .route
                .slowest_to_fastest_p95_ratio_denominator
                .to_string(),
        );
    }
    if options.route.slowest_to_global_p95_ratio_numerator
        != defaults.route.slowest_to_global_p95_ratio_numerator
    {
        push_non_default_override(
            out,
            "route.slowest_to_global_p95_ratio_numerator",
            options
                .route
                .slowest_to_global_p95_ratio_numerator
                .to_string(),
        );
    }
    if options.route.slowest_to_global_p95_ratio_denominator
        != defaults.route.slowest_to_global_p95_ratio_denominator
    {
        push_non_default_override(
            out,
            "route.slowest_to_global_p95_ratio_denominator",
            options
                .route
                .slowest_to_global_p95_ratio_denominator
                .to_string(),
        );
    }
}

fn temporal_non_default_overrides(
    out: &mut Vec<AnalyzeConfigOverrideSummary>,
    options: &AnalyzeOptions,
    defaults: &AnalyzeOptions,
) {
    if options.temporal.min_request_count != defaults.temporal.min_request_count {
        push_non_default_override(
            out,
            "temporal.min_request_count",
            options.temporal.min_request_count.to_string(),
        );
    }
    if options.temporal.min_segment_request_count != defaults.temporal.min_segment_request_count {
        push_non_default_override(
            out,
            "temporal.min_segment_request_count",
            options.temporal.min_segment_request_count.to_string(),
        );
    }
    if options.temporal.share_shift_permille != defaults.temporal.share_shift_permille {
        push_non_default_override(
            out,
            "temporal.share_shift_permille",
            options.temporal.share_shift_permille.to_string(),
        );
    }
    if options.temporal.p95_shift_ratio_numerator != defaults.temporal.p95_shift_ratio_numerator {
        push_non_default_override(
            out,
            "temporal.p95_shift_ratio_numerator",
            options.temporal.p95_shift_ratio_numerator.to_string(),
        );
    }
    if options.temporal.p95_shift_ratio_denominator != defaults.temporal.p95_shift_ratio_denominator
    {
        push_non_default_override(
            out,
            "temporal.p95_shift_ratio_denominator",
            options.temporal.p95_shift_ratio_denominator.to_string(),
        );
    }
    if options.temporal.emit_on_suspect_shift != defaults.temporal.emit_on_suspect_shift {
        push_non_default_override(
            out,
            "temporal.emit_on_suspect_shift",
            options.temporal.emit_on_suspect_shift.to_string(),
        );
    }
    if options
        .temporal
        .suppress_runtime_sparse_suspect_shift_without_supporting_movement
        != defaults
            .temporal
            .suppress_runtime_sparse_suspect_shift_without_supporting_movement
    {
        push_non_default_override(
            out,
            "temporal.suppress_runtime_sparse_suspect_shift_without_supporting_movement",
            options
                .temporal
                .suppress_runtime_sparse_suspect_shift_without_supporting_movement
                .to_string(),
        );
    }
}
impl AnalyzeOptions {
    /// Returns sorted non-default semantic option overrides as stable path/value summaries.
    #[must_use]
    pub fn non_default_overrides(&self) -> Vec<AnalyzeConfigOverrideSummary> {
        let defaults = Self::default();
        let mut out = Vec::new();
        queueing_non_default_overrides(&mut out, self, &defaults);
        blocking_non_default_overrides(&mut out, self, &defaults);
        executor_non_default_overrides(&mut out, self, &defaults);
        downstream_non_default_overrides(&mut out, self, &defaults);
        confidence_non_default_overrides(&mut out, self, &defaults);
        evidence_non_default_overrides(&mut out, self, &defaults);
        route_non_default_overrides(&mut out, self, &defaults);
        temporal_non_default_overrides(&mut out, self, &defaults);
        out.sort_by(|a, b| a.path.cmp(&b.path));
        out
    }
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
