#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

use std::collections::{BTreeMap, HashMap};

use serde::{Serialize, Serializer};

mod attribution;
mod confidence;
mod evidence;
mod options;
mod partial_evidence;
mod route;
mod scoring;
mod stage_attribution;
mod temporal;

pub use evidence::{EvidenceQuality, EvidenceQualityLevel, SignalCoverageStatus};
pub use options::{
    analyze_option_descriptors, AnalyzeConfigError, AnalyzeOptionDescriptor, AnalyzeOptions,
    BlockingOptions, ConfidenceOptions, DownstreamOptions, EvidenceOptions, ExecutorOptions,
    QueueingOptions, RouteOptions, TemporalOptions,
};
use partial_evidence::{EvidenceBasis, PartialEvidenceProfile, ScoredSuspect};
use tailtriage_core::{
    normalize_run_permissive, summarize_run_validation, validate_run_strict, InFlightSnapshot,
    QueueEvent, Run, RunValidationIssueCode, RuntimeSnapshot,
};

const ROUTE_DIVERGENCE_WARNING: &str =
    "Different routes show different primary suspects; inspect route_breakdowns before acting on the global suspect.";
const ROUTE_RUNTIME_ATTRIBUTION_WARNING: &str =
    "Runtime and in-flight signals are global and are not attributed to this route.";

/// Errors returned by strict run-artifact validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArtifactValidationError {
    /// More than one completed request event used the same `request_id`.
    DuplicateCompletedRequestId {
        /// Duplicate request IDs found in completed requests.
        request_ids: Vec<String>,
    },
    /// Stage or queue evidence referenced a `request_id` with no completed request.
    OrphanRequestScopedEvent {
        /// Section containing orphan request-scoped events.
        section: &'static str,
        /// Orphan request IDs found in that section.
        request_ids: Vec<String>,
    },
    /// Canonical core validation rejected another generic integrity issue.
    Core(tailtriage_core::RunValidationError),
}

impl std::fmt::Display for ArtifactValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateCompletedRequestId { request_ids } => write!(f, "strict artifact validation failed: duplicate_completed_request_id duplicate completed request_id value(s): {}", request_ids.join(", ")),
            Self::OrphanRequestScopedEvent { section, request_ids } => write!(f, "strict artifact validation failed: orphan_request_scoped_event orphan {section} request_id value(s) with no matching completed request: {}", request_ids.join(", ")),
            Self::Core(err) => err.fmt(f),
        }
    }
}
impl std::error::Error for ArtifactValidationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Core(err) => Some(err),
            Self::DuplicateCompletedRequestId { .. } | Self::OrphanRequestScopedEvent { .. } => {
                None
            }
        }
    }
}

/// Strictly validates request-scoped artifact invariants before analysis.
///
/// This delegates to canonical core strict validation for all generic `Run`
/// integrity failures, including metadata, required fields, timing, duplicate
/// request IDs, orphan children, parent-state, and containment failures. Default
/// analyzer entry points do not call this automatically; they keep
/// backward-compatible permissive behavior and emit warnings instead of failing.
///
/// # Errors
/// Returns [`ArtifactValidationError`] when core strict validation rejects the artifact.
pub fn validate_artifact_strict(run: &Run) -> Result<(), ArtifactValidationError> {
    match validate_run_strict(run) {
        Ok(()) => Ok(()),
        Err(err) => {
            if has_only_error_code(&err, RunValidationIssueCode::DuplicateCompletedRequestId) {
                let mut duplicate_ids = run
                    .requests
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| {
                        err.report().issues.iter().any(|issue| {
                            issue.code == RunValidationIssueCode::DuplicateCompletedRequestId
                                && issue.location.section == tailtriage_core::RunSection::Requests
                                && issue.location.index == Some(*index)
                        })
                    })
                    .map(|(_, request)| request.request_id.clone())
                    .collect::<Vec<_>>();
                duplicate_ids.sort();
                duplicate_ids.dedup();
                if !duplicate_ids.is_empty() {
                    return Err(ArtifactValidationError::DuplicateCompletedRequestId {
                        request_ids: duplicate_ids,
                    });
                }
            }
            if has_only_error_code(&err, RunValidationIssueCode::OrphanRequestScopedEvent) {
                let orphan_sections = err
                    .report()
                    .issues
                    .iter()
                    .filter(|issue| {
                        issue.severity == tailtriage_core::RunValidationSeverity::Error
                            && issue.code == RunValidationIssueCode::OrphanRequestScopedEvent
                    })
                    .map(|issue| issue.location.section)
                    .collect::<std::collections::BTreeSet<_>>();
                if let [section] = orphan_sections
                    .iter()
                    .copied()
                    .collect::<Vec<_>>()
                    .as_slice()
                {
                    let name = match *section {
                        tailtriage_core::RunSection::Stages => "stage",
                        tailtriage_core::RunSection::Queues => "queue",
                        _ => return Err(ArtifactValidationError::Core(err)),
                    };
                    let mut ids = err
                        .report()
                        .issues
                        .iter()
                        .filter(|issue| {
                            issue.code == RunValidationIssueCode::OrphanRequestScopedEvent
                                && issue.location.section == *section
                        })
                        .filter_map(|issue| issue.location.index)
                        .map(|index| {
                            if *section == tailtriage_core::RunSection::Stages {
                                run.stages[index].request_id.clone()
                            } else {
                                run.queues[index].request_id.clone()
                            }
                        })
                        .collect::<Vec<_>>();
                    ids.sort();
                    ids.dedup();
                    if !ids.is_empty() {
                        return Err(ArtifactValidationError::OrphanRequestScopedEvent {
                            section: name,
                            request_ids: ids,
                        });
                    }
                }
            }
            Err(ArtifactValidationError::Core(err))
        }
    }
}

