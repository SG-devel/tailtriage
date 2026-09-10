use crate::AnalyzeConfigOverrideSummary;
use serde::Serialize;
use std::error::Error;
use std::fmt::{Display, Formatter};

mod descriptors;
mod overrides;
mod registry;
mod toml;
pub use descriptors::analyze_option_descriptors;

/// Semantic configuration used by [`crate::analyze_run`], grouped by triage domain.
///
/// [`AnalyzeOptions::default`] is the canonical configuration. Direct mutation of the
/// public nested fields is supported, but assignment does not validate the resulting
/// configuration. Call [`AnalyzeOptions::validate`] explicitly when needed;
/// [`crate::analyze_run`] always calls it before analysis. Invalid semantic values return
/// [`AnalyzeConfigError::InvalidConfigValue`].
///
/// TOML and checked `path=value` overrides ultimately update this same semantic structure
/// through the shared option registry. [`analyze_option_descriptors`] exposes that registry's
/// stable paths, displayed defaults, Rust value types, affected behavior, descriptions, and
/// directional effects. [`AnalyzeOptions::non_default_overrides`] returns the non-default
/// semantic values that analysis records in [`crate::Report::analyzer_config`].
///
/// # Examples
///
/// ```
/// use tailtriage_analyzer::AnalyzeOptions;
///
/// let mut options = AnalyzeOptions::default();
/// options.queueing.trigger_permille = 450;
/// options.confidence.high_score_threshold = 90;
///
/// assert_eq!(options.queueing.trigger_permille, 450);
/// assert_eq!(options.confidence.high_score_threshold, 90);
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct AnalyzeOptions {
    /// Application-queue-pressure thresholds. See [`QueueingOptions`].
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

/// Application-queue-pressure suspect thresholds.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct QueueingOptions {
    /// Minimum p95 queue-time share needed for application queue pressure to be eligible.
    ///
    /// Unit: permille (`1000` is the whole request latency). Default: `300`. Valid range:
    /// `0..=1000`; larger values produce [`AnalyzeConfigError::InvalidConfigValue`] from
    /// [`AnalyzeOptions::validate`].
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
    /// Minimum count of non-zero blocking-queue samples needed for a blocking signal.
    /// Default: `2`. The semantic validator permits zero.
    pub min_nonzero_samples_for_signal: usize,
    /// Blocking queue-depth p95 count used for stronger blocking-pressure evidence.
    /// Default: `12`. The semantic validator permits zero.
    pub strong_p95_threshold: u64,
    /// Blocking queue-depth peak count used for stronger blocking-pressure evidence.
    /// Default: `20`. The semantic validator permits zero.
    pub strong_peak_threshold: u64,
    /// Minimum share of non-zero blocking samples for stronger blocking-pressure evidence.
    ///
    /// Unit: permille. Default: `700`. Valid range: `0..=1000`; larger values fail
    /// [`AnalyzeOptions::validate`] with [`AnalyzeConfigError::InvalidConfigValue`].
    pub strong_nonzero_share_permille: u64,
    /// Minimum blocking-sample count before strong blocking heuristics can trigger.
    /// Default: `30`. The semantic validator permits zero.
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
    /// Minimum runtime global-queue p95 depth count needed for executor-pressure eligibility.
    /// Default: `1`. The semantic validator permits zero.
    pub min_global_queue_p95_for_signal: u64,
    /// Minimum normalized p95 runnable-queue depth per worker for executor pressure.
    ///
    /// Unit: milli-tasks per worker (`1000` means one runnable task per worker). Default:
    /// `500`. The semantic validator permits zero.
    pub min_runnable_queue_per_worker_p95_milli_for_signal: u64,
}

impl Default for ExecutorOptions {
    fn default() -> Self {
        Self {
            min_global_queue_p95_for_signal: 1,
            min_runnable_queue_per_worker_p95_milli_for_signal: 500,
        }
    }
}

