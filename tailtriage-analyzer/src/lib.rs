#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

use std::collections::{BTreeMap, HashMap};

use serde::{Serialize, Serializer};

mod attribution;
mod confidence;
mod evidence;
mod options;
mod partial_evidence;
mod ratio;
mod route;
mod scoring;
mod slicing;
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
    normalize_run_permissive, summarize_run_validation, InFlightSnapshot, QueueEvent, Run,
    RuntimeSnapshot,
};

const ROUTE_DIVERGENCE_WARNING: &str =
    "Different routes show different primary suspects; inspect route_breakdowns before acting on the global suspect.";
const ROUTE_RUNTIME_ATTRIBUTION_WARNING: &str =
    "Runtime and in-flight signals are global and are not attributed to this route.";

/// Evidence-ranked diagnosis categories produced by heuristic triage.
///
/// These categories are leads for investigation and are not proof of root cause.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosisKind {
    /// Queue wait dominates request latency, suggesting application-level queue pressure.
    ApplicationQueuePressure,
    /// Blocking pool backlog suggests pressure in `spawn_blocking`-backed work.
    BlockingPoolPressure,
    /// Runtime scheduler queueing suggests potential executor pressure.
    ExecutorPressure,
    /// One stage dominates aggregate latency, suggesting downstream slowdown.
    DownstreamStageDominance,
    /// Captured signals are too sparse to rank stronger suspects.
    InsufficientEvidence,
}

