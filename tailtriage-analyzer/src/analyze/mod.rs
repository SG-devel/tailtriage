use std::collections::{BTreeMap, HashMap};

use serde::{Serialize, Serializer};

mod confidence;
mod evidence;
mod route;
mod scoring;
mod temporal;

pub use evidence::{EvidenceQuality, EvidenceQualityLevel, SignalCoverageStatus};
use tailtriage_core::{InFlightSnapshot, Run, RuntimeSnapshot};

const LOW_COMPLETED_REQUEST_THRESHOLD: usize = 20;
const QUEUE_SHARE_TRIGGER_PERMILLE: u64 = 300;
const MEDIUM_CONFIDENCE_SCORE_THRESHOLD: u8 = 65;
const HIGH_CONFIDENCE_SCORE_THRESHOLD: u8 = 85;
const AMBIGUITY_MIN_SCORE_THRESHOLD: u8 = 60;
const AMBIGUITY_SCORE_GAP_THRESHOLD: u8 = 4;
const ROUTE_MIN_REQUEST_COUNT: usize = 3;
const ROUTE_BREAKDOWN_LIMIT: usize = 10;
const TEMPORAL_MIN_REQUEST_COUNT: usize = 20;
const TEMPORAL_MIN_SEGMENT_REQUEST_COUNT: usize = 8;
const TEMPORAL_SHARE_SHIFT_PERMILLE: u64 = 200;
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
    pub(super) fn new(
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
    /// Supporting early/late temporal triage summaries when within-run shifts add value.
    pub temporal_segments: Vec<TemporalSegment>,
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
/// // `analyze_run(&Run)` can operate on an in-memory run with zero requests.
/// let report = analyze_run(&run, AnalyzeOptions::default());
/// assert_eq!(report.request_count, 0);
/// ```
#[must_use]
pub fn analyze_run(run: &Run, options: AnalyzeOptions) -> Report {
    Analyzer::new(options).analyze_run(run)
}

/// Analyzer with configurable analysis options.
#[derive(Debug, Clone)]
pub struct Analyzer {
    options: AnalyzeOptions,
}

impl Analyzer {
    /// Creates a new analyzer with the provided options.
    #[must_use]
    pub const fn new(options: AnalyzeOptions) -> Self {
        Self { options }
    }

    /// Analyzes one run artifact with rule-based heuristics and returns a triage report.
    #[must_use]
    pub fn analyze_run(&self, run: &Run) -> Report {
        let _ = &self.options;

        let mut report = analyze_run_internal(run);
        let route_context = route::route_breakdowns(run, &report);
        if route_context.divergent {
            report.warnings.push(ROUTE_DIVERGENCE_WARNING.to_string());
        }
        report.route_breakdowns = route_context.breakdowns;
        report.temporal_segments = temporal::temporal_segments(run, &mut report.warnings);
        report
    }
}

/// Analysis options for run diagnosis.
#[non_exhaustive]
#[derive(Debug, Clone, Default)]
pub struct AnalyzeOptions {}

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

    if let Some(queue_suspect) = scoring::queue_saturation_suspect(run, inflight_trend.as_ref()) {
        suspects.push(queue_suspect);
    }

    if let Some(blocking_suspect) = scoring::blocking_pressure_suspect(run) {
        suspects.push(blocking_suspect);
    }

    if let Some(executor_suspect) = scoring::executor_pressure_suspect(run, inflight_trend.as_ref())
    {
        suspects.push(executor_suspect);
    }

    if let Some(stage_suspect) = scoring::downstream_stage_suspect(run) {
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
    let evidence_quality = evidence::evidence_quality(run);

    confidence::apply_evidence_aware_confidence_caps(&mut suspects, run, &evidence_quality);

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
    let mut warnings = evidence::truncation_warnings(run);
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

pub use render::render_text;

mod render;

#[cfg(test)]
mod tests;
