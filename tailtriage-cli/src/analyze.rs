use std::collections::{BTreeMap, HashMap};

use serde::{Serialize, Serializer};
use tailtriage_core::{InFlightSnapshot, RequestEvent, Run, RuntimeSnapshot};

const LOW_COMPLETED_REQUEST_THRESHOLD: usize = 20;
const QUEUE_SHARE_TRIGGER_PERMILLE: u64 = 300;
const MEDIUM_CONFIDENCE_SCORE_THRESHOLD: u8 = 65;
const HIGH_CONFIDENCE_SCORE_THRESHOLD: u8 = 85;
const DOWNSTREAM_MIN_STAGE_SAMPLES: usize = 3;
const AMBIGUITY_MIN_SCORE_THRESHOLD: u8 = 60;
const AMBIGUITY_SCORE_GAP_THRESHOLD: u8 = 4;
const SAMPLE_QUALITY_HIGH_SAMPLE_COUNT: usize = 100;
const SAMPLE_QUALITY_MEDIUM_SAMPLE_COUNT: usize = 40;
const SAMPLE_QUALITY_LOW_SAMPLE_COUNT: usize = 20;
const SAMPLE_QUALITY_MIN_NONZERO_SAMPLE_COUNT: usize = 8;
const ROUTE_MIN_REQUEST_COUNT: usize = 3;
const ROUTE_BREAKDOWN_LIMIT: usize = 10;
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
    fn from_score(score: u8) -> Self {
        if score >= HIGH_CONFIDENCE_SCORE_THRESHOLD {
            Self::High
        } else if score >= MEDIUM_CONFIDENCE_SCORE_THRESHOLD {
            Self::Medium
        } else {
            Self::Low
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
/// Overall evidence-quality level for this capture.
pub enum EvidenceQualityLevel {
    /// Evidence coverage is sufficient for a strong triage interpretation.
    Strong,
    /// Evidence coverage has important limitations.
    Partial,
    /// Evidence coverage is too sparse/truncated for stable interpretation.
    Weak,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
/// Coverage status for one signal family.
pub enum SignalCoverageStatus {
    /// Signal family has usable data.
    Present,
    /// Signal family is absent.
    Missing,
    /// Signal family exists but has limited interpretability.
    Partial,
    /// Signal family had capture drops due to truncation.
    Truncated,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
/// Structured capture-coverage and interpretation-quality summary.
pub struct EvidenceQuality {
    /// Number of completed request events captured.
    pub request_count: usize,
    /// Number of queue events captured.
    pub queue_event_count: usize,
    /// Number of stage events captured.
    pub stage_event_count: usize,
    /// Number of runtime snapshots captured.
    pub runtime_snapshot_count: usize,
    /// Number of in-flight snapshots captured.
    pub inflight_snapshot_count: usize,
    /// Coverage status for request events.
    pub requests: SignalCoverageStatus,
    /// Coverage status for queue events.
    pub queues: SignalCoverageStatus,
    /// Coverage status for stage events.
    pub stages: SignalCoverageStatus,
    /// Coverage status for runtime snapshots.
    pub runtime_snapshots: SignalCoverageStatus,
    /// Coverage status for in-flight snapshots.
    pub inflight_snapshots: SignalCoverageStatus,
    /// Whether any capture truncation limit was hit.
    pub truncated: bool,
    /// Number of dropped request events.
    pub dropped_requests: u64,
    /// Number of dropped stage events.
    pub dropped_stages: u64,
    /// Number of dropped queue events.
    pub dropped_queues: u64,
    /// Number of dropped in-flight snapshots.
    pub dropped_inflight_snapshots: u64,
    /// Number of dropped runtime snapshots.
    pub dropped_runtime_snapshots: u64,
    /// Overall quality level for this report's evidence coverage.
    pub quality: EvidenceQualityLevel,
    /// Interpretation limitations inferred from coverage/truncation.
    pub limitations: Vec<String>,
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
            confidence: Confidence::from_score(score),
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

/// Rule-based triage report for one run artifact.
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
    /// Optional early/late within-run hints emitted only for material shifts.
    pub temporal_segments: Vec<TemporalSegment>,
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

#[derive(Debug, Clone, PartialEq, Serialize)]
/// Supporting early/late temporal triage summary.
pub struct TemporalSegment {
    /// Segment label (`early` or `late`).
    pub name: String,
    /// Completed request count included in this segment.
    pub request_count: usize,
    /// Earliest request start timestamp in this segment.
    pub started_at_unix_ms: Option<u64>,
    /// Latest request finish timestamp in this segment.
    pub finished_at_unix_ms: Option<u64>,
    /// Segment p50 request latency in microseconds.
    pub p50_latency_us: Option<u64>,
    /// Segment p95 request latency in microseconds.
    pub p95_latency_us: Option<u64>,
    /// Segment p99 request latency in microseconds.
    pub p99_latency_us: Option<u64>,
    /// Segment p95 queue share in permille.
    pub p95_queue_share_permille: Option<u64>,
    /// Segment p95 non-queue service share in permille.
    pub p95_service_share_permille: Option<u64>,
    /// Segment evidence coverage summary.
    pub evidence_quality: EvidenceQuality,
    /// Top suspect for this segment.
    pub primary_suspect: Suspect,
    /// Lower-ranked suspects for this segment.
    pub secondary_suspects: Vec<Suspect>,
    /// Segment-local warnings.
    pub warnings: Vec<String>,
}

/// Analyzes one run artifact with rule-based heuristics and returns a triage report.
///
/// The analysis ranks evidence-backed suspects and next checks; it does not
/// claim causal certainty or proven root cause.
///
/// # Examples
///
/// Library API example (this does not use the CLI file-loader contract):
///
/// ```
/// use tailtriage_cli::analyze::analyze_run;
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
/// // `analyze_run(&Run)` can operate on an in-memory run with zero requests.
/// let report = analyze_run(&run);
/// assert_eq!(report.request_count, 0);
/// ```
#[must_use]
pub fn analyze_run(run: &Run) -> Report {
    let mut report = analyze_run_internal(run);
    let route_context = route_breakdowns(run, &report);
    if route_context.divergent {
        report.warnings.push(ROUTE_DIVERGENCE_WARNING.to_string());
    }
    report.route_breakdowns = route_context.breakdowns;
    let temporal = temporal_segments(run);
    if temporal.primary_shift {
        report
            .warnings
            .push("Temporal segments show different primary suspects; inspect temporal_segments before acting on the global suspect.".to_string());
    }
    if temporal.p95_shift {
        report.warnings.push(
            "Temporal segments show a large p95 latency shift between early and late requests."
                .to_string(),
        );
    }
    report.temporal_segments = temporal.segments;
    report
}

fn analyze_run_internal(run: &Run) -> Report {
    let request_latencies = run
        .requests
        .iter()
        .map(|request| request.latency_us)
        .collect::<Vec<_>>();

    let p50_latency_us = percentile(&request_latencies, 50, 100);
    let p95_latency_us = percentile(&request_latencies, 95, 100);
    let p99_latency_us = percentile(&request_latencies, 99, 100);
    let (queue_shares, service_shares) = request_time_shares(run);
    let p95_queue_share_permille = percentile(&queue_shares, 95, 100);
    let p95_service_share_permille = percentile(&service_shares, 95, 100);
    let inflight_trend = dominant_inflight_trend(&run.inflight);

    let mut suspects = Vec::new();

    if let Some(queue_suspect) = queue_saturation_suspect(run, inflight_trend.as_ref()) {
        suspects.push(queue_suspect);
    }

    if let Some(blocking_suspect) = blocking_pressure_suspect(run) {
        suspects.push(blocking_suspect);
    }

    if let Some(executor_suspect) = executor_pressure_suspect(run, inflight_trend.as_ref()) {
        suspects.push(executor_suspect);
    }

    if let Some(stage_suspect) = downstream_stage_suspect(run) {
        suspects.push(stage_suspect);
    }

    if suspects.is_empty() {
        suspects.push(Suspect::new(
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
        ));
    }

    suspects.sort_by_key(|suspect| std::cmp::Reverse(suspect.score));

    let warnings = analysis_warnings(run, &suspects);
    let evidence_quality = evidence_quality(run);

    apply_evidence_aware_confidence_caps(&mut suspects, run, &evidence_quality);

    let mut ranked = suspects.into_iter();
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
    }
}

struct TemporalSegmentContext {
    segments: Vec<TemporalSegment>,
    primary_shift: bool,
    p95_shift: bool,
}

fn temporal_segments(run: &Run) -> TemporalSegmentContext {
    if run.requests.len() < 20 {
        return TemporalSegmentContext {
            segments: vec![],
            primary_shift: false,
            p95_shift: false,
        };
    }
    let mut sorted = run.requests.clone();
    sorted.sort_by(|a, b| {
        a.started_at_unix_ms
            .cmp(&b.started_at_unix_ms)
            .then_with(|| a.request_id.cmp(&b.request_id))
    });
    let mid = sorted.len() / 2;
    let (early, late) = sorted.split_at(mid);
    if early.len() < 8 || late.len() < 8 {
        return TemporalSegmentContext {
            segments: vec![],
            primary_shift: false,
            p95_shift: false,
        };
    }
    let early_seg = build_temporal_segment(run, "early", early);
    let late_seg = build_temporal_segment(run, "late", late);
    let primary_shift = early_seg.primary_suspect.kind != late_seg.primary_suspect.kind;
    let p95_shift = match (early_seg.p95_latency_us, late_seg.p95_latency_us) {
        (Some(a), Some(b)) => {
            let lo = a.min(b);
            let hi = a.max(b);
            lo > 0 && hi.saturating_mul(2) >= lo.saturating_mul(3) && hi.saturating_sub(lo) >= 5_000
        }
        _ => false,
    };
    let share_shift = match (
        early_seg.p95_queue_share_permille,
        late_seg.p95_queue_share_permille,
        early_seg.p95_service_share_permille,
        late_seg.p95_service_share_permille,
    ) {
        (Some(eq), Some(lq), Some(es), Some(ls)) => {
            eq.abs_diff(lq) >= 200 || es.abs_diff(ls) >= 200
        }
        _ => false,
    };
    let evidence_shift = early_seg.evidence_quality.quality != late_seg.evidence_quality.quality;
    if !(primary_shift || p95_shift || share_shift || evidence_shift) {
        return TemporalSegmentContext {
            segments: vec![],
            primary_shift: false,
            p95_shift: false,
        };
    }
    TemporalSegmentContext {
        segments: vec![early_seg, late_seg],
        primary_shift,
        p95_shift,
    }
}

fn build_temporal_segment(run: &Run, name: &str, requests: &[RequestEvent]) -> TemporalSegment {
    let ids: Vec<String> = requests.iter().map(|r| r.request_id.clone()).collect();
    let min_start = requests.iter().map(|r| r.started_at_unix_ms).min();
    let max_finish = requests.iter().map(|r| r.finished_at_unix_ms).max();
    let mut filtered = filtered_run_for_ids(run, &ids);
    filtered.runtime_snapshots = run
        .runtime_snapshots
        .iter()
        .filter(|s| match (min_start, max_finish) {
            (Some(start), Some(end)) => s.at_unix_ms >= start && s.at_unix_ms <= end,
            _ => false,
        })
        .cloned()
        .collect();
    filtered.inflight = run
        .inflight
        .iter()
        .filter(|s| match (min_start, max_finish) {
            (Some(start), Some(end)) => s.at_unix_ms >= start && s.at_unix_ms <= end,
            _ => false,
        })
        .cloned()
        .collect();
    let analyzed = analyze_run_internal(&filtered);
    TemporalSegment {
        name: name.to_string(),
        request_count: analyzed.request_count,
        started_at_unix_ms: min_start,
        finished_at_unix_ms: max_finish,
        p50_latency_us: analyzed.p50_latency_us,
        p95_latency_us: analyzed.p95_latency_us,
        p99_latency_us: analyzed.p99_latency_us,
        p95_queue_share_permille: analyzed.p95_queue_share_permille,
        p95_service_share_permille: analyzed.p95_service_share_permille,
        evidence_quality: analyzed.evidence_quality,
        primary_suspect: analyzed.primary_suspect,
        secondary_suspects: analyzed.secondary_suspects,
        warnings: analyzed.warnings,
    }
}

struct RouteBreakdownContext {
    breakdowns: Vec<RouteBreakdown>,
    divergent: bool,
}

fn route_breakdowns(run: &Run, global: &Report) -> RouteBreakdownContext {
    let mut ids_by_route: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for request in &run.requests {
        ids_by_route
            .entry(request.route.clone())
            .or_default()
            .push(request.request_id.clone());
    }
    let eligible: Vec<(String, Vec<String>)> = ids_by_route
        .into_iter()
        .filter(|(_, ids)| ids.len() >= ROUTE_MIN_REQUEST_COUNT)
        .collect();
    if eligible.len() < 2 {
        return RouteBreakdownContext {
            breakdowns: vec![],
            divergent: false,
        };
    }

    let omitted_routes = run
        .requests
        .iter()
        .fold(BTreeMap::<String, usize>::new(), |mut acc, request| {
            *acc.entry(request.route.clone()).or_default() += 1;
            acc
        })
        .into_values()
        .filter(|count| *count < ROUTE_MIN_REQUEST_COUNT)
        .count();

    let mut candidates = Vec::new();
    for (route, request_ids) in eligible {
        let mut filtered = filtered_run_for_ids(run, &request_ids);
        filtered.runtime_snapshots = Vec::new();
        filtered.inflight = Vec::new();
        let mut analyzed = analyze_run_internal(&filtered);
        analyzed
            .warnings
            .push(ROUTE_RUNTIME_ATTRIBUTION_WARNING.to_string());
        candidates.push(RouteBreakdown {
            route,
            request_count: analyzed.request_count,
            p50_latency_us: analyzed.p50_latency_us,
            p95_latency_us: analyzed.p95_latency_us,
            p99_latency_us: analyzed.p99_latency_us,
            p95_queue_share_permille: analyzed.p95_queue_share_permille,
            p95_service_share_permille: analyzed.p95_service_share_permille,
            evidence_quality: analyzed.evidence_quality,
            primary_suspect: analyzed.primary_suspect,
            secondary_suspects: analyzed.secondary_suspects,
            warnings: analyzed.warnings,
        });
    }
    if !should_emit_route_breakdowns(global, &candidates) {
        return RouteBreakdownContext {
            breakdowns: vec![],
            divergent: false,
        };
    }
    let mut emitted = candidates;
    emitted.sort_by(|a, b| {
        b.p95_latency_us
            .cmp(&a.p95_latency_us)
            .then_with(|| b.request_count.cmp(&a.request_count))
            .then_with(|| a.route.cmp(&b.route))
    });
    emitted.truncate(ROUTE_BREAKDOWN_LIMIT);
    let divergent = route_divergence(&emitted);
    if omitted_routes > 0 {
        let note = format!(
            "Some routes are omitted from route_breakdowns because they have fewer than {ROUTE_MIN_REQUEST_COUNT} completed requests."
        );
        for breakdown in &mut emitted {
            breakdown.warnings.push(note.clone());
        }
    }
    RouteBreakdownContext {
        breakdowns: emitted,
        divergent,
    }
}

fn route_divergence(candidates: &[RouteBreakdown]) -> bool {
    candidates
        .iter()
        .map(|c| c.primary_suspect.kind.as_str())
        .collect::<std::collections::BTreeSet<_>>()
        .len()
        >= 2
}

fn should_emit_route_breakdowns(global: &Report, candidates: &[RouteBreakdown]) -> bool {
    if candidates.len() < 2 {
        return false;
    }
    if route_divergence(candidates) {
        return true;
    }
    let p95s: Vec<u64> = candidates.iter().filter_map(|c| c.p95_latency_us).collect();
    if p95s.len() < 2 {
        return false;
    }
    let slowest = *p95s.iter().max().unwrap_or(&0);
    let fastest = *p95s.iter().min().unwrap_or(&0);
    (fastest > 0 && slowest.saturating_mul(2) >= fastest.saturating_mul(3))
        || match global.p95_latency_us {
            Some(global_p95) if global_p95 > 0 => {
                slowest.saturating_mul(4) >= global_p95.saturating_mul(5)
            }
            _ => false,
        }
}

fn filtered_run_for_ids(run: &Run, request_ids: &[String]) -> Run {
    let request_ids: std::collections::HashSet<&str> =
        request_ids.iter().map(String::as_str).collect();
    let mut filtered = run.clone();
    filtered.requests = run
        .requests
        .iter()
        .filter(|r| request_ids.contains(r.request_id.as_str()))
        .cloned()
        .collect();
    filtered.stages = run
        .stages
        .iter()
        .filter(|s| request_ids.contains(s.request_id.as_str()))
        .cloned()
        .collect();
    filtered.queues = run
        .queues
        .iter()
        .filter(|q| request_ids.contains(q.request_id.as_str()))
        .cloned()
        .collect();
    filtered
}

fn apply_evidence_aware_confidence_caps(
    suspects: &mut [Suspect],
    run: &Run,
    evidence_quality: &EvidenceQuality,
) {
    let runtime_missing_key_fields = run.runtime_snapshots.is_empty()
        || run
            .runtime_snapshots
            .iter()
            .all(|snapshot| snapshot.blocking_queue_depth.is_none())
        || run
            .runtime_snapshots
            .iter()
            .all(|snapshot| snapshot.local_queue_depth.is_none())
        || run
            .runtime_snapshots
            .iter()
            .all(|snapshot| snapshot.global_queue_depth.is_none());
    let ambiguous = ambiguity_warning(suspects).is_some();
    for (i, suspect) in suspects.iter_mut().enumerate() {
        let mut cap = Confidence::High;
        let mut notes = Vec::new();
        let is_primary = i == 0;
        let is_insufficient = suspect.kind == DiagnosisKind::InsufficientEvidence;
        if !is_insufficient && evidence_quality.quality == EvidenceQualityLevel::Weak {
            cap = cap.min(Confidence::Medium);
        }
        if !is_insufficient && run.requests.is_empty() {
            cap = Confidence::Low;
            notes.push("Low completed-request count caps confidence.".to_string());
        } else if run.requests.len() < LOW_COMPLETED_REQUEST_THRESHOLD {
            if !is_insufficient {
                cap = cap.min(Confidence::Medium);
            }
            if is_primary {
                notes.push("Low completed-request count caps confidence.".to_string());
            }
        }
        if run.truncation.dropped_requests > 0 && !is_insufficient {
            cap = cap.min(Confidence::Medium);
            notes.push(
                "Capture truncation caps confidence because dropped evidence may affect ranking."
                    .to_string(),
            );
        }
        apply_family_evidence_caps(
            &suspect.kind,
            run,
            runtime_missing_key_fields,
            &mut cap,
            &mut notes,
        );
        if is_primary && ambiguous {
            cap = cap.min(Confidence::Medium);
            notes.push(
                "Top suspects are close in score; confidence is capped by ambiguity.".to_string(),
            );
        }
        let original = suspect.confidence;
        suspect.confidence = original.min(cap);
        let cap_changed_bucket = suspect.confidence != original;
        if cap_changed_bucket || (is_primary && ambiguous) {
            notes.sort();
            notes.dedup();
            suspect.confidence_notes = notes;
        } else {
            suspect.confidence_notes.clear();
        }
    }
}

fn apply_family_evidence_caps(
    kind: &DiagnosisKind,
    run: &Run,
    runtime_missing_key_fields: bool,
    cap: &mut Confidence,
    notes: &mut Vec<String>,
) {
    match kind {
        DiagnosisKind::ApplicationQueueSaturation => {
            if run.truncation.dropped_queues > 0 {
                *cap = (*cap).min(Confidence::Medium);
                notes.push(
                    "Capture truncation caps confidence because dropped evidence may affect ranking."
                        .to_string(),
                );
            }
            if run.queues.is_empty() {
                *cap = (*cap).min(Confidence::Medium);
                notes.push(
                    "Missing queue instrumentation limits queue-saturation confidence.".to_string(),
                );
            }
        }
        DiagnosisKind::DownstreamStageDominates => {
            if run.truncation.dropped_stages > 0 {
                *cap = (*cap).min(Confidence::Medium);
                notes.push(
                    "Capture truncation caps confidence because dropped evidence may affect ranking."
                        .to_string(),
                );
            }
            if run.stages.is_empty() {
                *cap = (*cap).min(Confidence::Medium);
                notes.push(
                    "Missing stage instrumentation limits downstream-stage confidence.".to_string(),
                );
            }
        }
        DiagnosisKind::BlockingPoolPressure | DiagnosisKind::ExecutorPressureSuspected => {
            if run.truncation.dropped_runtime_snapshots > 0 {
                *cap = (*cap).min(Confidence::Medium);
                notes.push(
                    "Capture truncation caps confidence because dropped evidence may affect ranking."
                        .to_string(),
                );
            }
            if runtime_missing_key_fields {
                *cap = (*cap).min(Confidence::Medium);
                notes.push(
                    "Missing runtime snapshots limit executor/blocking confidence.".to_string(),
                );
            }
        }
        DiagnosisKind::InsufficientEvidence => {}
    }
}

fn evidence_quality(run: &Run) -> EvidenceQuality {
    let requests = request_status(run);
    let queues = family_status(run.queues.is_empty(), run.truncation.dropped_queues);
    let stages = family_status(run.stages.is_empty(), run.truncation.dropped_stages);
    let runtime_snapshots = runtime_status(run);
    let inflight_snapshots = family_status(
        run.inflight.is_empty(),
        run.truncation.dropped_inflight_snapshots,
    );
    let limitations = evidence_limitations(run, queues, stages, runtime_snapshots);
    let non_request_truncated = matches!(queues, SignalCoverageStatus::Truncated)
        || matches!(stages, SignalCoverageStatus::Truncated)
        || matches!(runtime_snapshots, SignalCoverageStatus::Truncated)
        || matches!(inflight_snapshots, SignalCoverageStatus::Truncated);
    let explanatory_present =
        !run.queues.is_empty() || !run.stages.is_empty() || !run.runtime_snapshots.is_empty();
    let quality = if run.requests.is_empty()
        || run.requests.len() < LOW_COMPLETED_REQUEST_THRESHOLD
        || run.truncation.dropped_requests > 0
        || !explanatory_present
    {
        EvidenceQualityLevel::Weak
    } else if non_request_truncated
        || (run.queues.is_empty() && run.stages.is_empty())
        || runtime_snapshots == SignalCoverageStatus::Partial
    {
        EvidenceQualityLevel::Partial
    } else {
        // Runtime snapshots are optional; when queue/stage evidence is otherwise strong,
        // missing runtime is represented as a limitation, not an automatic downgrade.
        EvidenceQualityLevel::Strong
    };

    EvidenceQuality {
        request_count: run.requests.len(),
        queue_event_count: run.queues.len(),
        stage_event_count: run.stages.len(),
        runtime_snapshot_count: run.runtime_snapshots.len(),
        inflight_snapshot_count: run.inflight.len(),
        requests,
        queues,
        stages,
        runtime_snapshots,
        inflight_snapshots,
        truncated: run.truncation.is_truncated() || run.truncation.limits_hit,
        dropped_requests: run.truncation.dropped_requests,
        dropped_stages: run.truncation.dropped_stages,
        dropped_queues: run.truncation.dropped_queues,
        dropped_inflight_snapshots: run.truncation.dropped_inflight_snapshots,
        dropped_runtime_snapshots: run.truncation.dropped_runtime_snapshots,
        quality,
        limitations,
    }
}

fn request_status(run: &Run) -> SignalCoverageStatus {
    if run.requests.is_empty() {
        SignalCoverageStatus::Missing
    } else if run.truncation.dropped_requests > 0 {
        SignalCoverageStatus::Truncated
    } else if run.requests.len() < LOW_COMPLETED_REQUEST_THRESHOLD {
        SignalCoverageStatus::Partial
    } else {
        SignalCoverageStatus::Present
    }
}

fn family_status(is_empty: bool, dropped: u64) -> SignalCoverageStatus {
    if dropped > 0 {
        SignalCoverageStatus::Truncated
    } else if is_empty {
        SignalCoverageStatus::Missing
    } else {
        SignalCoverageStatus::Present
    }
}

fn runtime_status(run: &Run) -> SignalCoverageStatus {
    if run.truncation.dropped_runtime_snapshots > 0 {
        SignalCoverageStatus::Truncated
    } else if run.runtime_snapshots.is_empty() {
        SignalCoverageStatus::Missing
    } else if run
        .runtime_snapshots
        .iter()
        .all(|snapshot| snapshot.blocking_queue_depth.is_none())
        || run
            .runtime_snapshots
            .iter()
            .all(|snapshot| snapshot.local_queue_depth.is_none())
        || run
            .runtime_snapshots
            .iter()
            .all(|snapshot| snapshot.global_queue_depth.is_none())
    {
        SignalCoverageStatus::Partial
    } else {
        SignalCoverageStatus::Present
    }
}

fn evidence_limitations(
    run: &Run,
    queues: SignalCoverageStatus,
    stages: SignalCoverageStatus,
    runtime_snapshots: SignalCoverageStatus,
) -> Vec<String> {
    let mut limitations = Vec::new();
    if run.requests.len() < LOW_COMPLETED_REQUEST_THRESHOLD {
        limitations
            .push("Low completed-request count can make suspect ranking unstable.".to_string());
    }
    if matches!(
        queues,
        SignalCoverageStatus::Missing | SignalCoverageStatus::Truncated
    ) && matches!(
        stages,
        SignalCoverageStatus::Missing | SignalCoverageStatus::Truncated
    ) {
        limitations.push("Queue and stage instrumentation are both unavailable, limiting application vs downstream interpretation.".to_string());
    }
    if run.runtime_snapshots.is_empty() {
        limitations.push("Runtime snapshots are missing, limiting executor and blocking-pressure interpretation.".to_string());
    } else if runtime_snapshots == SignalCoverageStatus::Partial {
        limitations.push("Runtime snapshots have missing queue-depth fields, limiting executor vs blocking differentiation.".to_string());
    }
    if run.truncation.is_truncated() || run.truncation.limits_hit {
        limitations.push(
            "Capture truncation dropped evidence and can reduce diagnosis completeness."
                .to_string(),
        );
    }
    limitations
}

fn truncation_warnings(run: &Run) -> Vec<String> {
    let mut warnings = Vec::new();
    if run.truncation.limits_hit || run.truncation.is_truncated() {
        warnings.push(
            "Capture limits were hit during this run; dropped evidence can reduce diagnosis completeness and confidence."
                .to_string(),
        );
    }
    if run.truncation.dropped_requests > 0 {
        warnings.push(format!(
            "Capture truncated requests: dropped {} request events after reaching the configured max_requests limit. This dropped evidence can reduce diagnosis completeness and confidence.",
            run.truncation.dropped_requests
        ));
    }
    if run.truncation.dropped_stages > 0 {
        warnings.push(format!(
            "Capture truncated stages: dropped {} stage events after reaching the configured max_stages limit. This dropped evidence can reduce diagnosis completeness and confidence.",
            run.truncation.dropped_stages
        ));
    }
    if run.truncation.dropped_queues > 0 {
        warnings.push(format!(
            "Capture truncated queues: dropped {} queue events after reaching the configured max_queues limit. This dropped evidence can reduce diagnosis completeness and confidence.",
            run.truncation.dropped_queues
        ));
    }
    if run.truncation.dropped_inflight_snapshots > 0 {
        warnings.push(format!(
            "Capture truncated in-flight snapshots: dropped {} entries after reaching max_inflight_snapshots. This dropped evidence can reduce diagnosis completeness and confidence.",
            run.truncation.dropped_inflight_snapshots
        ));
    }
    if run.truncation.dropped_runtime_snapshots > 0 {
        warnings.push(format!(
            "Capture truncated runtime snapshots: dropped {} entries after reaching max_runtime_snapshots. This dropped evidence can reduce diagnosis completeness and confidence.",
            run.truncation.dropped_runtime_snapshots
        ));
    }
    warnings
}

fn clamp_score(value: u64) -> u8 {
    u8::try_from(value.min(100)).unwrap_or(100)
}

fn nonzero_sample_count(values: &[u64]) -> usize {
    values.iter().filter(|&&v| v > 0).count()
}

fn max_or_zero(values: &[u64]) -> u64 {
    values.iter().copied().max().unwrap_or(0)
}

fn score_sample_quality(sample_count: usize) -> u8 {
    if sample_count >= SAMPLE_QUALITY_HIGH_SAMPLE_COUNT {
        8
    } else if sample_count >= SAMPLE_QUALITY_MEDIUM_SAMPLE_COUNT {
        5
    } else if sample_count >= SAMPLE_QUALITY_LOW_SAMPLE_COUNT {
        3
    } else {
        u8::from(sample_count >= SAMPLE_QUALITY_MIN_NONZERO_SAMPLE_COUNT)
    }
}

fn score_from_permille(base: u64, permille: u64, scale: u64) -> u64 {
    base + permille.min(1000) / scale
}

fn cap_unless_clean_evidence(score: u64, clean: bool, soft_cap: u8) -> u8 {
    if clean {
        clamp_score(score)
    } else {
        clamp_score(score.min(u64::from(soft_cap)))
    }
}

fn queue_saturation_suspect(run: &Run, inflight_trend: Option<&InflightTrend>) -> Option<Suspect> {
    let (queue_shares, _) = request_time_shares(run);
    let p95_queue_share_permille = percentile(&queue_shares, 95, 100)?;
    if p95_queue_share_permille < QUEUE_SHARE_TRIGGER_PERMILLE {
        return None;
    }
    let queue_depths = run
        .queues
        .iter()
        .filter_map(|q| q.depth_at_start)
        .collect::<Vec<_>>();
    let max_depth = max_or_zero(&queue_depths);
    let growth_bonus = inflight_trend
        .filter(|t| t.growth_delta > 0)
        .map_or(0, |_| 5);
    let depth_bonus = (max_depth.min(40) * 2) / 3;
    let base = score_from_permille(22, p95_queue_share_permille, 14);
    let clean_extreme = p95_queue_share_permille >= 985
        && max_depth >= 12
        && queue_shares.len() >= 20
        && inflight_trend.is_some_and(|t| t.growth_delta > 0);
    let score = cap_unless_clean_evidence(
        base + depth_bonus + growth_bonus + u64::from(score_sample_quality(queue_shares.len())),
        clean_extreme,
        95,
    );
    let mut evidence = vec![format!(
        "Queue wait at p95 consumes {}.{}% of request time.",
        p95_queue_share_permille / 10,
        p95_queue_share_permille % 10
    )];
    if max_depth > 0 {
        evidence.push(format!("Observed queue depth sample up to {max_depth}."));
    }
    if let Some(trend) = inflight_trend.filter(|trend| trend.growth_delta > 0) {
        evidence.push(format!(
            "In-flight gauge '{}' grew by {} over the run window (p95={}, peak={}).",
            trend.gauge, trend.growth_delta, trend.p95_count, trend.peak_count
        ));
    }
    Some(Suspect::new(
        DiagnosisKind::ApplicationQueueSaturation,
        score,
        evidence,
        vec![
            "Inspect queue admission limits and producer burst patterns.".to_string(),
            "Compare queue wait distribution before and after increasing worker parallelism."
                .to_string(),
        ],
    ))
}

#[derive(Clone, Copy)]
struct BlockingSignal {
    p95: u64,
    peak: u64,
    nonzero: usize,
    samples: usize,
    nz_share_permille: u64,
}

fn blocking_signal(run: &Run) -> Option<BlockingSignal> {
    let depths = runtime_metric_series(&run.runtime_snapshots, |s| s.blocking_queue_depth);
    let p95 = percentile(&depths, 95, 100)?;
    let nonzero = nonzero_sample_count(&depths);
    if p95 == 0 && nonzero < 2 {
        return None;
    }
    let peak = max_or_zero(&depths);
    let nz_share_permille = if depths.is_empty() {
        0
    } else {
        nonzero as u64 * 1000 / depths.len() as u64
    };
    Some(BlockingSignal {
        p95,
        peak,
        nonzero,
        samples: depths.len(),
        nz_share_permille,
    })
}

fn strong_blocking_signal(signal: BlockingSignal) -> bool {
    signal.p95 >= 12 && signal.peak >= 20 && signal.nz_share_permille >= 700 && signal.samples >= 30
}

fn stage_correlates_with_blocking_pool(stage: &str) -> bool {
    let lower = stage.to_ascii_lowercase();
    lower.contains("spawn_blocking")
        || lower.contains("blocking_path")
        || lower.contains("blocking")
}

fn blocking_pressure_suspect(run: &Run) -> Option<Suspect> {
    let signal = blocking_signal(run)?;
    let clean_extreme = signal.p95 >= 16 && signal.peak >= 24 && signal.nz_share_permille >= 900;
    let score = cap_unless_clean_evidence(
        32 + signal.p95.min(24)
            + (signal.peak.min(24) / 2)
            + (signal.nz_share_permille / 80)
            + u64::from(score_sample_quality(signal.samples)),
        clean_extreme,
        94,
    );
    Some(Suspect::new(
        DiagnosisKind::BlockingPoolPressure,
        score,
        vec![format!(
            "Blocking queue depth p95 is {}, peak is {}, with {}/{} nonzero samples.",
            signal.p95, signal.peak, signal.nonzero, signal.samples
        )],
        vec![
            "Audit blocking sections and move avoidable synchronous work out of hot paths."
                .to_string(),
            "Inspect spawn_blocking callsites for long-running CPU or I/O work.".to_string(),
        ],
    ))
}

fn executor_pressure_suspect(run: &Run, inflight_trend: Option<&InflightTrend>) -> Option<Suspect> {
    let global = runtime_metric_series(&run.runtime_snapshots, |s| s.global_queue_depth);
    let p95_global = percentile(&global, 95, 100)?;
    if p95_global == 0 {
        return None;
    }
    let local = runtime_metric_series(&run.runtime_snapshots, |s| s.local_queue_depth);
    let alive = runtime_metric_series(&run.runtime_snapshots, |s| s.alive_tasks);
    let growth_bonus = inflight_trend
        .filter(|t| t.growth_delta > 0)
        .map_or(0, |_| 4);
    let clean_extreme = p95_global >= 140 && global.len() >= 30;
    let score = cap_unless_clean_evidence(
        34 + (p95_global.min(150) / 4)
            + (percentile(&local, 95, 100).unwrap_or(0).min(60) / 6)
            + (percentile(&alive, 95, 100).unwrap_or(0).min(400) / 40)
            + growth_bonus
            + u64::from(score_sample_quality(global.len())),
        clean_extreme,
        94,
    );
    let mut evidence = vec![format!(
        "Runtime global queue depth p95 is {p95_global}, suggesting scheduler contention."
    )];
    if let Some(lp95) = percentile(&local, 95, 100) {
        evidence.push(format!("Runtime local queue depth p95 is {lp95}."));
    }
    if let Some(ap95) = percentile(&alive, 95, 100) {
        evidence.push(format!("Runtime alive_tasks p95 is {ap95}."));
    }
    Some(Suspect::new(
        DiagnosisKind::ExecutorPressureSuspected,
        score,
        evidence,
        vec![
            "Check for long polls without yielding and uneven task fan-out.".to_string(),
            "Compare with per-stage timings to isolate overloaded async stages.".to_string(),
        ],
    ))
}

#[derive(Clone)]
struct StageCandidate {
    stage: String,
    samples: usize,
    p95: u64,
    cumulative: u64,
    cum_share: u64,
    tail_share: u64,
    score: u8,
}

fn downstream_stage_candidates(run: &Run, p95_req: u64, total_req: u64) -> Vec<StageCandidate> {
    let tail_ids: std::collections::HashMap<&str, u64> = run
        .requests
        .iter()
        .filter(|r| r.latency_us >= p95_req)
        .map(|r| (r.request_id.as_str(), r.latency_us))
        .collect();
    let tail_total = tail_ids.values().copied().fold(0_u64, u64::saturating_add);
    let mut by: BTreeMap<&str, Vec<&tailtriage_core::StageEvent>> = BTreeMap::new();
    for st in &run.stages {
        by.entry(st.stage.as_str()).or_default().push(st);
    }
    let mut cands = Vec::new();
    for (name, ss) in by {
        if ss.len() < DOWNSTREAM_MIN_STAGE_SAMPLES {
            continue;
        }
        let lats = ss.iter().map(|s| s.latency_us).collect::<Vec<_>>();
        let cum = lats.iter().copied().fold(0_u64, u64::saturating_add);
        let p95 = percentile(&lats, 95, 100).unwrap_or(0);
        let cum_share = cum.saturating_mul(1000).checked_div(total_req).unwrap_or(0);
        let tail_stage = ss
            .iter()
            .filter_map(|s| tail_ids.get(s.request_id.as_str()).map(|_| s.latency_us))
            .fold(0_u64, u64::saturating_add);
        let tail_share = if tail_total == 0 {
            0
        } else {
            tail_stage
                .saturating_mul(1000)
                .checked_div(tail_total)
                .unwrap_or(0)
        };
        let clean_extreme = tail_share >= 960 && cum_share >= 920 && ss.len() >= 20;
        let score = cap_unless_clean_evidence(
            score_from_permille(24, tail_share, 11)
                + (cum_share / 35)
                + u64::from(score_sample_quality(ss.len())),
            clean_extreme,
            95,
        );
        cands.push(StageCandidate {
            stage: name.to_string(),
            samples: ss.len(),
            p95,
            cumulative: cum,
            cum_share,
            tail_share,
            score,
        });
    }
    cands
}

fn downstream_stage_suspect(run: &Run) -> Option<Suspect> {
    let p95_req = percentile(
        &run.requests
            .iter()
            .map(|r| r.latency_us)
            .collect::<Vec<_>>(),
        95,
        100,
    )?;
    let total_req = run
        .requests
        .iter()
        .map(|r| r.latency_us)
        .fold(0_u64, u64::saturating_add);
    let blocking = blocking_signal(run);
    let blocking_score = blocking.map(|signal| {
        let clean_extreme =
            signal.p95 >= 16 && signal.peak >= 24 && signal.nz_share_permille >= 900;
        cap_unless_clean_evidence(
            32 + signal.p95.min(24)
                + (signal.peak.min(24) / 2)
                + (signal.nz_share_permille / 80)
                + u64::from(score_sample_quality(signal.samples)),
            clean_extreme,
            94,
        )
    });
    let best = downstream_stage_candidates(run, p95_req, total_req)
        .into_iter()
        .max_by(|a, b| {
            a.score
                .cmp(&b.score)
                .then_with(|| a.tail_share.cmp(&b.tail_share))
                .then_with(|| a.cum_share.cmp(&b.cum_share))
                .then_with(|| b.stage.cmp(&a.stage))
        })?;
    let mut downstream_score = best.score;
    let mut correlation_evidence: Option<String> = None;
    if stage_correlates_with_blocking_pool(&best.stage)
        && blocking.is_some_and(strong_blocking_signal)
        && blocking_score.is_some()
    {
        let cap = blocking_score.unwrap_or(downstream_score).saturating_sub(2);
        downstream_score = downstream_score.min(cap);
        correlation_evidence = Some(format!(
            "Stage '{}' looks blocking-correlated; strong runtime blocking-queue evidence keeps blocking_pool_pressure prioritized.",
            best.stage
        ));
    }
    let mut evidence = vec![
        format!(
            "Stage '{}' has p95 latency {} us across {} samples.",
            best.stage, best.p95, best.samples
        ),
        format!(
            "Stage '{}' cumulative latency is {} us ({} permille of request latency).",
            best.stage, best.cumulative, best.cum_share
        ),
        format!(
            "Stage '{}' contributes {} permille of tail request latency.",
            best.stage, best.tail_share
        ),
    ];
    if let Some(extra) = correlation_evidence {
        evidence.push(extra);
    }
    Some(Suspect::new(
        DiagnosisKind::DownstreamStageDominates,
        downstream_score,
        evidence,
        vec![
            format!(
                "Inspect downstream dependency behind stage '{}'.",
                best.stage
            ),
            "Collect downstream service timings and retry behavior during tail windows."
                .to_string(),
            "Review downstream SLO/error budget and align retry budget/backoff with it."
                .to_string(),
        ],
    ))
}

fn ambiguity_warning(suspects: &[Suspect]) -> Option<String> {
    let mut ranked = suspects
        .iter()
        .filter(|s| s.kind != DiagnosisKind::InsufficientEvidence)
        .collect::<Vec<_>>();
    ranked.sort_by_key(|s| std::cmp::Reverse(s.score));
    if ranked.len() >= 2
        && ranked[0].score >= AMBIGUITY_MIN_SCORE_THRESHOLD
        && ranked[1].score >= AMBIGUITY_MIN_SCORE_THRESHOLD
        && ranked[0].score.abs_diff(ranked[1].score) <= AMBIGUITY_SCORE_GAP_THRESHOLD
    {
        Some("Top suspects are close in score; treat ranking as ambiguous and validate both with next checks.".to_string())
    } else {
        None
    }
}

fn analysis_warnings(run: &Run, suspects: &[Suspect]) -> Vec<String> {
    let mut warnings = truncation_warnings(run);
    if run.requests.len() < LOW_COMPLETED_REQUEST_THRESHOLD {
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
            && s.score >= 85
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
    if let Some(w) = ambiguity_warning(suspects) {
        warnings.push(w);
    }
    warnings
}

fn request_time_shares(run: &Run) -> (Vec<u64>, Vec<u64>) {
    let mut total_queue_wait_by_request = HashMap::<&str, u64>::new();
    for queue in &run.queues {
        *total_queue_wait_by_request
            .entry(queue.request_id.as_str())
            .or_default() = total_queue_wait_by_request
            .get(queue.request_id.as_str())
            .copied()
            .unwrap_or_default()
            .saturating_add(queue.wait_us);
    }

    let mut queue_shares = Vec::new();
    let mut service_shares = Vec::new();

    for request in &run.requests {
        if request.latency_us == 0 {
            continue;
        }

        let queue_wait = total_queue_wait_by_request
            .get(request.request_id.as_str())
            .copied()
            .unwrap_or_default()
            .min(request.latency_us);
        let service_time = request.latency_us.saturating_sub(queue_wait);

        queue_shares.push(queue_wait.saturating_mul(1_000) / request.latency_us);
        service_shares.push(service_time.saturating_mul(1_000) / request.latency_us);
    }

    (queue_shares, service_shares)
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

fn fmt_opt_u64(value: Option<u64>) -> String {
    match value {
        Some(value) => value.to_string(),
        None => "n/a".to_string(),
    }
}

fn fmt_percent_permille(value: Option<u64>) -> String {
    match value {
        Some(value) => format!("{}.{:01}%", value / 10, value % 10),
        None => "n/a".to_string(),
    }
}

fn fmt_confidence(confidence: Confidence) -> &'static str {
    match confidence {
        Confidence::Low => "low",
        Confidence::Medium => "medium",
        Confidence::High => "high",
    }
}

#[must_use]
/// Renders a compact text triage summary from a [`Report`].
///
/// The rendered output is guidance for follow-up checks, not proof of root cause.
#[allow(clippy::too_many_lines)]
pub fn render_text(report: &Report) -> String {
    let mut lines = vec![
        "tailtriage diagnosis".to_string(),
        format!("Requests analyzed: {}", report.request_count),
        format!(
            "Latency (us): p50 {}, p95 {}, p99 {}",
            fmt_opt_u64(report.p50_latency_us),
            fmt_opt_u64(report.p95_latency_us),
            fmt_opt_u64(report.p99_latency_us),
        ),
        format!(
            "Request time at p95: queue {}, non-queue service {}",
            fmt_percent_permille(report.p95_queue_share_permille),
            fmt_percent_permille(report.p95_service_share_permille),
        ),
    ];

    match &report.inflight_trend {
        Some(trend) => {
            lines.push(format!(
                "Inflight trend: gauge '{}', samples {}, peak {}, p95 {}, net growth {:+}",
                trend.gauge,
                trend.sample_count,
                trend.peak_count,
                trend.p95_count,
                trend.growth_delta,
            ));
        }
        None => {
            lines.push("Inflight trend: none".to_string());
        }
    }

    lines.push(format!(
        "Primary suspect: {} ({} confidence, score {})",
        report.primary_suspect.kind.as_str(),
        fmt_confidence(report.primary_suspect.confidence),
        report.primary_suspect.score,
    ));
    lines.push(format!(
        "Evidence quality: {}{}",
        match report.evidence_quality.quality {
            EvidenceQualityLevel::Strong => "strong",
            EvidenceQualityLevel::Partial => "partial",
            EvidenceQualityLevel::Weak => "weak",
        },
        report
            .evidence_quality
            .limitations
            .first()
            .map_or_else(String::new, |l| format!(" ({l})"))
    ));

    if !report.warnings.is_empty() {
        lines.push("Warnings:".to_string());
        for warning in &report.warnings {
            lines.push(format!("- {warning}"));
        }
    }

    if !report.primary_suspect.evidence.is_empty() {
        lines.push("Evidence:".to_string());
        for evidence in &report.primary_suspect.evidence {
            lines.push(format!("- {evidence}"));
        }
    }

    if !report.primary_suspect.next_checks.is_empty() {
        lines.push("Next checks:".to_string());
        for next_check in &report.primary_suspect.next_checks {
            lines.push(format!("- {next_check}"));
        }
    }

    if !report.secondary_suspects.is_empty() {
        lines.push("Secondary suspects:".to_string());
        for suspect in &report.secondary_suspects {
            lines.push(format!(
                "- {} ({} confidence, score {})",
                suspect.kind.as_str(),
                fmt_confidence(suspect.confidence),
                suspect.score,
            ));
        }
    }
    if !report.route_breakdowns.is_empty() {
        lines.push("Route breakdowns:".to_string());
        for route in &report.route_breakdowns {
            lines.push(format!(
                "- {}: requests {}, p95 {}us, suspect {} ({} confidence)",
                route.route,
                route.request_count,
                fmt_opt_u64(route.p95_latency_us),
                route.primary_suspect.kind.as_str(),
                fmt_confidence(route.primary_suspect.confidence),
            ));
        }
    }
    if !report.temporal_segments.is_empty() {
        lines.push("Temporal segments:".to_string());
        for segment in &report.temporal_segments {
            lines.push(format!(
                "- {}: requests {}, p95 {}us, suspect {} ({} confidence)",
                segment.name,
                segment.request_count,
                fmt_opt_u64(segment.p95_latency_us),
                segment.primary_suspect.kind.as_str(),
                fmt_confidence(segment.primary_suspect.confidence),
            ));
        }
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use tailtriage_core::{
        CaptureMode, EffectiveCoreConfig, QueueEvent, RequestEvent, Run, RunMetadata,
        RuntimeSnapshot, StageEvent, SCHEMA_VERSION,
    };

    use crate::analyze::{
        analyze_run, analyze_run_internal, apply_evidence_aware_confidence_caps, evidence_quality,
        render_text, Confidence, DiagnosisKind, EvidenceQuality, EvidenceQualityLevel,
        InflightTrend, Report, SignalCoverageStatus, Suspect, ROUTE_DIVERGENCE_WARNING,
        ROUTE_RUNTIME_ATTRIBUTION_WARNING,
    };

    fn test_run() -> Run {
        Run {
            schema_version: SCHEMA_VERSION,
            metadata: RunMetadata {
                run_id: "run-1".to_owned(),
                service_name: "svc".to_owned(),
                service_version: None,
                started_at_unix_ms: 1,
                finished_at_unix_ms: 2,
                finalized_at_unix_ms: Some(2),
                mode: CaptureMode::Light,
                effective_core_config: Some(EffectiveCoreConfig {
                    mode: CaptureMode::Light,
                    capture_limits: CaptureMode::Light.core_defaults(),
                    strict_lifecycle: false,
                }),
                effective_tokio_sampler_config: None,
                host: None,
                pid: Some(1),
                lifecycle_warnings: Vec::new(),
                unfinished_requests: tailtriage_core::UnfinishedRequests::default(),
                run_end_reason: None,
            },
            requests: vec![
                RequestEvent {
                    request_id: "req-1".to_owned(),
                    route: "/test".to_owned(),
                    kind: None,
                    started_at_unix_ms: 1,
                    finished_at_unix_ms: 2,
                    latency_us: 1_000,
                    outcome: "ok".to_owned(),
                },
                RequestEvent {
                    request_id: "req-2".to_owned(),
                    route: "/test".to_owned(),
                    kind: None,
                    started_at_unix_ms: 2,
                    finished_at_unix_ms: 3,
                    latency_us: 1_000,
                    outcome: "ok".to_owned(),
                },
                RequestEvent {
                    request_id: "req-3".to_owned(),
                    route: "/test".to_owned(),
                    kind: None,
                    started_at_unix_ms: 3,
                    finished_at_unix_ms: 4,
                    latency_us: 1_000,
                    outcome: "ok".to_owned(),
                },
            ],
            stages: Vec::new(),
            queues: Vec::new(),
            inflight: Vec::new(),
            runtime_snapshots: Vec::new(),
            truncation: tailtriage_core::TruncationSummary::default(),
        }
    }

    fn sample_request(id: u64) -> RequestEvent {
        RequestEvent {
            request_id: format!("req-{id}"),
            route: "/t".into(),
            kind: None,
            started_at_unix_ms: id,
            finished_at_unix_ms: id + 1,
            latency_us: 1_000,
            outcome: "ok".into(),
        }
    }

    fn runtime_snapshot(
        global: Option<u64>,
        local: Option<u64>,
        blocking: Option<u64>,
    ) -> RuntimeSnapshot {
        RuntimeSnapshot {
            at_unix_ms: 1,
            global_queue_depth: global,
            local_queue_depth: local,
            alive_tasks: Some(20),
            blocking_queue_depth: blocking,
            remote_schedule_count: None,
        }
    }

    #[test]
    fn downstream_stage_tie_break_is_deterministic() {
        let mut run = test_run();
        run.stages = vec![
            StageEvent {
                request_id: "req-1".to_owned(),
                stage: "stage_a".to_owned(),
                started_at_unix_ms: 1,
                finished_at_unix_ms: 2,
                latency_us: 300,
                success: true,
            },
            StageEvent {
                request_id: "req-2".to_owned(),
                stage: "stage_a".to_owned(),
                started_at_unix_ms: 2,
                finished_at_unix_ms: 3,
                latency_us: 300,
                success: true,
            },
            StageEvent {
                request_id: "req-3".to_owned(),
                stage: "stage_a".to_owned(),
                started_at_unix_ms: 3,
                finished_at_unix_ms: 4,
                latency_us: 300,
                success: true,
            },
            StageEvent {
                request_id: "req-1".to_owned(),
                stage: "stage_b".to_owned(),
                started_at_unix_ms: 1,
                finished_at_unix_ms: 2,
                latency_us: 300,
                success: true,
            },
            StageEvent {
                request_id: "req-2".to_owned(),
                stage: "stage_b".to_owned(),
                started_at_unix_ms: 2,
                finished_at_unix_ms: 3,
                latency_us: 300,
                success: true,
            },
            StageEvent {
                request_id: "req-3".to_owned(),
                stage: "stage_b".to_owned(),
                started_at_unix_ms: 3,
                finished_at_unix_ms: 4,
                latency_us: 300,
                success: true,
            },
        ];

        let report = analyze_run(&run);
        assert_eq!(
            report.primary_suspect.kind,
            DiagnosisKind::DownstreamStageDominates
        );
        assert!(
            report.primary_suspect.evidence[0].contains("stage_a"),
            "expected deterministic stage tie-breaker to choose stage_a, got {:?}",
            report.primary_suspect.evidence
        );
    }

    #[test]
    fn inflight_trend_is_none_for_empty_series() {
        assert!(super::dominant_inflight_trend(&[]).is_none());
    }

    #[test]
    fn inflight_trend_handles_constant_series() {
        let trend = super::dominant_inflight_trend(&[
            tailtriage_core::InFlightSnapshot {
                gauge: "http".to_owned(),
                at_unix_ms: 10,
                count: 3,
            },
            tailtriage_core::InFlightSnapshot {
                gauge: "http".to_owned(),
                at_unix_ms: 20,
                count: 3,
            },
        ])
        .expect("trend should exist");

        assert_eq!(trend.peak_count, 3);
        assert_eq!(trend.p95_count, 3);
        assert_eq!(trend.growth_delta, 0);
    }

    #[test]
    fn inflight_trend_handles_monotonic_increase() {
        let trend = super::dominant_inflight_trend(&[
            tailtriage_core::InFlightSnapshot {
                gauge: "http".to_owned(),
                at_unix_ms: 10,
                count: 1,
            },
            tailtriage_core::InFlightSnapshot {
                gauge: "http".to_owned(),
                at_unix_ms: 20,
                count: 4,
            },
            tailtriage_core::InFlightSnapshot {
                gauge: "http".to_owned(),
                at_unix_ms: 30,
                count: 6,
            },
        ])
        .expect("trend should exist");

        assert_eq!(trend.peak_count, 6);
        assert_eq!(trend.p95_count, 6);
        assert_eq!(trend.growth_delta, 5);
        assert_eq!(trend.growth_per_sec_milli, Some(250_000));
    }

    #[test]
    fn render_text_formats_inflight_trend_fields() {
        let report = Report {
            request_count: 2,
            p50_latency_us: Some(10),
            p95_latency_us: Some(20),
            p99_latency_us: Some(20),
            p95_queue_share_permille: Some(100),
            p95_service_share_permille: Some(900),
            inflight_trend: Some(InflightTrend {
                gauge: "queue_inflight".to_owned(),
                sample_count: 4,
                peak_count: 8,
                p95_count: 7,
                growth_delta: 5,
                growth_per_sec_milli: Some(2_500),
            }),
            warnings: Vec::new(),
            evidence_quality: EvidenceQuality {
                request_count: 2,
                queue_event_count: 0,
                stage_event_count: 0,
                runtime_snapshot_count: 0,
                inflight_snapshot_count: 0,
                requests: SignalCoverageStatus::Partial,
                queues: SignalCoverageStatus::Missing,
                stages: SignalCoverageStatus::Missing,
                runtime_snapshots: SignalCoverageStatus::Missing,
                inflight_snapshots: SignalCoverageStatus::Missing,
                truncated: false,
                dropped_requests: 0,
                dropped_stages: 0,
                dropped_queues: 0,
                dropped_inflight_snapshots: 0,
                dropped_runtime_snapshots: 0,
                quality: EvidenceQualityLevel::Weak,
                limitations: vec![],
            },
            primary_suspect: Suspect {
                kind: DiagnosisKind::ApplicationQueueSaturation,
                score: 90,
                confidence: Confidence::High,
                evidence: vec!["queue wait high".to_owned()],
                next_checks: vec!["check queue policy".to_owned()],
                confidence_notes: Vec::new(),
            },
            secondary_suspects: Vec::new(),
            route_breakdowns: Vec::new(),
            temporal_segments: Vec::new(),
        };

        let text = render_text(&report);
        assert!(text.contains("Inflight trend: gauge 'queue_inflight'"));
        assert!(text.contains("samples 4"));
        assert!(text.contains("peak 8"));
        assert!(text.contains("p95 7"));
        assert!(text.contains("net growth +5"));
        assert!(text.contains("Request time at p95: queue 10.0%, non-queue service 90.0%"));
    }

    #[test]
    fn render_text_marks_missing_inflight_trend() {
        let report = Report {
            request_count: 0,
            p50_latency_us: None,
            p95_latency_us: None,
            p99_latency_us: None,
            p95_queue_share_permille: None,
            p95_service_share_permille: None,
            inflight_trend: None,
            warnings: vec!["Capture truncated requests.".to_owned()],
            evidence_quality: EvidenceQuality {
                request_count: 0,
                queue_event_count: 0,
                stage_event_count: 0,
                runtime_snapshot_count: 0,
                inflight_snapshot_count: 0,
                requests: SignalCoverageStatus::Missing,
                queues: SignalCoverageStatus::Missing,
                stages: SignalCoverageStatus::Missing,
                runtime_snapshots: SignalCoverageStatus::Missing,
                inflight_snapshots: SignalCoverageStatus::Missing,
                truncated: true,
                dropped_requests: 1,
                dropped_stages: 0,
                dropped_queues: 0,
                dropped_inflight_snapshots: 0,
                dropped_runtime_snapshots: 0,
                quality: EvidenceQualityLevel::Weak,
                limitations: vec!["capture limited".to_owned()],
            },
            primary_suspect: Suspect {
                kind: DiagnosisKind::InsufficientEvidence,
                score: 50,
                confidence: Confidence::Low,
                evidence: vec!["missing signals".to_owned()],
                next_checks: vec!["add instrumentation".to_owned()],
                confidence_notes: Vec::new(),
            },
            secondary_suspects: Vec::new(),
            route_breakdowns: Vec::new(),
            temporal_segments: Vec::new(),
        };

        let text = render_text(&report);
        assert!(text.contains("Inflight trend: none"));
        assert!(text.contains("Warnings:"));
        assert!(text.contains("- Capture truncated requests."));
    }

    #[test]
    fn analyze_run_emits_truncation_warnings() {
        let mut run = test_run();
        run.truncation.dropped_requests = 2;
        run.truncation.dropped_runtime_snapshots = 1;
        run.truncation.limits_hit = true;

        let report = analyze_run(&run);
        assert!(report.warnings.len() >= 3);
        assert!(report.warnings.iter().any(|warning| {
            warning.contains("dropped evidence can reduce diagnosis completeness and confidence")
        }));
        assert!(report
            .warnings
            .iter()
            .any(|warning| warning.contains("dropped 2 request events")));
        assert!(report
            .warnings
            .iter()
            .any(|warning| warning.contains("dropped 1 entries")));
    }

    #[test]
    fn low_request_count_warning_appears() {
        let report = analyze_run(&test_run());
        assert!(report
            .warnings
            .iter()
            .any(|w| w.contains("Low completed-request count")));
    }

    #[test]
    fn no_runtime_warning_not_emitted_for_clean_queue_primary() {
        let mut run = test_run();
        run.queues = vec![
            QueueEvent {
                request_id: "req-1".into(),
                queue: "q".into(),
                wait_us: 900,
                waited_from_unix_ms: 0,
                waited_until_unix_ms: 1,
                depth_at_start: Some(9),
            },
            QueueEvent {
                request_id: "req-2".into(),
                queue: "q".into(),
                wait_us: 900,
                waited_from_unix_ms: 1,
                waited_until_unix_ms: 2,
                depth_at_start: Some(9),
            },
            QueueEvent {
                request_id: "req-3".into(),
                queue: "q".into(),
                wait_us: 900,
                waited_from_unix_ms: 2,
                waited_until_unix_ms: 3,
                depth_at_start: Some(9),
            },
        ];
        let report = analyze_run(&run);
        assert_eq!(
            report.primary_suspect.kind,
            DiagnosisKind::ApplicationQueueSaturation
        );
        assert!(!report
            .warnings
            .iter()
            .any(|w| w.contains("No runtime snapshots captured")));
    }

    #[test]
    fn runtime_warning_emitted_when_insufficient_evidence() {
        let report = analyze_run(&test_run());
        assert!(report
            .warnings
            .iter()
            .any(|w| w.contains("No runtime snapshots captured")));
    }

    #[test]
    fn downstream_beats_weak_blocking() {
        let mut run = test_run();
        run.stages = vec![
            StageEvent {
                request_id: "req-1".into(),
                stage: "db".into(),
                started_at_unix_ms: 1,
                finished_at_unix_ms: 2,
                latency_us: 900,
                success: true,
            },
            StageEvent {
                request_id: "req-2".into(),
                stage: "db".into(),
                started_at_unix_ms: 2,
                finished_at_unix_ms: 3,
                latency_us: 900,
                success: true,
            },
            StageEvent {
                request_id: "req-3".into(),
                stage: "db".into(),
                started_at_unix_ms: 3,
                finished_at_unix_ms: 4,
                latency_us: 900,
                success: true,
            },
        ];
        run.runtime_snapshots = vec![runtime_snapshot(Some(2), Some(1), Some(1)); 5];
        let report = analyze_run(&run);
        assert_eq!(
            report.primary_suspect.kind,
            DiagnosisKind::DownstreamStageDominates
        );
    }

    #[test]
    fn score_100_is_reserved_for_overwhelming_queue_evidence() {
        let mut run = test_run();
        run.requests = (0..40)
            .map(|i| RequestEvent {
                request_id: format!("req-{i}"),
                route: "/test".into(),
                kind: None,
                started_at_unix_ms: i,
                finished_at_unix_ms: i + 1,
                latency_us: 1_000,
                outcome: "ok".into(),
            })
            .collect();
        run.queues = run
            .requests
            .iter()
            .map(|r| QueueEvent {
                request_id: r.request_id.clone(),
                queue: "q".into(),
                waited_from_unix_ms: 1,
                waited_until_unix_ms: 2,
                wait_us: 990,
                depth_at_start: Some(20),
            })
            .collect();
        let report = analyze_run(&run);
        assert_eq!(
            report.primary_suspect.kind,
            DiagnosisKind::ApplicationQueueSaturation
        );
        assert!(report.primary_suspect.score >= 95);
    }

    #[test]
    fn ambiguity_warning_requires_close_calibrated_scores() {
        let suspects = vec![
            Suspect::new(
                DiagnosisKind::DownstreamStageDominates,
                82,
                vec!["e".into()],
                vec![],
            ),
            Suspect::new(
                DiagnosisKind::BlockingPoolPressure,
                79,
                vec!["e".into()],
                vec![],
            ),
        ];
        assert!(super::ambiguity_warning(&suspects).is_some());
    }

    #[test]
    fn blocking_like_stage_does_not_outrank_strong_blocking_runtime_signal() {
        let mut run = test_run();
        run.requests = (0..40)
            .map(|i| RequestEvent {
                request_id: format!("req-{i}"),
                route: "/test".into(),
                kind: None,
                started_at_unix_ms: i,
                finished_at_unix_ms: i + 1,
                latency_us: 4_000_000,
                outcome: "ok".into(),
            })
            .collect();
        run.stages = run
            .requests
            .iter()
            .map(|r| StageEvent {
                request_id: r.request_id.clone(),
                stage: "spawn_blocking_path".into(),
                started_at_unix_ms: 1,
                finished_at_unix_ms: 2,
                latency_us: 3_900_000,
                success: true,
            })
            .collect();
        run.runtime_snapshots = vec![runtime_snapshot(Some(1), Some(1), Some(240)); 80];
        let report = analyze_run(&run);
        assert_eq!(
            report.primary_suspect.kind,
            DiagnosisKind::BlockingPoolPressure
        );
        assert!(report
            .secondary_suspects
            .iter()
            .any(|s| s.kind == DiagnosisKind::DownstreamStageDominates));
    }

    #[test]
    fn retry_or_db_stage_is_not_treated_as_blocking_correlated_stage() {
        assert!(!super::stage_correlates_with_blocking_pool("db_query"));
        assert!(!super::stage_correlates_with_blocking_pool("retry_attempt"));
        assert!(super::stage_correlates_with_blocking_pool(
            "spawn_blocking_path"
        ));
    }

    #[test]
    fn truncation_warnings_remain_additive() {
        let mut run = test_run();
        run.truncation.dropped_requests = 1;
        run.truncation.dropped_stages = 1;
        run.truncation.dropped_runtime_snapshots = 1;
        let report = analyze_run(&run);
        assert!(report
            .warnings
            .iter()
            .any(|w| w.contains("dropped 1 request events")));
        assert!(report
            .warnings
            .iter()
            .any(|w| w.contains("dropped 1 stage events")));
        assert!(report
            .warnings
            .iter()
            .any(|w| w.contains("dropped 1 entries after reaching max_runtime_snapshots")));
    }

    #[test]
    fn evidence_quality_weak_for_low_requests() {
        let report = analyze_run(&test_run());
        assert_eq!(report.evidence_quality.quality, EvidenceQualityLevel::Weak);
        assert_eq!(
            report.evidence_quality.requests,
            SignalCoverageStatus::Partial
        );
    }

    #[test]
    fn evidence_quality_requests_missing_when_zero_requests_even_if_dropped() {
        let mut run = test_run();
        run.requests.clear();
        run.truncation.dropped_requests = 3;
        let report = analyze_run(&run);
        assert_eq!(
            report.evidence_quality.requests,
            SignalCoverageStatus::Missing
        );
    }

    #[test]
    fn evidence_quality_partial_for_runtime_partial_fields() {
        let mut run = test_run();
        run.requests = (0..25)
            .map(|i| RequestEvent {
                request_id: format!("req-{i}"),
                route: "/t".into(),
                kind: None,
                started_at_unix_ms: i,
                finished_at_unix_ms: i + 1,
                latency_us: 1_000,
                outcome: "ok".into(),
            })
            .collect();
        run.queues = run
            .requests
            .iter()
            .map(|r| QueueEvent {
                request_id: r.request_id.clone(),
                queue: "q".into(),
                wait_us: 600,
                waited_from_unix_ms: 1,
                waited_until_unix_ms: 2,
                depth_at_start: Some(2),
            })
            .collect();
        run.runtime_snapshots = vec![runtime_snapshot(Some(1), None, Some(1)); 10];
        let report = analyze_run(&run);
        assert_eq!(
            report.evidence_quality.runtime_snapshots,
            SignalCoverageStatus::Partial
        );
        assert_eq!(
            report.evidence_quality.quality,
            EvidenceQualityLevel::Partial
        );
    }

    #[test]
    fn evidence_quality_strong_without_runtime_snapshots_when_queue_stage_present() {
        let mut run = test_run();
        run.requests = (0..30)
            .map(|i| RequestEvent {
                request_id: format!("req-{i}"),
                route: "/t".into(),
                kind: None,
                started_at_unix_ms: i,
                finished_at_unix_ms: i + 1,
                latency_us: 1_000,
                outcome: "ok".into(),
            })
            .collect();
        run.queues = run
            .requests
            .iter()
            .map(|r| QueueEvent {
                request_id: r.request_id.clone(),
                queue: "q".into(),
                wait_us: 500,
                waited_from_unix_ms: 1,
                waited_until_unix_ms: 2,
                depth_at_start: Some(2),
            })
            .collect();
        run.stages = run
            .requests
            .iter()
            .map(|r| StageEvent {
                request_id: r.request_id.clone(),
                stage: "db".into(),
                started_at_unix_ms: 1,
                finished_at_unix_ms: 2,
                latency_us: 400,
                success: true,
            })
            .collect();
        let report = analyze_run(&run);
        assert_eq!(
            report.evidence_quality.quality,
            EvidenceQualityLevel::Strong
        );
    }

    #[test]
    fn evidence_quality_marks_queue_signal_truncated_and_not_strong() {
        let mut run = test_run();
        run.requests = (0..30)
            .map(|i| RequestEvent {
                request_id: format!("req-{i}"),
                route: "/t".into(),
                kind: None,
                started_at_unix_ms: i,
                finished_at_unix_ms: i + 1,
                latency_us: 1_000,
                outcome: "ok".into(),
            })
            .collect();
        run.queues = run
            .requests
            .iter()
            .map(|r| QueueEvent {
                request_id: r.request_id.clone(),
                queue: "q".into(),
                wait_us: 500,
                waited_from_unix_ms: 1,
                waited_until_unix_ms: 2,
                depth_at_start: Some(2),
            })
            .collect();
        run.stages = run
            .requests
            .iter()
            .map(|r| StageEvent {
                request_id: r.request_id.clone(),
                stage: "db".into(),
                started_at_unix_ms: 1,
                finished_at_unix_ms: 2,
                latency_us: 400,
                success: true,
            })
            .collect();
        run.truncation.dropped_queues = 2;

        let report = analyze_run(&run);
        assert_eq!(
            report.evidence_quality.queues,
            SignalCoverageStatus::Truncated
        );
        assert_ne!(
            report.evidence_quality.quality,
            EvidenceQualityLevel::Strong
        );
    }

    #[test]
    fn confidence_caps_do_not_change_score_ordering() {
        let mut run = test_run();
        run.requests = (0..40)
            .map(|i| RequestEvent {
                request_id: format!("req-{i}"),
                route: "/t".into(),
                kind: None,
                started_at_unix_ms: i,
                finished_at_unix_ms: i + 1,
                latency_us: 1_000,
                outcome: "ok".into(),
            })
            .collect();
        run.queues = run
            .requests
            .iter()
            .map(|r| QueueEvent {
                request_id: r.request_id.clone(),
                queue: "q".into(),
                wait_us: 900,
                waited_from_unix_ms: 1,
                waited_until_unix_ms: 2,
                depth_at_start: Some(8),
            })
            .collect();
        run.stages = run
            .requests
            .iter()
            .map(|r| StageEvent {
                request_id: r.request_id.clone(),
                stage: "db".into(),
                started_at_unix_ms: 1,
                finished_at_unix_ms: 2,
                latency_us: 800,
                success: true,
            })
            .collect();
        run.truncation.dropped_requests = 1;
        let report = analyze_run(&run);
        let mut scores = vec![report.primary_suspect.score];
        scores.extend(report.secondary_suspects.iter().map(|s| s.score));
        assert!(scores.windows(2).all(|w| w[0] >= w[1]));
    }

    #[test]
    fn low_request_count_caps_primary_confidence_and_adds_note() {
        let mut run = test_run();
        run.requests = (0..15)
            .map(|i| RequestEvent {
                request_id: format!("req-{i}"),
                route: "/t".into(),
                kind: None,
                started_at_unix_ms: i,
                finished_at_unix_ms: i + 1,
                latency_us: 1_000,
                outcome: "ok".into(),
            })
            .collect();
        run.queues = run
            .requests
            .iter()
            .map(|r| QueueEvent {
                request_id: r.request_id.clone(),
                queue: "q".into(),
                waited_from_unix_ms: 1,
                waited_until_unix_ms: 2,
                wait_us: 990,
                depth_at_start: Some(18),
            })
            .collect();
        let report = analyze_run(&run);
        assert_eq!(report.primary_suspect.confidence, Confidence::Medium);
        assert!(report
            .primary_suspect
            .confidence_notes
            .iter()
            .any(|n| n == "Low completed-request count caps confidence."));
    }

    #[test]
    fn clean_strong_queue_evidence_keeps_high_confidence_without_notes() {
        let mut run = test_run();
        run.requests = (0..45)
            .map(|i| RequestEvent {
                request_id: format!("req-{i}"),
                route: "/test".into(),
                kind: None,
                started_at_unix_ms: i,
                finished_at_unix_ms: i + 1,
                latency_us: 1_000,
                outcome: "ok".into(),
            })
            .collect();
        run.queues = run
            .requests
            .iter()
            .map(|r| QueueEvent {
                request_id: r.request_id.clone(),
                queue: "q".into(),
                waited_from_unix_ms: 1,
                waited_until_unix_ms: 2,
                wait_us: 985,
                depth_at_start: Some(15),
            })
            .collect();
        run.inflight = vec![
            tailtriage_core::InFlightSnapshot {
                gauge: "http".into(),
                at_unix_ms: 1,
                count: 1,
            },
            tailtriage_core::InFlightSnapshot {
                gauge: "http".into(),
                at_unix_ms: 2,
                count: 10,
            },
        ];
        let report = analyze_run(&run);
        assert_eq!(
            report.primary_suspect.kind,
            DiagnosisKind::ApplicationQueueSaturation
        );
        assert_eq!(report.primary_suspect.confidence, Confidence::High);
        assert!(report.primary_suspect.confidence_notes.is_empty());
    }

    #[test]
    fn queue_truncation_uses_truncation_note_not_missing_queue_note() {
        let mut run = test_run();
        run.requests = (0..45)
            .map(|i| RequestEvent {
                request_id: format!("req-{i}"),
                route: "/q".into(),
                kind: None,
                started_at_unix_ms: i,
                finished_at_unix_ms: i + 1,
                latency_us: 1_000,
                outcome: "ok".into(),
            })
            .collect();
        run.queues = run
            .requests
            .iter()
            .map(|r| QueueEvent {
                request_id: r.request_id.clone(),
                queue: "q".into(),
                waited_from_unix_ms: 1,
                waited_until_unix_ms: 2,
                wait_us: 990,
                depth_at_start: Some(15),
            })
            .collect();
        run.truncation.dropped_queues = 1;
        let report = analyze_run(&run);
        assert!(report.primary_suspect.confidence_notes.iter().any(|n| n
            == "Capture truncation caps confidence because dropped evidence may affect ranking."));
        assert!(!report
            .primary_suspect
            .confidence_notes
            .iter()
            .any(|n| n == "Missing queue instrumentation limits queue-saturation confidence."));
    }

    #[test]
    fn missing_queue_instrumentation_uses_missing_queue_note() {
        let mut run = test_run();
        run.requests = vec![sample_request(1)];
        run.queues.clear();
        let eq = evidence_quality(&run);
        let mut suspects = vec![Suspect::new(
            DiagnosisKind::ApplicationQueueSaturation,
            100,
            vec![],
            vec![],
        )];
        apply_evidence_aware_confidence_caps(&mut suspects, &run, &eq);
        assert!(suspects[0]
            .confidence_notes
            .iter()
            .any(|n| n == "Missing queue instrumentation limits queue-saturation confidence."));
    }

    #[test]
    fn stage_truncation_uses_truncation_note_not_missing_stage_note() {
        let mut run = test_run();
        run.requests = (0..45)
            .map(|i| RequestEvent {
                request_id: format!("req-{i}"),
                route: "/s".into(),
                kind: None,
                started_at_unix_ms: i,
                finished_at_unix_ms: i + 1,
                latency_us: 5_000,
                outcome: "ok".into(),
            })
            .collect();
        run.stages = run
            .requests
            .iter()
            .map(|r| StageEvent {
                request_id: r.request_id.clone(),
                stage: "db".into(),
                started_at_unix_ms: 1,
                finished_at_unix_ms: 10,
                latency_us: 4_800,
                success: true,
            })
            .collect();
        run.truncation.dropped_stages = 1;
        let report = analyze_run(&run);
        assert!(report.primary_suspect.confidence_notes.iter().any(|n| n
            == "Capture truncation caps confidence because dropped evidence may affect ranking."));
        assert!(!report
            .primary_suspect
            .confidence_notes
            .iter()
            .any(|n| n == "Missing stage instrumentation limits downstream-stage confidence."));
    }

    #[test]
    fn missing_stage_instrumentation_uses_missing_stage_note() {
        let mut run = test_run();
        run.requests = vec![sample_request(1)];
        run.stages.clear();
        let eq = evidence_quality(&run);
        let mut suspects = vec![Suspect::new(
            DiagnosisKind::DownstreamStageDominates,
            100,
            vec![],
            vec![],
        )];
        apply_evidence_aware_confidence_caps(&mut suspects, &run, &eq);
        assert!(suspects[0]
            .confidence_notes
            .iter()
            .any(|n| n == "Missing stage instrumentation limits downstream-stage confidence."));
    }

    #[test]
    fn runtime_partial_fields_cap_executor_or_blocking_confidence() {
        let mut run = test_run();
        run.requests = vec![sample_request(1)];
        run.runtime_snapshots = (0..10)
            .map(|i| RuntimeSnapshot {
                at_unix_ms: i,
                alive_tasks: Some(1),
                global_queue_depth: Some(5),
                local_queue_depth: Some(2),
                blocking_queue_depth: None,
                remote_schedule_count: Some(0),
            })
            .collect();
        let eq = evidence_quality(&run);
        let mut suspects = vec![Suspect::new(
            DiagnosisKind::BlockingPoolPressure,
            100,
            vec![],
            vec![],
        )];
        apply_evidence_aware_confidence_caps(&mut suspects, &run, &eq);
        assert_eq!(suspects[0].confidence, Confidence::Medium);
        assert!(suspects[0]
            .confidence_notes
            .iter()
            .any(|n| n == "Missing runtime snapshots limit executor/blocking confidence."));
    }

    #[test]
    fn ambiguity_cap_adds_note_to_primary() {
        let mut suspects = vec![
            Suspect::new(
                DiagnosisKind::ApplicationQueueSaturation,
                100,
                vec![],
                vec![],
            ),
            Suspect::new(DiagnosisKind::DownstreamStageDominates, 97, vec![], vec![]),
        ];
        let run = test_run();
        let eq = evidence_quality(&run);
        apply_evidence_aware_confidence_caps(&mut suspects, &run, &eq);
        assert_eq!(suspects[0].confidence, Confidence::Medium);
        assert!(suspects[0]
            .confidence_notes
            .iter()
            .any(|n| n == "Top suspects are close in score; confidence is capped by ambiguity."));
    }

    #[test]
    fn ambiguity_tied_top_scores_only_caps_first_sorted_suspect() {
        let mut suspects = vec![
            Suspect::new(
                DiagnosisKind::ApplicationQueueSaturation,
                100,
                vec![],
                vec![],
            ),
            Suspect::new(DiagnosisKind::DownstreamStageDominates, 100, vec![], vec![]),
        ];
        let run = test_run();
        let eq = evidence_quality(&run);
        apply_evidence_aware_confidence_caps(&mut suspects, &run, &eq);

        assert_eq!(suspects[0].score, 100);
        assert_eq!(suspects[1].score, 100);
        assert_eq!(suspects[0].kind, DiagnosisKind::ApplicationQueueSaturation);
        assert_eq!(suspects[1].kind, DiagnosisKind::DownstreamStageDominates);
        assert!(suspects[0]
            .confidence_notes
            .iter()
            .any(|n| n == "Top suspects are close in score; confidence is capped by ambiguity."));
        assert!(!suspects[1]
            .confidence_notes
            .iter()
            .any(|n| n == "Top suspects are close in score; confidence is capped by ambiguity."));
    }

    #[test]
    fn route_breakdowns_empty_for_single_route() {
        let report = analyze_run(&test_run());
        assert!(report.route_breakdowns.is_empty());
        assert!(report
            .warnings
            .iter()
            .all(|warning| warning != ROUTE_DIVERGENCE_WARNING));
    }

    #[test]
    fn single_route_executor_signals_do_not_emit_route_breakdowns_or_divergence_warning() {
        let mut run = test_run();
        run.runtime_snapshots = vec![runtime_snapshot(Some(150), Some(120), Some(2))];
        let report = analyze_run(&run);
        assert!(report.route_breakdowns.is_empty());
        assert!(report
            .warnings
            .iter()
            .all(|warning| warning != ROUTE_DIVERGENCE_WARNING));
    }

    #[test]
    fn multi_route_divergence_emits_sorted_breakdowns_and_stable_warning() {
        let mut run = test_run();
        run.requests.clear();
        for idx in 1..=4 {
            let mut req = sample_request(idx);
            req.route = "/a".into();
            req.latency_us = 10_000;
            run.requests.push(req);
        }
        for idx in 5..=7 {
            let mut req = sample_request(idx);
            req.route = "/b".into();
            req.latency_us = 2_000;
            run.requests.push(req);
        }
        // Below threshold route must be omitted.
        for idx in 8..=9 {
            let mut req = sample_request(idx);
            req.route = "/c".into();
            req.latency_us = 50_000;
            run.requests.push(req);
        }
        for req_id in ["req-1", "req-2", "req-3", "req-4"] {
            run.queues.push(QueueEvent {
                request_id: req_id.to_owned(),
                queue: "ingress".into(),
                wait_us: 9_000,
                waited_from_unix_ms: 0,
                waited_until_unix_ms: 1,
                depth_at_start: Some(9),
            });
        }
        for req_id in ["req-5", "req-6", "req-7"] {
            run.stages.push(StageEvent {
                request_id: req_id.to_owned(),
                stage: "db".into(),
                started_at_unix_ms: 1,
                finished_at_unix_ms: 2,
                latency_us: 1_900,
                success: true,
            });
        }
        run.runtime_snapshots = vec![runtime_snapshot(Some(200), Some(140), Some(180))];
        let report = analyze_run(&run);
        assert_eq!(report.route_breakdowns.len(), 2);
        assert_eq!(report.route_breakdowns[0].route, "/a");
        assert_eq!(report.route_breakdowns[1].route, "/b");
        assert!(report
            .warnings
            .iter()
            .any(|warning| warning == ROUTE_DIVERGENCE_WARNING));
        assert!(report.route_breakdowns.iter().all(|rb| rb
            .warnings
            .iter()
            .any(|warning| warning == ROUTE_RUNTIME_ATTRIBUTION_WARNING)));
        assert!(report.route_breakdowns.iter().all(|rb| rb
            .warnings
            .iter()
            .any(|warning| warning.contains("fewer than 3 completed requests"))));
        assert!(report.route_breakdowns.iter().all(|rb| rb
            .secondary_suspects
            .iter()
            .all(|s| s.kind != DiagnosisKind::ExecutorPressureSuspected
                && s.kind != DiagnosisKind::BlockingPoolPressure)));
        let value = serde_json::to_value(&report).expect("serialize report");
        for breakdown in value
            .get("route_breakdowns")
            .and_then(serde_json::Value::as_array)
            .expect("route_breakdowns array")
        {
            assert!(breakdown.get("route_breakdowns").is_none());
        }
    }

    #[test]
    fn route_breakdowns_do_not_change_global_primary_suspect() {
        let mut run = test_run();
        run.runtime_snapshots = vec![runtime_snapshot(Some(300), Some(250), Some(200))];
        let global = analyze_run_internal(&run);
        let report = analyze_run(&run);
        assert_eq!(report.primary_suspect.kind, global.primary_suspect.kind);
        assert_eq!(report.primary_suspect.score, global.primary_suspect.score);
    }

    #[test]
    fn temporal_segments_empty_below_threshold_and_present_in_json() {
        let report = analyze_run(&test_run());
        assert!(report.temporal_segments.is_empty());
        let json = serde_json::to_value(&report).expect("json");
        assert!(json.get("temporal_segments").is_some());
    }

    #[test]
    fn temporal_segments_emit_on_large_p95_shift() {
        let mut run = test_run();
        run.requests = (0..20)
            .map(|i| RequestEvent {
                request_id: format!("req-{i}"),
                route: "/t".into(),
                kind: None,
                started_at_unix_ms: i,
                finished_at_unix_ms: i + 1,
                latency_us: if i < 10 { 100 } else { 20_000 },
                outcome: "ok".into(),
            })
            .collect();
        let report = analyze_run(&run);
        assert_eq!(report.temporal_segments.len(), 2);
        assert!(report
            .warnings
            .iter()
            .any(|w| w.contains("large p95 latency shift")));
    }

    #[test]
    fn temporal_segments_use_max_finish_timestamp() {
        let mut run = test_run();
        run.requests = (0..20).map(sample_request).collect();
        run.requests[9].finished_at_unix_ms = 200;
        let report = analyze_run(&run);
        if report.temporal_segments.len() == 2 {
            assert_eq!(report.temporal_segments[0].finished_at_unix_ms, Some(200));
        }
    }
}