fn has_only_error_code(
    err: &tailtriage_core::RunValidationError,
    code: RunValidationIssueCode,
) -> bool {
    let mut saw_error = false;
    for issue in &err.report().issues {
        if issue.severity == tailtriage_core::RunValidationSeverity::Error {
            saw_error = true;
            if issue.code != code {
                return false;
            }
        }
    }
    saw_error
}

/// Validates options and strict artifact invariants, then analyzes one [`Run`].
///
/// # Errors
/// Returns an error when analyzer options are invalid or strict artifact
/// validation fails.
pub fn try_analyze_run_strict_artifact(
    run: &Run,
    options: AnalyzeOptions,
) -> Result<Report, AnalyzeRunError> {
    options.validate()?;
    validate_artifact_strict(run)?;
    Ok(Analyzer::new(options).analyze_run(run))
}

/// Error returned by [`try_analyze_run_strict_artifact`].
#[derive(Debug)]
pub enum AnalyzeRunError {
    /// Analyzer option validation failed.
    Config(AnalyzeConfigError),
    /// Strict artifact validation failed.
    Artifact(ArtifactValidationError),
}

impl std::fmt::Display for AnalyzeRunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Config(err) => err.fmt(f),
            Self::Artifact(err) => err.fmt(f),
        }
    }
}

impl std::error::Error for AnalyzeRunError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Config(err) => Some(err),
            Self::Artifact(err) => Some(err),
        }
    }
}

impl From<AnalyzeConfigError> for AnalyzeRunError {
    fn from(value: AnalyzeConfigError) -> Self {
        Self::Config(value)
    }
}

impl From<ArtifactValidationError> for AnalyzeRunError {
    fn from(value: ArtifactValidationError) -> Self {
        Self::Artifact(value)
    }
}

/// Evidence-ranked diagnosis categories produced by heuristic triage.
///
/// These categories are leads for investigation and are not proof of root cause.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosisKind {
    /// Queue wait dominates request latency, suggesting application-level queue pressure.
    ApplicationQueueSaturation,
    /// Blocking pool backlog suggests pressure in `spawn_blocking`-backed work.
    BlockingPoolPressure,
    /// Runtime scheduler queueing suggests potential executor pressure.
    ExecutorPressureSuspected,
    /// One stage dominates aggregate latency, suggesting downstream slowdown.
    DownstreamStageDominates,
    /// Captured signals are too sparse to rank stronger suspects.
    InsufficientEvidence,
}

impl DiagnosisKind {
    /// Returns the stable machine-readable diagnosis kind label.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::ApplicationQueueSaturation => "application_queue_saturation",
            Self::BlockingPoolPressure => "blocking_pool_pressure",
            Self::ExecutorPressureSuspected => "executor_pressure_suspected",
            Self::DownstreamStageDominates => "downstream_stage_dominates",
            Self::InsufficientEvidence => "insufficient_evidence",
        }
    }
}

