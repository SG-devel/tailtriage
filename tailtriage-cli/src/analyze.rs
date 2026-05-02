use std::collections::{BTreeMap, HashMap};

use serde::{Serialize, Serializer};
use tailtriage_core::{InFlightSnapshot, Run, RuntimeSnapshot};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
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
        if score >= 85 {
            Self::High
        } else if score >= 65 {
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
    /// Highest-ranked suspect from this run.
    pub primary_suspect: Suspect,
    /// Lower-ranked suspects retained for follow-up triage.
    pub secondary_suspects: Vec<Suspect>,
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
        primary_suspect,
        secondary_suspects: ranked.collect(),
    }
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
    if sample_count >= 100 {
        8
    } else if sample_count >= 40 {
        5
    } else if sample_count >= 20 {
        3
    } else {
        u8::from(sample_count >= 8)
    }
}

fn queue_saturation_suspect(run: &Run, inflight_trend: Option<&InflightTrend>) -> Option<Suspect> {
    let (queue_shares, _) = request_time_shares(run);
    let p95_queue_share_permille = percentile(&queue_shares, 95, 100)?;
    if p95_queue_share_permille < 300 {
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
    let score = clamp_score(
        40 + (p95_queue_share_permille / 12)
            + (max_depth.min(40))
            + growth_bonus
            + u64::from(score_sample_quality(queue_shares.len())),
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

fn blocking_pressure_suspect(run: &Run) -> Option<Suspect> {
    let blocking_depths = runtime_metric_series(&run.runtime_snapshots, |s| s.blocking_queue_depth);
    let p95_blocking_depth = percentile(&blocking_depths, 95, 100)?;
    let nonzero = nonzero_sample_count(&blocking_depths);
    if p95_blocking_depth == 0 && nonzero < 2 {
        return None;
    }
    let peak = max_or_zero(&blocking_depths);
    let nz_share_permille: u64 = if blocking_depths.is_empty() {
        0
    } else {
        nonzero as u64 * 1000 / blocking_depths.len() as u64
    };
    let score = clamp_score(
        28 + (p95_blocking_depth.min(20) * 2)
            + peak.min(20)
            + (nz_share_permille / 100)
            + u64::from(score_sample_quality(blocking_depths.len())),
    );
    Some(Suspect::new(DiagnosisKind::BlockingPoolPressure, score, vec![format!("Blocking queue depth p95 is {p95_blocking_depth}, peak is {peak}, with {nonzero}/{} nonzero samples.", blocking_depths.len())], vec!["Audit blocking sections and move avoidable synchronous work out of hot paths.".to_string(),"Inspect spawn_blocking callsites for long-running CPU or I/O work.".to_string()]))
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
    let score = clamp_score(
        35 + (p95_global.min(150) / 3)
            + (percentile(&local, 95, 100).unwrap_or(0).min(60) / 6)
            + (percentile(&alive, 95, 100).unwrap_or(0).min(400) / 40)
            + growth_bonus
            + u64::from(score_sample_quality(global.len())),
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
        if ss.len() < 3 {
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
        let score = clamp_score(
            35 + (tail_share / 8) + (cum_share / 30) + u64::from(score_sample_quality(ss.len())),
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
    let best = downstream_stage_candidates(run, p95_req, total_req)
        .into_iter()
        .max_by(|a, b| {
            a.score
                .cmp(&b.score)
                .then_with(|| a.tail_share.cmp(&b.tail_share))
                .then_with(|| a.cum_share.cmp(&b.cum_share))
                .then_with(|| b.stage.cmp(&a.stage))
        })?;
    Some(Suspect::new(
        DiagnosisKind::DownstreamStageDominates,
        best.score,
        vec![
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
        ],
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
    if ranked.len() >= 2 && ranked[0].score.abs_diff(ranked[1].score) <= 5 {
        Some("Top suspects are close in score; treat ranking as ambiguous and validate both with next checks.".to_string())
    } else {
        None
    }
}

fn analysis_warnings(run: &Run, suspects: &[Suspect]) -> Vec<String> {
    let mut warnings = truncation_warnings(run);
    if run.requests.len() < 20 {
        warnings.push(
            "Low completed-request count; diagnosis ranking may be unstable for this run window."
                .to_string(),
        );
    }
    if run.queues.is_empty()
        && suspects
            .iter()
            .all(|s| s.kind != DiagnosisKind::DownstreamStageDominates)
    {
        warnings.push(
            "No queue events captured; queue saturation interpretation is limited.".to_string(),
        );
    }
    if run.stages.is_empty()
        && suspects
            .iter()
            .all(|s| s.kind != DiagnosisKind::ApplicationQueueSaturation)
    {
        warnings.push(
            "No stage events captured; downstream-stage interpretation is limited.".to_string(),
        );
    }
    if run.runtime_snapshots.is_empty() {
        warnings.push("No runtime snapshots captured; executor and blocking-pressure interpretation is limited.".to_string());
    } else if run
        .runtime_snapshots
        .iter()
        .all(|s| s.blocking_queue_depth.is_none())
        || run
            .runtime_snapshots
            .iter()
            .all(|s| s.local_queue_depth.is_none())
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

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use tailtriage_core::{
        CaptureMode, EffectiveCoreConfig, RequestEvent, Run, RunMetadata, StageEvent,
        SCHEMA_VERSION,
    };

    use crate::analyze::{
        analyze_run, render_text, Confidence, DiagnosisKind, InflightTrend, Report, Suspect,
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
            primary_suspect: Suspect {
                kind: DiagnosisKind::ApplicationQueueSaturation,
                score: 90,
                confidence: Confidence::High,
                evidence: vec!["queue wait high".to_owned()],
                next_checks: vec!["check queue policy".to_owned()],
            },
            secondary_suspects: Vec::new(),
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
            primary_suspect: Suspect {
                kind: DiagnosisKind::InsufficientEvidence,
                score: 50,
                confidence: Confidence::Low,
                evidence: vec!["missing signals".to_owned()],
                next_checks: vec!["add instrumentation".to_owned()],
            },
            secondary_suspects: Vec::new(),
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
        assert_eq!(report.warnings.len(), 3);
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
}