/// Downstream-stage suspect thresholds and blocking-correlation heuristics.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DownstreamOptions {
    /// Minimum distinct completed-request count with retained stage evidence for eligibility.
    /// Default: `3`. The semantic validator permits zero.
    pub min_stage_samples: usize,
    /// Stage-name substrings used to detect evidence that may correlate with blocking work.
    ///
    /// Default: `"spawn_blocking"`, `"blocking_path"`, and `"blocking"`. The list must be
    /// non-empty and every entry must be non-empty after trimming; otherwise validation returns
    /// [`AnalyzeConfigError::InvalidConfigValue`].
    pub blocking_correlated_stage_patterns: Vec<String>,
    /// Score-point margin required before favoring downstream over a blocking-correlated reading.
    ///
    /// Default: `2`. Valid range: `0..=100`; this is an analyzer score margin, not a
    /// probability or percentage. Larger values fail [`AnalyzeOptions::validate`].
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
    /// Minimum analyzer score points classified as medium confidence. Default: `65`.
    /// Must be no greater than [`Self::high_score_threshold`].
    pub medium_score_threshold: u8,
    /// Minimum analyzer score points classified as high confidence. Default: `85`.
    /// Valid range: `0..=100`; it must also be at least [`Self::medium_score_threshold`].
    pub high_score_threshold: u8,
    /// Minimum top-suspect analyzer score points before ambiguity can be reported.
    /// Default: `60`. Valid range: `0..=100`.
    pub ambiguity_min_score: u8,
    /// Maximum analyzer score-point gap treated as an ambiguous near-tie.
    /// Default: `4`. Valid range: `0..=100`.
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
    /// Completed-request count below which low-sample warnings and weak quality apply.
    /// Default: `20`. The semantic validator permits zero.
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
    /// Minimum completed-request count required for route breakdown inclusion.
    /// Default: `3`. The semantic validator permits zero.
    pub min_request_count: usize,
    /// Maximum count of route breakdown entries emitted in one report. Default: `10`.
    /// Must be greater than zero.
    pub breakdown_limit: usize,
    /// Whether divergent route-level primary suspects emit a warning. Default: `true`.
    pub emit_on_divergent_suspects: bool,
    /// Numerator of the dimensionless slowest-to-fastest p95 threshold ratio. Default: `3`.
    /// Must be greater than zero and at least the paired denominator.
    pub slowest_to_fastest_p95_ratio_numerator: u64,
    /// Denominator of the dimensionless slowest-to-fastest p95 threshold ratio. Default: `2`.
    /// Must be greater than zero; the paired numerator must be at least this value.
    pub slowest_to_fastest_p95_ratio_denominator: u64,
    /// Numerator of the dimensionless slowest-route-to-global p95 threshold ratio. Default: `5`.
    /// Must be greater than zero and at least the paired denominator.
    pub slowest_to_global_p95_ratio_numerator: u64,
    /// Denominator of the dimensionless slowest-route-to-global p95 threshold ratio. Default: `4`.
    /// Must be greater than zero; the paired numerator must be at least this value.
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
    /// Minimum completed-request count before temporal segmentation runs. Default: `20`.
    /// There is no independent positivity check on this field, but a valid configuration requires
    /// twice [`Self::min_segment_request_count`] (using saturating multiplication) to be no greater
    /// than this value. Because the segment minimum must be greater than zero, zero cannot form a
    /// valid configuration.
    pub min_request_count: usize,
    /// Minimum completed-request count in each temporal segment. Default: `8`.
    /// Must be greater than zero, and twice this value must be no greater than
    /// [`Self::min_request_count`].
    pub min_segment_request_count: usize,
    /// Minimum queue/service-share movement required to flag temporal shift evidence.
    /// Unit: permille. Default: `200`. Valid range: `0..=1000`.
    pub share_shift_permille: u64,
    /// Numerator of the dimensionless temporal p95 movement threshold ratio. Default: `3`.
    /// Must be greater than zero and at least the paired denominator.
    pub p95_shift_ratio_numerator: u64,
    /// Denominator of the dimensionless temporal p95 movement threshold ratio. Default: `2`.
    /// Must be greater than zero; the paired numerator must be at least this value.
    pub p95_shift_ratio_denominator: u64,
    /// Whether detected temporal suspect shifts emit warnings. Default: `true`.
    pub emit_on_suspect_shift: bool,
    /// Whether runtime-sparse shift warnings without supporting movement are suppressed.
    /// Default: `true`.
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

impl AnalyzeOptions {
    /// Returns sorted non-default semantic option overrides as stable path/value summaries.
    ///
    /// These are the values placed in [`crate::Report::analyzer_config`] by analysis. An empty
    /// result means the options equal [`AnalyzeOptions::default`].
    #[must_use]
    pub fn non_default_overrides(&self) -> Vec<AnalyzeConfigOverrideSummary> {
        registry::non_default_overrides(self)
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

/// Errors from semantic analyzer validation and the checked configuration input paths.
///
/// Direct field mutation is checked by [`AnalyzeOptions::validate`] or [`crate::analyze_run`]
/// and can directly produce only [`Self::InvalidConfigValue`]. Checked `path=value` overrides
/// can additionally fail syntax, path lookup, or type parsing before semantic validation. TOML
/// input can additionally fail table, schema-version, parsing, or decoding checks before the
/// same semantic validation.
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
                    write!(f, "unknown analyzer option '{path}'; did you mean '{s}'?")
                } else {
                    write!(f, "unknown analyzer option '{path}'")
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
/// Machine- and human-readable registry metadata for one supported semantic option path.
///
/// A descriptor provides a stable path, displayed default, Rust value type, affected behavior,
/// description, and optional directional effects. It does not carry valid ranges; use
/// [`AnalyzeOptions::validate`] for semantic validation.
pub struct AnalyzeOptionDescriptor {
    /// Stable analyzer option path name.
    path: &'static str,
    /// Default value string for this option path.
    default_value: &'static str,
    /// Rust type name for this option value.
    value_type: &'static str,
    /// Short label describing which triage heuristic area this option affects.
    affects: &'static str,
    /// Bounded explanation of this option's role in suspect ranking heuristics.
    description: &'static str,
    /// Effect summary when this threshold increases, if directional wording applies.
    increasing: Option<&'static str>,
    /// Effect summary when this threshold decreases, if directional wording applies.
    decreasing: Option<&'static str>,
}
impl AnalyzeOptionDescriptor {
    /// Creates a static descriptor entry for one semantic analyzer option path.
    #[must_use]
    pub(crate) const fn new(
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

    /// Returns the stable analyzer option path.
    #[must_use]
    pub const fn path(&self) -> &'static str {
        self.path
    }
    /// Returns the stable display form of the default value.
    #[must_use]
    pub const fn default_value(&self) -> &'static str {
        self.default_value
    }
    /// Returns the Rust value type name.
    #[must_use]
    pub const fn value_type(&self) -> &'static str {
        self.value_type
    }
    /// Returns the triage heuristic area affected by this option.
    #[must_use]
    pub const fn affects(&self) -> &'static str {
        self.affects
    }
    /// Returns the bounded description of this option.
    #[must_use]
    pub const fn description(&self) -> &'static str {
        self.description
    }
    /// Returns the effect of increasing this option, when applicable.
    #[must_use]
    pub const fn increasing(&self) -> Option<&'static str> {
        self.increasing
    }
    /// Returns the effect of decreasing this option, when applicable.
    #[must_use]
    pub const fn decreasing(&self) -> Option<&'static str> {
        self.decreasing
    }
}