impl Serialize for DiagnosisKind {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
/// Confidence bucket derived from suspect score thresholds.
///
/// This is score-derived ranking confidence, not causal certainty.
pub enum Confidence {
    /// Weak signal quality relative to stronger suspects in the same report.
    Low,
    /// Moderate signal quality for triage follow-up.
    Medium,
    /// Strong signal quality for triage follow-up.
    High,
}

impl Confidence {
    pub(crate) fn from_score_with_options(score: u8, options: &AnalyzeOptions) -> Self {
        if score >= options.confidence.high_score_threshold {
            Self::High
        } else if score >= options.confidence.medium_score_threshold {
            Self::Medium
        } else {
            Self::Low
        }
    }
}

/// Evidence-ranked suspect produced by heuristic analysis.
///
/// Suspects are triage leads and should be validated with follow-up checks.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Suspect {
    /// Ranked suspect category.
    pub kind: DiagnosisKind,
    /// Relative ranking score in range `0..=100` (higher means stronger evidence).
    pub score: u8,
    /// Score-derived confidence bucket for triage prioritization.
    pub confidence: Confidence,
    /// Supporting evidence strings used to justify this suspect ranking.
    pub evidence: Vec<String>,
    /// Recommended next checks to validate or falsify this suspect.
    pub next_checks: Vec<String>,
    /// Machine-readable notes explaining confidence caps due to evidence limitations.
    pub confidence_notes: Vec<String>,
}

impl Suspect {
    fn new(
        kind: DiagnosisKind,
        score: u8,
        evidence: Vec<String>,
        next_checks: Vec<String>,
    ) -> Self {
        Self {
            kind,
            score,
            confidence: Confidence::from_score_with_options(score, &AnalyzeOptions::default()),
            evidence,
            next_checks,
            confidence_notes: Vec::new(),
        }
    }
}

/// Summary of one dominant in-flight gauge trend over the run window.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InflightTrend {
    /// Gauge name chosen as the dominant trend candidate.
    pub gauge: String,
    /// Number of snapshots seen for this gauge.
    pub sample_count: usize,
    /// Peak in-flight count observed for this gauge.
    pub peak_count: u64,
    /// p95 in-flight count for this gauge.
    pub p95_count: u64,
    /// Net growth (`last - first`) across the sampled run window.
    pub growth_delta: i64,
    /// Growth rate in milli-counts/sec, if timestamps permit calculation.
    pub growth_per_sec_milli: Option<i64>,
}

/// Rule-based triage report for one completed [`Run`] snapshot.
///
/// The report ranks evidence-backed suspects and suggests next checks.
/// It does not prove root cause and should be used as triage guidance.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Report {
    /// Number of request events considered in analysis.
    pub request_count: usize,
    /// p50 request latency in microseconds.
    pub p50_latency_us: Option<u64>,
    /// p95 request latency in microseconds.
    pub p95_latency_us: Option<u64>,
    /// p99 request latency in microseconds.
    pub p99_latency_us: Option<u64>,
    /// p95 queue-time share per request in permille (`0..=1000`).
    pub p95_queue_share_permille: Option<u64>,
    /// p95 non-queue service-time share per request in permille (`0..=1000`).
    pub p95_service_share_permille: Option<u64>,
    /// Dominant in-flight trend signal, when at least one in-flight gauge has samples.
    pub inflight_trend: Option<InflightTrend>,
    /// Non-fatal analysis warnings (for example, capture truncation notices).
    pub warnings: Vec<String>,
    /// Structured evidence coverage and interpretation quality summary.
    pub evidence_quality: EvidenceQuality,
    /// Highest-ranked suspect from this run.
    pub primary_suspect: Suspect,
    /// Lower-ranked suspects retained for follow-up triage.
    pub secondary_suspects: Vec<Suspect>,
    /// Supporting per-route triage summaries when route-level signal adds value.
    pub route_breakdowns: Vec<RouteBreakdown>,
    /// Supporting early/late temporal triage summaries when within-run shifts add value.
    pub temporal_segments: Vec<TemporalSegment>,
    /// Non-default analyzer configuration overrides used for this report, when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub analyzer_config: Option<AnalyzerConfigSummary>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