impl DiagnosisKind {
    /// Returns the stable machine-readable diagnosis kind label.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::ApplicationQueuePressure => "application_queue_pressure",
            Self::BlockingPoolPressure => "blocking_pool_pressure",
            Self::ExecutorPressure => "executor_pressure",
            Self::DownstreamStageDominance => "downstream_stage_dominance",
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

/// Summary of the selected gauge's latest retained in-flight activity episode.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InflightTrend {
    /// Gauge name chosen as the dominant trend candidate.
    pub gauge: String,
    /// Number of retained snapshots in the episode, including a terminal zero.
    pub sample_count: usize,
    /// Peak in-flight count observed in the episode.
    pub peak_count: u64,
    /// p95 in-flight count for the episode.
    pub p95_count: u64,
    /// Episode-local net growth (`last - first`), or `None` when fewer than two
    /// samples make direction unavailable. `Some(0)` means observed flatness.
    pub growth_delta: Option<i64>,
    /// Growth rate in milli-counts/sec when valid run-relative microsecond timing permits it.
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
/// about duplicate completed IDs. Callers that require strict acceptance can compose
/// `tailtriage_core::validate_run_strict` before analysis. Users remain responsible for meaningful
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
/// let report = analyze_run(&run, AnalyzeOptions::default())?;
/// assert_eq!(report.request_count, 0);
/// # Ok::<(), tailtriage_analyzer::AnalyzeConfigError>(())
/// ```
///
/// # Errors
///
/// Returns [`AnalyzeConfigError`] when options fail semantic validation.
// The public operation takes ownership so callers can pass a configured value directly and the
// API has one consistent invocation shape for defaults, TOML, and CLI-derived options.
#[allow(clippy::needless_pass_by_value)]
pub fn analyze_run(run: &Run, options: AnalyzeOptions) -> Result<Report, AnalyzeConfigError> {
    options.validate()?;
    Ok(analyze_run_with_options(run, &options))
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

fn analyze_run_with_options(run: &Run, options: &AnalyzeOptions) -> Report {
    let worker_status = scoring::classify_worker_evidence(run);
    let normalized = normalize_run_permissive(run);
    let analysis_run = &normalized.run;
    let profile = PartialEvidenceProfile::from_run(analysis_run);
    let mut report = analyze_run_internal(analysis_run, worker_status, options);
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
        temporal::temporal_segments(analysis_run, run, &mut report.warnings, options);
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

fn analyze_run_internal(
    run: &Run,
    worker_status: Option<scoring::WorkerEvidenceStatus>,
    options: &AnalyzeOptions,
) -> Report {
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
    let inflight_candidate = dominant_inflight_candidate(&run.inflight);
    let inflight_trend = inflight_candidate
        .as_ref()
        .map(|candidate| candidate.trend.clone());

    let mut suspects = Vec::new();

    if let Some(queue_suspect) = scoring::queue_saturation_suspect(
        run,
        &request_time_shares.completed_queue,
        &request_time_shares.observed_queue,
        inflight_candidate.as_ref(),
        options,
    ) {
        suspects.push(queue_suspect);
    }

    if let Some(blocking_suspect) = scoring::blocking_pressure_suspect(run, options) {
        suspects.push(ScoredSuspect {
            suspect: blocking_suspect,
            basis: EvidenceBasis::Completed,
            executor_limitation: None,
        });
    }

    if let Some(executor_suspect) =
        scoring::executor_pressure_suspect(run, worker_status, inflight_candidate.as_ref(), options)
    {
        let (executor_suspect, executor_limitation) = executor_suspect;
        suspects.push(ScoredSuspect {
            suspect: executor_suspect,
            basis: EvidenceBasis::Completed,
            executor_limitation,
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
        ), basis: EvidenceBasis::Completed, executor_limitation: None });
    }

    let evidence_quality = evidence::evidence_quality(run, options);
    let ranked_suspects = finalize_scored_suspects(suspects, run, &evidence_quality, options);
    let warnings = analysis_warnings(run, &ranked_suspects, options);

    let mut ranked = ranked_suspects.into_iter();
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

fn finalize_scored_suspects(
    mut suspects: Vec<ScoredSuspect>,
    run: &Run,
    evidence_quality: &EvidenceQuality,
    options: &AnalyzeOptions,
) -> Vec<Suspect> {
    for scored in &mut suspects {
        scored.suspect.confidence =
            Confidence::from_score_with_options(scored.suspect.score, options);
    }
    confidence::apply_evidence_aware_confidence_caps_scored(
        &mut suspects,
        run,
        evidence_quality,
        options,
    );
    suspects.sort_by(final_suspect_order);
    suspects.into_iter().map(|s| s.suspect).collect()
}

fn final_suspect_order(a: &ScoredSuspect, b: &ScoredSuspect) -> std::cmp::Ordering {
    let a_insufficient = a.suspect.kind == DiagnosisKind::InsufficientEvidence;
    let b_insufficient = b.suspect.kind == DiagnosisKind::InsufficientEvidence;
    a_insufficient
        .cmp(&b_insufficient)
        .then_with(|| {
            confidence_rank(b.suspect.confidence).cmp(&confidence_rank(a.suspect.confidence))
        })
        .then_with(|| b.suspect.score.cmp(&a.suspect.score))
        .then_with(|| {
            diagnosis_kind_rank(&a.suspect.kind).cmp(&diagnosis_kind_rank(&b.suspect.kind))
        })
}

const fn confidence_rank(confidence: Confidence) -> u8 {
    match confidence {
        Confidence::High => 3,
        Confidence::Medium => 2,
        Confidence::Low => 1,
    }
}

const fn diagnosis_kind_rank(kind: &DiagnosisKind) -> u8 {
    match kind {
        DiagnosisKind::ApplicationQueuePressure => 0,
        DiagnosisKind::BlockingPoolPressure => 1,
        DiagnosisKind::ExecutorPressure => 2,
        DiagnosisKind::DownstreamStageDominance => 3,
        DiagnosisKind::InsufficientEvidence => 255,
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
        && primary_kind.is_some_and(|kind| *kind == DiagnosisKind::ApplicationQueuePressure)
    {
        warnings.push(
            "No queue events captured; queue saturation interpretation is limited.".to_string(),
        );
    }
    if run.stages.is_empty()
        && primary_kind.is_some_and(|kind| *kind == DiagnosisKind::DownstreamStageDominance)
    {
        warnings.push(
            "No stage events captured; downstream-stage interpretation is limited.".to_string(),
        );
    }
    let runtime_distinction_relevant = suspects.iter().any(|s| {
        s.kind == DiagnosisKind::BlockingPoolPressure || s.kind == DiagnosisKind::ExecutorPressure
    });
    let strong_non_runtime_primary = suspects.first().is_some_and(|s| {
        (s.kind == DiagnosisKind::ApplicationQueuePressure
            || s.kind == DiagnosisKind::DownstreamStageDominance)
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum InflightOrdering {
    RunRelative,
    UnixFallback,
}

#[derive(Clone, Debug)]
pub(crate) struct InflightCandidate {
    pub(crate) trend: InflightTrend,
    active: bool,
    pub(crate) ordering: InflightOrdering,
}

impl InflightCandidate {
    pub(crate) fn known_positive_growth(&self) -> bool {
        self.trend.growth_delta.is_some_and(|delta| delta > 0)
    }
}

#[cfg(test)]
fn dominant_inflight_trend(snapshots: &[InFlightSnapshot]) -> Option<InflightTrend> {
    dominant_inflight_candidate(snapshots).map(|candidate| candidate.trend)
}

fn dominant_inflight_candidate(snapshots: &[InFlightSnapshot]) -> Option<InflightCandidate> {
    let mut by_gauge: BTreeMap<&str, Vec<(usize, &InFlightSnapshot)>> = BTreeMap::new();
    for (index, snapshot) in snapshots.iter().enumerate() {
        by_gauge
            .entry(snapshot.gauge.as_str())
            .or_default()
            .push((index, snapshot));
    }

    by_gauge
        .into_iter()
        .filter_map(|(gauge, samples)| inflight_trend_for_gauge(gauge, samples))
        .max_by(compare_inflight_candidates)
}

fn inflight_trend_for_gauge(
    gauge: &str,
    mut samples: Vec<(usize, &InFlightSnapshot)>,
) -> Option<InflightCandidate> {
    if samples.is_empty() {
        return None;
    }

    let ordering = if samples.iter().all(|(_, sample)| sample.at_run_us.is_some()) {
        samples.sort_by_key(|(index, sample)| (sample.at_run_us.unwrap_or(0), *index));
        InflightOrdering::RunRelative
    } else {
        samples.sort_by_key(|(index, sample)| (sample.at_unix_ms, *index));
        InflightOrdering::UnixFallback
    };

    let mut latest = Vec::new();
    let mut open = false;
    for sample in samples {
        if sample.1.count > 0 {
            if !open {
                latest.clear();
                open = true;
            }
            latest.push(sample);
        } else if open {
            latest.push(sample);
            open = false;
        }
    }
    if latest.is_empty() {
        return None;
    }

    let counts = latest
        .iter()
        .map(|(_, sample)| sample.count)
        .collect::<Vec<_>>();
    let first = latest.first()?.1;
    let last = latest.last()?.1;
    let growth_delta = (latest.len() >= 2).then(|| signed_u64_delta(first.count, last.count));
    let growth_per_sec_milli = (latest.len() >= 2 && ordering == InflightOrdering::RunRelative)
        .then(|| {
            let times = latest
                .iter()
                .map(|(_, sample)| sample.at_run_us.unwrap_or(0))
                .collect::<Vec<_>>();
            let elapsed = times.last()?.checked_sub(*times.first()?)?;
            if elapsed == 0 || !times.windows(2).all(|pair| pair[0] <= pair[1]) {
                return None;
            }
            let rate = i128::from(growth_delta?) * 1_000_000_000i128 / i128::from(elapsed);
            Some(
                i64::try_from(rate.clamp(i128::from(i64::MIN), i128::from(i64::MAX)))
                    .expect("clamped growth rate fits i64"),
            )
        })
        .flatten();

    Some(InflightCandidate {
        active: last.count > 0,
        ordering,
        trend: InflightTrend {
            gauge: gauge.to_owned(),
            sample_count: counts.len(),
            peak_count: counts.iter().copied().max().unwrap_or(0),
            p95_count: percentile(&counts, 95, 100).unwrap_or(0),
            growth_delta,
            growth_per_sec_milli,
        },
    })
}

fn compare_inflight_candidates(
    left: &InflightCandidate,
    right: &InflightCandidate,
) -> std::cmp::Ordering {
    let left_positive = left.known_positive_growth();
    let right_positive = right.known_positive_growth();
    left.active
        .cmp(&right.active)
        .then_with(|| left_positive.cmp(&right_positive))
        .then_with(|| {
            if left_positive && right_positive {
                left.trend.growth_delta.cmp(&right.trend.growth_delta)
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .then_with(|| {
            if left_positive
                && right_positive
                && left.trend.growth_delta == right.trend.growth_delta
            {
                left.trend
                    .growth_per_sec_milli
                    .filter(|rate| *rate > 0)
                    .cmp(&right.trend.growth_per_sec_milli.filter(|rate| *rate > 0))
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .then_with(|| left.trend.p95_count.cmp(&right.trend.p95_count))
        .then_with(|| left.trend.peak_count.cmp(&right.trend.peak_count))
        .then_with(|| left.trend.gauge.cmp(&right.trend.gauge).reverse())
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
