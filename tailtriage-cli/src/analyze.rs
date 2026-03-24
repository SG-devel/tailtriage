use std::collections::{BTreeMap, HashMap};

use serde::{Serialize, Serializer};
use tailtriage_core::{InFlightSnapshot, Run, RuntimeSnapshot};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosisKind {
    ApplicationQueueSaturation,
    BlockingPoolPressure,
    ExecutorPressureSuspected,
    DownstreamStageDominates,
    InsufficientEvidence,
}

impl DiagnosisKind {
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
pub enum Confidence {
    Low,
    Medium,
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

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Suspect {
    pub kind: DiagnosisKind,
    pub score: u8,
    pub confidence: Confidence,
    pub evidence: Vec<String>,
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

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InflightTrend {
    pub gauge: String,
    pub sample_count: usize,
    pub peak_count: u64,
    pub p95_count: u64,
    pub growth_delta: i64,
    pub growth_per_sec_milli: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Report {
    pub request_count: usize,
    pub p50_latency_us: Option<u64>,
    pub p95_latency_us: Option<u64>,
    pub p99_latency_us: Option<u64>,
    pub p95_queue_share_permille: Option<u64>,
    pub p95_service_share_permille: Option<u64>,
    pub inflight_trend: Option<InflightTrend>,
    pub warnings: Vec<String>,
    pub primary_suspect: Suspect,
    pub secondary_suspects: Vec<Suspect>,
}

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

    suspects.sort_by(|left, right| right.score.cmp(&left.score));

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
        warnings: truncation_warnings(run),
        primary_suspect,
        secondary_suspects: ranked.collect(),
    }
}

fn truncation_warnings(run: &Run) -> Vec<String> {
    let mut warnings = Vec::new();
    if run.truncation.dropped_requests > 0 {
        warnings.push(format!(
            "Capture truncated requests: dropped {} request events after reaching the configured max_requests limit.",
            run.truncation.dropped_requests
        ));
    }
    if run.truncation.dropped_stages > 0 {
        warnings.push(format!(
            "Capture truncated stages: dropped {} stage events after reaching the configured max_stages limit.",
            run.truncation.dropped_stages
        ));
    }
    if run.truncation.dropped_queues > 0 {
        warnings.push(format!(
            "Capture truncated queues: dropped {} queue events after reaching the configured max_queues limit.",
            run.truncation.dropped_queues
        ));
    }
    if run.truncation.dropped_inflight_snapshots > 0 {
        warnings.push(format!(
            "Capture truncated in-flight snapshots: dropped {} entries after reaching max_inflight_snapshots.",
            run.truncation.dropped_inflight_snapshots
        ));
    }
    if run.truncation.dropped_runtime_snapshots > 0 {
        warnings.push(format!(
            "Capture truncated runtime snapshots: dropped {} entries after reaching max_runtime_snapshots.",
            run.truncation.dropped_runtime_snapshots
        ));
    }
    warnings
}

fn queue_saturation_suspect(run: &Run, inflight_trend: Option<&InflightTrend>) -> Option<Suspect> {
    let (queue_shares, _) = request_time_shares(run);
    let p95_queue_share_permille = percentile(&queue_shares, 95, 100)?;
    let max_depth = run
        .queues
        .iter()
        .filter_map(|queue| queue.depth_at_start)
        .max();

    if p95_queue_share_permille < 300 {
        return None;
    }

    let whole_percent = p95_queue_share_permille / 10;
    let tenth_percent = p95_queue_share_permille % 10;
    let mut evidence = vec![format!(
        "Queue wait at p95 consumes {whole_percent}.{tenth_percent}% of request time."
    )];

    if let Some(depth) = max_depth {
        evidence.push(format!("Observed queue depth sample up to {depth}."));
    }
    if let Some(trend) = inflight_trend.filter(|trend| trend.growth_delta > 0) {
        evidence.push(format!(
            "In-flight gauge '{}' grew by {} over the run window (p95={}, peak={}).",
            trend.gauge, trend.growth_delta, trend.p95_count, trend.peak_count
        ));
    }

    Some(Suspect::new(
        DiagnosisKind::ApplicationQueueSaturation,
        90,
        evidence,
        vec![
            "Inspect queue admission limits and producer burst patterns.".to_string(),
            "Compare queue wait distribution before and after increasing worker parallelism."
                .to_string(),
        ],
    ))
}

fn blocking_pressure_suspect(run: &Run) -> Option<Suspect> {
    let blocking_depths = runtime_metric_series(&run.runtime_snapshots, |snapshot| {
        snapshot.blocking_queue_depth
    });
    let p95_blocking_depth = percentile(&blocking_depths, 95, 100)?;

    if p95_blocking_depth == 0 {
        return None;
    }

    Some(Suspect::new(
        DiagnosisKind::BlockingPoolPressure,
        80,
        vec![format!(
            "Blocking queue depth p95 is {p95_blocking_depth}, indicating sustained spawn_blocking backlog."
        )],
        vec![
            "Audit blocking sections and move avoidable synchronous work out of hot paths."
                .to_string(),
            "Inspect spawn_blocking callsites for long-running CPU or I/O work.".to_string(),
        ],
    ))
}

fn executor_pressure_suspect(run: &Run, inflight_trend: Option<&InflightTrend>) -> Option<Suspect> {
    let global_queue_depths = runtime_metric_series(&run.runtime_snapshots, |snapshot| {
        snapshot.global_queue_depth
    });
    let p95_global_depth = percentile(&global_queue_depths, 95, 100)?;

    if p95_global_depth == 0 {
        return None;
    }

    let mut evidence = vec![format!(
        "Runtime global queue depth p95 is {p95_global_depth}, suggesting scheduler contention."
    )];
    let positive_growth = inflight_trend.is_some_and(|trend| trend.growth_delta > 0);
    if let Some(trend) = inflight_trend.filter(|trend| trend.growth_delta > 0) {
        evidence.push(format!(
            "In-flight gauge '{}' growth is positive (delta={}, peak={}), consistent with accumulating executor pressure.",
            trend.gauge, trend.growth_delta, trend.peak_count
        ));
    }

    let depth_bonus = if p95_global_depth >= 300 {
        20
    } else if p95_global_depth >= 200 {
        12
    } else if p95_global_depth >= 100 {
        6
    } else {
        0
    };
    let trend_bonus = if positive_growth { 5 } else { 0 };
    let score = (65 + depth_bonus + trend_bonus).min(90);

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

fn downstream_stage_suspect(run: &Run) -> Option<Suspect> {
    let mut stage_totals: BTreeMap<&str, u64> = BTreeMap::new();
    for stage in &run.stages {
        *stage_totals.entry(stage.stage.as_str()).or_default() = stage_totals
            .get(stage.stage.as_str())
            .copied()
            .unwrap_or_default()
            .saturating_add(stage.latency_us);
    }

    let (dominant_stage, total_latency) = stage_totals
        .iter()
        .max_by(|left, right| left.1.cmp(right.1).then_with(|| right.0.cmp(left.0)))
        .map(|(stage, latency)| (*stage, *latency))?;

    let stage_count = run
        .stages
        .iter()
        .filter(|stage| stage.stage == dominant_stage)
        .count();
    let stage_latencies = run
        .stages
        .iter()
        .filter(|stage| stage.stage == dominant_stage)
        .map(|stage| stage.latency_us)
        .collect::<Vec<_>>();
    let stage_p95 = percentile(&stage_latencies, 95, 100)?;
    let total_request_latency = run
        .requests
        .iter()
        .map(|request| request.latency_us)
        .fold(0_u64, u64::saturating_add);
    let stage_share_permille = if total_request_latency == 0 {
        0
    } else {
        total_latency.saturating_mul(1_000) / total_request_latency
    };
    let share_bonus = (stage_share_permille / 40).min(25) as u8;
    let score = (55 + share_bonus).min(79);

    if stage_count < 3 {
        return None;
    }

    Some(Suspect::new(
        DiagnosisKind::DownstreamStageDominates,
        score,
        vec![
            format!(
                "Stage '{dominant_stage}' has p95 latency {stage_p95} us across {stage_count} samples."
            ),
            format!("Stage '{dominant_stage}' cumulative latency is {total_latency} us."),
            format!(
                "Stage '{dominant_stage}' contributes {stage_share_permille} permille of cumulative request latency."
            ),
        ],
        vec![
            format!("Inspect downstream dependency behind stage '{dominant_stage}'."),
            "Collect downstream service timings and retry behavior during tail windows.".to_string(),
            "Review downstream SLO/error budget and align retry budget/backoff with it.".to_string(),
        ],
    ))
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

#[must_use]
pub fn render_text(report: &Report) -> String {
    let inflight_line = match &report.inflight_trend {
        Some(trend) => format!(
            "inflight_trend gauge={} samples={} peak={} p95={} growth_delta={} growth_per_sec_milli={:?}",
            trend.gauge,
            trend.sample_count,
            trend.peak_count,
            trend.p95_count,
            trend.growth_delta,
            trend.growth_per_sec_milli
        ),
        None => "inflight_trend none".to_string(),
    };

    let mut lines = vec![
        "tailtriage diagnosis".to_string(),
        format!("requests: {}", report.request_count),
        format!(
            "latency_us p50={:?} p95={:?} p99={:?}",
            report.p50_latency_us, report.p95_latency_us, report.p99_latency_us
        ),
        format!(
            "request_time_share_permille p95 queue={:?} service={:?} (independent percentiles; not expected to sum to 1000)",
            report.p95_queue_share_permille, report.p95_service_share_permille
        ),
        inflight_line,
        format!(
            "primary: {} (confidence={:?}, score={})",
            report.primary_suspect.kind.as_str(),
            report.primary_suspect.confidence,
            report.primary_suspect.score
        ),
    ];
    for warning in &report.warnings {
        lines.push(format!("warning {warning}"));
    }

    for evidence in &report.primary_suspect.evidence {
        lines.push(format!("  evidence: {evidence}"));
    }

    for next_check in &report.primary_suspect.next_checks {
        lines.push(format!("  next: {next_check}"));
    }

    if !report.secondary_suspects.is_empty() {
        lines.push("secondary suspects:".to_string());
        for suspect in &report.secondary_suspects {
            lines.push(format!(
                "  - {} (confidence={:?}, score={})",
                suspect.kind.as_str(),
                suspect.confidence,
                suspect.score
            ));
        }
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use tailtriage_core::{
        CaptureMode, RequestEvent, Run, RunMetadata, StageEvent, SCHEMA_VERSION,
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
                mode: CaptureMode::Light,
                host: None,
                pid: Some(1),
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
        assert!(text.contains("inflight_trend gauge=queue_inflight"));
        assert!(text.contains("samples=4"));
        assert!(text.contains("growth_per_sec_milli=Some(2500)"));
        assert!(text.contains("independent percentiles; not expected to sum to 1000"));
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
        assert!(text.contains("inflight_trend none"));
        assert!(text.contains("warning Capture truncated requests."));
    }

    #[test]
    fn analyze_run_emits_truncation_warnings() {
        let mut run = test_run();
        run.truncation.dropped_requests = 2;
        run.truncation.dropped_runtime_snapshots = 1;

        let report = analyze_run(&run);
        assert_eq!(report.warnings.len(), 2);
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