/// Summary of non-default analyzer options used during analysis.
pub struct AnalyzerConfigSummary {
    /// Analyzer config summary schema version.
    pub schema_version: u32,
    /// Non-default semantic analyzer options rendered as stable path/value pairs.
    pub non_default_options: Vec<AnalyzeConfigOverrideSummary>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
/// One non-default analyzer option override rendered as a stable path/value pair.
pub struct AnalyzeConfigOverrideSummary {
    /// Stable semantic option path.
    pub path: String,
    /// Stable string-rendered option value.
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
/// Supporting early/late temporal triage summary for one run.
pub struct TemporalSegment {
    /// Segment label, currently `early` or `late`.
    pub name: String,
    /// Completed request count included in this segment.
    pub request_count: usize,
    /// Earliest request start timestamp in the segment.
    pub started_at_unix_ms: Option<u64>,
    /// Latest request finish timestamp in the segment.
    pub finished_at_unix_ms: Option<u64>,
    /// p50 request latency for this segment in microseconds.
    pub p50_latency_us: Option<u64>,
    /// p95 request latency for this segment in microseconds.
    pub p95_latency_us: Option<u64>,
    /// p99 request latency for this segment in microseconds.
    pub p99_latency_us: Option<u64>,
    /// p95 queue-time share for this segment in permille.
    pub p95_queue_share_permille: Option<u64>,
    /// p95 non-queue service-time share for this segment in permille.
    pub p95_service_share_permille: Option<u64>,
    /// Evidence coverage summary for this segment.
    pub evidence_quality: EvidenceQuality,
    /// Highest-ranked segment-level suspect.
    pub primary_suspect: Suspect,
    /// Lower-ranked segment-level suspects for follow-up.
    pub secondary_suspects: Vec<Suspect>,
    /// Segment-scoped warnings and interpretation limits.
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
/// Supporting per-route triage summary derived from captured request route labels.
pub struct RouteBreakdown {
    /// Route or operation label from request capture.
    pub route: String,
    /// Completed request count included for this route.
    pub request_count: usize,
    /// p50 request latency for this route in microseconds.
    pub p50_latency_us: Option<u64>,
    /// p95 request latency for this route in microseconds.
    pub p95_latency_us: Option<u64>,
    /// p99 request latency for this route in microseconds.
    pub p99_latency_us: Option<u64>,
    /// p95 queue-time share for this route in permille.
    pub p95_queue_share_permille: Option<u64>,
    /// p95 non-queue service-time share for this route in permille.
    pub p95_service_share_permille: Option<u64>,
    /// Evidence coverage summary for this route-filtered analysis.
    pub evidence_quality: EvidenceQuality,
    /// Highest-ranked route-level suspect.
    pub primary_suspect: Suspect,
    /// Lower-ranked route-level suspects for follow-up.
    pub secondary_suspects: Vec<Suspect>,
    /// Route-scoped warnings and interpretation limits.
    pub warnings: Vec<String>,
}

/// Analyzes one completed [`Run`] with rule-based heuristics and returns a triage report.
///
/// The analysis ranks evidence-backed suspects and next checks; it does not
/// claim causal certainty or proven root cause.
///
/// `request_id` is the per-run identity of one completed logical request/work item.
/// It must be unique among completed requests in a `Run`, and stage/queue events
/// must reuse that ID only for the same logical request. Default analysis warns
/// about duplicate completed IDs; use [`validate_artifact_strict`] or
/// [`try_analyze_run_strict_artifact`] to reject duplicate or orphan request-scoped
/// evidence before analysis. Users remain responsible for meaningful
/// instrumentation and request-boundary semantics.
///
/// # Examples
///
/// Library API example (this does not use the CLI file-loader contract):
///
/// ```
/// use tailtriage_analyzer::{analyze_run, AnalyzeOptions};
/// use tailtriage_core::{
///     CaptureMode, EffectiveCoreConfig, Run, RunMetadata, UnfinishedRequests, SCHEMA_VERSION,
/// };
///
/// let run = Run {
///     schema_version: SCHEMA_VERSION,
///     metadata: RunMetadata {
///         run_id: "run-1".to_string(),
///         service_name: "svc".to_string(),
///         service_version: None,
///         started_at_unix_ms: 1,
///         finished_at_unix_ms: 2,
///         finalized_at_unix_ms: Some(2),
///         mode: CaptureMode::Light,
///         effective_core_config: Some(EffectiveCoreConfig {
///             mode: CaptureMode::Light,
///             capture_limits: CaptureMode::Light.core_defaults(),
///             strict_lifecycle: false,
///         }),
///         effective_tokio_sampler_config: None,
///         host: None,
///         pid: None,
///         lifecycle_warnings: Vec::new(),
///         unfinished_requests: UnfinishedRequests::default(),
///         run_end_reason: None,
///     },
///     requests: vec![],
///     stages: vec![],
///     queues: vec![],
///     inflight: vec![],
///     runtime_snapshots: vec![],
///     truncation: Default::default(),
/// };
///
/// // `analyze_run(&Run, AnalyzeOptions)` can operate on an in-memory run with zero requests.
/// let report = analyze_run(&run, AnalyzeOptions::default());
/// assert_eq!(report.request_count, 0);
/// ```
///
/// # Panics
///
/// Panics if `options` fails semantic validation. Use [`try_analyze_run`] to handle invalid options as errors.
#[must_use]
pub fn analyze_run(run: &Run, options: AnalyzeOptions) -> Report {
    if let Err(err) = options.validate() {
        panic!("invalid AnalyzeOptions passed to analyze_run: {err}");
    }
    Analyzer::new(options).analyze_run(run)
}

/// Analyzes one completed [`Run`] with validated options and returns a triage report.
///
/// # Errors
///
/// Returns an error when options fail semantic validation.
pub fn try_analyze_run(run: &Run, options: AnalyzeOptions) -> Result<Report, AnalyzeConfigError> {
    options.validate()?;
    Ok(Analyzer::new(options).analyze_run(run))
}

/// Renders analyzer [`Report`] JSON in compact form.
///
/// This renders analyzer report JSON (the diagnosis output), not raw run artifact JSON.
///
/// # Errors
///
/// Returns any serialization error from `serde_json::to_string`.
#[must_use = "The rendered JSON string should be used for output or transport."]
pub fn render_json(report: &Report) -> Result<String, serde_json::Error> {
    serde_json::to_string(report)
}

/// Renders analyzer [`Report`] JSON in canonical pretty form.
///
/// This renders analyzer report JSON (the diagnosis output), not raw run artifact JSON.
/// The pretty output is intended as the canonical renderer for CLI JSON output.
///
/// # Errors
///
/// Returns any serialization error from `serde_json::to_string_pretty`.
#[must_use = "The rendered JSON string should be used for output or transport."]
pub fn render_json_pretty(report: &Report) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(report)
}

/// Analyzes one in-memory [`Run`] and returns compact analyzer [`Report`] JSON.
///
/// This analyzes a run artifact already loaded in memory and returns analyzer report JSON,
/// not raw run artifact JSON.
///
/// # Errors
///
/// Returns any serialization error from [`render_json`].
#[must_use = "The rendered JSON string should be used for output or transport."]
pub fn analyze_run_json(
    run: &tailtriage_core::Run,
    options: AnalyzeOptions,
) -> Result<String, serde_json::Error> {
    let report = analyze_run(run, options);
    render_json(&report)
}

/// Analyzes one in-memory [`Run`] and returns compact analyzer [`Report`] JSON.
///
/// # Errors
///
/// Returns an error when options fail validation or JSON serialization fails.
pub fn try_analyze_run_json(
    run: &tailtriage_core::Run,
    options: AnalyzeOptions,
) -> Result<String, AnalyzeConfigError> {
    let report = try_analyze_run(run, options)?;
    render_json(&report).map_err(|error| AnalyzeConfigError::InvalidConfigValue {
        path: "analyzer.report_json",
        message: format!("report serialization failed: {error}"),
    })
}

/// Analyzes one in-memory [`Run`] and returns canonical pretty analyzer [`Report`] JSON.
///
/// This analyzes a run artifact already loaded in memory and returns analyzer report JSON,
/// not raw run artifact JSON. The pretty output is intended for CLI JSON output.
///
/// # Errors
///
/// Returns any serialization error from [`render_json_pretty`].
#[must_use = "The rendered JSON string should be used for output or transport."]
pub fn analyze_run_json_pretty(
    run: &tailtriage_core::Run,
    options: AnalyzeOptions,
) -> Result<String, serde_json::Error> {
    let report = analyze_run(run, options);
    render_json_pretty(&report)
}

/// Analyzes one in-memory [`Run`] and returns pretty analyzer [`Report`] JSON.
///
/// # Errors
///
/// Returns an error when options fail validation or JSON serialization fails.
pub fn try_analyze_run_json_pretty(
    run: &tailtriage_core::Run,
    options: AnalyzeOptions,
) -> Result<String, AnalyzeConfigError> {
    let report = try_analyze_run(run, options)?;
    render_json_pretty(&report).map_err(|error| AnalyzeConfigError::InvalidConfigValue {
        path: "analyzer.report_json",
        message: format!("report serialization failed: {error}"),
    })
}

/// Reusable analyzer configured with [`AnalyzeOptions`].
#[derive(Debug, Clone, Default)]
pub struct Analyzer {
    options: AnalyzeOptions,
}

impl Analyzer {
    /// Creates an analyzer with the provided options.
    #[must_use]
    pub const fn new(options: AnalyzeOptions) -> Self {
        Self { options }
    }

