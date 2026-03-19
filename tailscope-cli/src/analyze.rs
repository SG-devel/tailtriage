use std::collections::{BTreeMap, HashMap};

use serde::Serialize;
use tailscope_core::{Run, RuntimeSnapshot};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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
pub struct Report {
    pub request_count: usize,
    pub p50_latency_us: Option<u64>,
    pub p95_latency_us: Option<u64>,
    pub p99_latency_us: Option<u64>,
    pub p95_queue_share_permille: Option<u64>,
    pub p95_service_share_permille: Option<u64>,
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

    let mut suspects = Vec::new();

    if let Some(queue_suspect) = queue_saturation_suspect(run) {
        suspects.push(queue_suspect);
    }

    if let Some(blocking_suspect) = blocking_pressure_suspect(run) {
        suspects.push(blocking_suspect);
    }

    if let Some(executor_suspect) = executor_pressure_suspect(run) {
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
                "Wrap critical awaits with queue(...).await_on(...) and stage(...).await_on(...)."
                    .to_string(),
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
        primary_suspect,
        secondary_suspects: ranked.collect(),
    }
}

fn queue_saturation_suspect(run: &Run) -> Option<Suspect> {
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

fn executor_pressure_suspect(run: &Run) -> Option<Suspect> {
    let global_queue_depths = runtime_metric_series(&run.runtime_snapshots, |snapshot| {
        snapshot.global_queue_depth
    });
    let p95_global_depth = percentile(&global_queue_depths, 95, 100)?;

    if p95_global_depth == 0 {
        return None;
    }

    Some(Suspect::new(
        DiagnosisKind::ExecutorPressureSuspected,
        65,
        vec![format!(
            "Runtime global queue depth p95 is {p95_global_depth}, suggesting scheduler contention."
        )],
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

    if stage_count < 3 {
        return None;
    }

    Some(Suspect::new(
        DiagnosisKind::DownstreamStageDominates,
        60,
        vec![
            format!(
                "Stage '{dominant_stage}' has p95 latency {stage_p95} us across {stage_count} samples."
            ),
            format!("Stage '{dominant_stage}' cumulative latency is {total_latency} us."),
        ],
        vec![
            format!("Inspect downstream dependency behind stage '{dominant_stage}'."),
            "Collect downstream service timings and retry behavior during tail windows.".to_string(),
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
    let mut lines = vec![
        "tailscope diagnosis".to_string(),
        format!("requests: {}", report.request_count),
        format!(
            "latency_us p50={:?} p95={:?} p99={:?}",
            report.p50_latency_us, report.p95_latency_us, report.p99_latency_us
        ),
        format!(
            "request_time_share_permille p95 queue={:?} service={:?}",
            report.p95_queue_share_permille, report.p95_service_share_permille
        ),
        format!(
            "primary: {} (confidence={:?}, score={})",
            report.primary_suspect.kind.as_str(),
            report.primary_suspect.confidence,
            report.primary_suspect.score
        ),
    ];

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
    use tailscope_core::{CaptureMode, RequestEvent, Run, RunMetadata, StageEvent};

    use crate::analyze::{analyze_run, DiagnosisKind};

    fn test_run() -> Run {
        Run {
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
}