    /// Analyzes one completed [`Run`] (or stable snapshot equivalent) and returns a triage report.
    #[must_use]
    pub fn analyze_run(&self, run: &Run) -> Report {
        analyze_run_with_options(run, &self.options)
    }
}

fn analyze_run_with_options(run: &Run, options: &AnalyzeOptions) -> Report {
    let normalized = normalize_run_permissive(run);
    let analysis_run = &normalized.run;
    let profile = PartialEvidenceProfile::from_run(analysis_run);
    let mut report = analyze_run_internal(analysis_run, options);
    if profile.has_partial() {
        push_unique(&mut report.warnings, partial_evidence::PARTIAL_WARNING);
    }
    let validation_warnings = summarize_run_validation(&normalized);
    report.warnings.splice(0..0, validation_warnings.clone());
    report.evidence_quality.limitations.extend(
        validation_warnings
            .into_iter()
            .map(|warning| format!("Validation limitation: {warning}")),
    );
    let route_context = route::route_breakdowns(analysis_run, &report, options);
    if route_context.warn_on_divergence {
        report.warnings.push(ROUTE_DIVERGENCE_WARNING.to_string());
    }
    report.route_breakdowns = route_context.breakdowns;
    report.temporal_segments =
        temporal::temporal_segments(analysis_run, &mut report.warnings, options);
    stable_dedup(&mut report.warnings);
    let overrides = options.non_default_overrides();
    report.analyzer_config = if overrides.is_empty() {
        None
    } else {
        Some(AnalyzerConfigSummary {
            schema_version: 1,
            non_default_options: overrides,
        })
    };
    report
}

fn push_unique(values: &mut Vec<String>, value: impl Into<String>) {
    let value = value.into();
    if !values.iter().any(|existing| existing == &value) {
        values.push(value);
    }
}

fn stable_dedup(values: &mut Vec<String>) {
    let mut deduped = Vec::with_capacity(values.len());
    for value in values.drain(..) {
        if !deduped.iter().any(|existing| existing == &value) {
            deduped.push(value);
        }
    }
    *values = deduped;
}

fn analyze_run_internal(run: &Run, options: &AnalyzeOptions) -> Report {
    let request_latencies = run
        .requests
        .iter()
        .map(|request| request.latency_us)
        .collect::<Vec<_>>();

    let p50_latency_us = percentile(&request_latencies, 50, 100);
    let p95_latency_us = percentile(&request_latencies, 95, 100);
    let p99_latency_us = percentile(&request_latencies, 99, 100);
    let request_time_shares = request_time_shares(run);
    let p95_queue_share_permille = percentile(&request_time_shares.completed_queue, 95, 100);
    let p95_service_share_permille = percentile(&request_time_shares.completed_service, 95, 100);
    let inflight_trend = dominant_inflight_trend(&run.inflight);

    let mut suspects = Vec::new();

    if let Some(queue_suspect) = scoring::queue_saturation_suspect(
        run,
        &request_time_shares.completed_queue,
        &request_time_shares.observed_queue,
        inflight_trend.as_ref(),
        options,
    ) {
        suspects.push(queue_suspect);
    }

    if let Some(blocking_suspect) = scoring::blocking_pressure_suspect(run, options) {
        suspects.push(ScoredSuspect {
            suspect: blocking_suspect,
            basis: EvidenceBasis::Completed,
        });
    }

    if let Some(executor_suspect) =
        scoring::executor_pressure_suspect(run, inflight_trend.as_ref(), options)
    {
        suspects.push(ScoredSuspect {
            suspect: executor_suspect,
            basis: EvidenceBasis::Completed,
        });
    }

    if let Some(stage_suspect) = scoring::downstream_stage_suspect(run, options) {
        suspects.push(stage_suspect);
    }

    if suspects.is_empty() {
        suspects.push(ScoredSuspect { suspect: Suspect::new(
            DiagnosisKind::InsufficientEvidence,
            50,
            vec![
                "Not enough queue, stage, or runtime signals to rank a stronger suspect."
                    .to_string(),
            ],
            vec![
                "Wrap critical awaits with queue(...).await_on(...), and use stage(...).await_on(...) for Result-returning work or stage(...).await_value(...) for infallible work.".to_string(),
                "Enable RuntimeSampler during the run to capture runtime pressure signals."
                    .to_string(),
            ],
        ), basis: EvidenceBasis::Completed });
    }

    suspects.sort_by_key(|scored| std::cmp::Reverse(scored.suspect.score));

    let plain_for_warnings = suspects
        .iter()
        .map(|s| s.suspect.clone())
        .collect::<Vec<_>>();
    let warnings = analysis_warnings(run, &plain_for_warnings, options);
    let evidence_quality = evidence::evidence_quality(run, options);

    for scored in &mut suspects {
        scored.suspect.confidence =
            Confidence::from_score_with_options(scored.suspect.score, options);
    }
    confidence::apply_evidence_aware_confidence_caps_scored(
        &mut suspects,
        run,
        &evidence_quality,
        options,
    );

    let mut ranked = suspects.into_iter().map(|s| s.suspect);
    let primary_suspect = ranked.next().unwrap_or_else(|| {
        Suspect::new(
            DiagnosisKind::InsufficientEvidence,
            50,
            vec!["No diagnosis signals were captured for this run.".to_string()],
            vec!["Verify that request, queue, or stage instrumentation is enabled.".to_string()],
        )
    });

    Report {
        request_count: run.requests.len(),
        p50_latency_us,
        p95_latency_us,
        p99_latency_us,
        p95_queue_share_permille,
        p95_service_share_permille,
        inflight_trend,
        warnings,
        evidence_quality,
        primary_suspect,
        secondary_suspects: ranked.collect(),
        route_breakdowns: Vec::new(),
        temporal_segments: Vec::new(),
        analyzer_config: None,
    }
}

fn ambiguity_warning(suspects: &[Suspect], options: &AnalyzeOptions) -> Option<String> {
    let mut ranked = suspects
        .iter()
        .filter(|s| s.kind != DiagnosisKind::InsufficientEvidence)
        .collect::<Vec<_>>();
    ranked.sort_by_key(|s| std::cmp::Reverse(s.score));
    if ranked.len() >= 2
        && ranked[0].score >= options.confidence.ambiguity_min_score
        && ranked[1].score >= options.confidence.ambiguity_min_score
        && ranked[0].score.abs_diff(ranked[1].score) <= options.confidence.ambiguity_score_gap
    {
        Some("Top suspects are close in score; treat ranking as ambiguous and validate both with next checks.".to_string())
    } else {
        None
    }
}

fn analysis_warnings(run: &Run, suspects: &[Suspect], options: &AnalyzeOptions) -> Vec<String> {
    let mut warnings = evidence::truncation_warnings(run);
    if run.requests.len() < options.evidence.low_completed_request_threshold {
        warnings.push(
            "Low completed-request count; diagnosis ranking may be unstable for this run window."
                .to_string(),
        );
    }
    let primary_kind = suspects.first().map(|s| &s.kind);
    if run.queues.is_empty()
        && primary_kind.is_some_and(|kind| *kind == DiagnosisKind::ApplicationQueueSaturation)
    {
        warnings.push(
            "No queue events captured; queue saturation interpretation is limited.".to_string(),
        );
    }
    if run.stages.is_empty()
        && primary_kind.is_some_and(|kind| *kind == DiagnosisKind::DownstreamStageDominates)
    {
        warnings.push(
            "No stage events captured; downstream-stage interpretation is limited.".to_string(),
        );
    }
    let runtime_distinction_relevant = suspects.iter().any(|s| {
        s.kind == DiagnosisKind::BlockingPoolPressure
            || s.kind == DiagnosisKind::ExecutorPressureSuspected
    });
    let strong_non_runtime_primary = suspects.first().is_some_and(|s| {
        (s.kind == DiagnosisKind::ApplicationQueueSaturation
            || s.kind == DiagnosisKind::DownstreamStageDominates)
            && s.score >= options.confidence.high_score_threshold
    });

    if run.runtime_snapshots.is_empty() {
        if !strong_non_runtime_primary {
            warnings.push("No runtime snapshots captured; executor and blocking-pressure interpretation is limited.".to_string());
        }
    } else if runtime_distinction_relevant
        && (run
            .runtime_snapshots
            .iter()
            .all(|s| s.blocking_queue_depth.is_none())
            || run
                .runtime_snapshots
                .iter()
                .all(|s| s.local_queue_depth.is_none()))
    {
        warnings.push("Runtime snapshots are missing blocking_queue_depth or local_queue_depth; separating executor vs blocking pressure is limited.".to_string());
    }
    if let Some(w) = ambiguity_warning(suspects, options) {
        warnings.push(w);
    }
    warnings
}

#[allow(dead_code)]
struct RequestTimeShares {
    queue: Vec<u64>,
    service: Vec<u64>,
    completed_queue: Vec<u64>,
    completed_service: Vec<u64>,
    observed_queue: Vec<u64>,
}

fn request_time_shares(run: &Run) -> RequestTimeShares {
    let mut completed_inputs_by_request: HashMap<&str, Vec<attribution::AttributionInput>> =
        HashMap::new();
    let mut observed_inputs_by_request: HashMap<&str, Vec<attribution::AttributionInput>> =
        HashMap::new();
    for queue in &run.queues {
        observed_inputs_by_request
            .entry(queue.request_id.as_str())
            .or_default()
            .push(queue_attribution_input(queue));
        if queue.completed {
            completed_inputs_by_request
                .entry(queue.request_id.as_str())
                .or_default()
                .push(queue_attribution_input(queue));
        }
    }

    let mut completed_queue_shares = Vec::new();
    let mut completed_service_shares = Vec::new();
    let mut observed_queue_shares = Vec::new();

    for request in &run.requests {
        if request.latency_us == 0 {
            continue;
        }

        let completed_events = completed_inputs_by_request
            .get(request.request_id.as_str())
            .map_or([].as_slice(), Vec::as_slice);
        let observed_events = observed_inputs_by_request
            .get(request.request_id.as_str())
            .map_or([].as_slice(), Vec::as_slice);
        let completed_wait =
            attribution::attributed_elapsed_duration(completed_events, request.latency_us)
                .duration_us
                .min(request.latency_us);
        let observed_wait =
            attribution::attributed_elapsed_duration(observed_events, request.latency_us)
                .duration_us
                .min(request.latency_us);
        let service_time = request.latency_us.saturating_sub(completed_wait);

        completed_queue_shares
            .push((completed_wait.saturating_mul(1_000) / request.latency_us).min(1_000));
        observed_queue_shares
            .push((observed_wait.saturating_mul(1_000) / request.latency_us).min(1_000));
        completed_service_shares
            .push((service_time.saturating_mul(1_000) / request.latency_us).min(1_000));
    }

    RequestTimeShares {
        queue: completed_queue_shares.clone(),
        service: completed_service_shares.clone(),
        completed_queue: completed_queue_shares,
        completed_service: completed_service_shares,
        observed_queue: observed_queue_shares,
    }
}

fn queue_attribution_input(queue: &QueueEvent) -> attribution::AttributionInput {
    attribution::AttributionInput {
        interval: queue.waited_from_run_us.zip(queue.waited_until_run_us),
        duration_us: queue.wait_us,
    }
}

fn runtime_metric_series(
    snapshots: &[RuntimeSnapshot],
    selector: impl Fn(&RuntimeSnapshot) -> Option<u64>,
) -> Vec<u64> {
    snapshots.iter().filter_map(selector).collect::<Vec<_>>()
}

fn dominant_inflight_trend(snapshots: &[InFlightSnapshot]) -> Option<InflightTrend> {
    let mut by_gauge: BTreeMap<&str, Vec<&InFlightSnapshot>> = BTreeMap::new();
    for snapshot in snapshots {
        by_gauge
            .entry(snapshot.gauge.as_str())
            .or_default()
            .push(snapshot);
    }

    by_gauge
        .into_iter()
        .filter_map(|(gauge, samples)| inflight_trend_for_gauge(gauge, samples))
        .max_by(|left, right| {
            left.peak_count
                .cmp(&right.peak_count)
                .then_with(|| left.p95_count.cmp(&right.p95_count))
                .then_with(|| left.gauge.cmp(&right.gauge).reverse())
        })
}

fn inflight_trend_for_gauge(
    gauge: &str,
    mut samples: Vec<&InFlightSnapshot>,
) -> Option<InflightTrend> {
    if samples.is_empty() {
        return None;
    }

    samples.sort_unstable_by(|left, right| {
        left.at_unix_ms
            .cmp(&right.at_unix_ms)
            .then_with(|| left.count.cmp(&right.count))
    });

    let counts = samples
        .iter()
        .map(|sample| sample.count)
        .collect::<Vec<_>>();
    let first = samples.first()?;
    let last = samples.last()?;
    let growth_delta = signed_u64_delta(first.count, last.count);
    let window_ms = last.at_unix_ms.saturating_sub(first.at_unix_ms);
    let growth_per_sec_milli = if window_ms == 0 {
        None
    } else {
        i64::try_from(window_ms)
            .ok()
            .map(|window_ms_i64| growth_delta.saturating_mul(1_000_000) / window_ms_i64)
    };

    Some(InflightTrend {
        gauge: gauge.to_owned(),
        sample_count: counts.len(),
        peak_count: counts.iter().copied().max().unwrap_or(0),
        p95_count: percentile(&counts, 95, 100).unwrap_or(0),
        growth_delta,
        growth_per_sec_milli,
    })
}

fn signed_u64_delta(start: u64, end: u64) -> i64 {
    if end >= start {
        i64::try_from(end - start).unwrap_or(i64::MAX)
    } else {
        -i64::try_from(start - end).unwrap_or(i64::MAX)
    }
}

fn percentile(values: &[u64], numerator: usize, denominator: usize) -> Option<u64> {
    let sorted = sorted_u64(values);
    percentile_sorted_u64(&sorted, numerator, denominator)
}

fn sorted_u64(values: &[u64]) -> Vec<u64> {
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    sorted
}

fn percentile_sorted_u64(values: &[u64], numerator: usize, denominator: usize) -> Option<u64> {
    if values.is_empty() {
        return None;
    }
    if denominator == 0 {
        return None;
    }

    let max_index = values.len().saturating_sub(1);
    let index = max_index
        .saturating_mul(numerator)
        .div_ceil(denominator)
        .min(max_index);
    values.get(index).copied()
}

pub use render::render_text;

mod render;

#[cfg(test)]
mod tests;
