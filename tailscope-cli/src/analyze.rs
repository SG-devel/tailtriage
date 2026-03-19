use std::collections::HashMap;

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

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Suspect {
    pub kind: DiagnosisKind,
    pub score: u8,
    pub evidence: Vec<String>,
    pub next_checks: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Report {
    pub request_count: usize,
    pub p50_latency_us: Option<u64>,
    pub p95_latency_us: Option<u64>,
    pub p99_latency_us: Option<u64>,
    pub suspects: Vec<Suspect>,
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
        suspects.push(Suspect {
            kind: DiagnosisKind::InsufficientEvidence,
            score: 100,
            evidence: vec![
                "Not enough queue, stage, or runtime signals to rank a stronger suspect."
                    .to_string(),
            ],
            next_checks: vec![
                "Wrap critical awaits with queue(...).await_on(...) and stage(...).await_on(...)."
                    .to_string(),
                "Enable RuntimeSampler during the run to capture runtime pressure signals."
                    .to_string(),
            ],
        });
    }

    suspects.sort_by(|left, right| right.score.cmp(&left.score));

    Report {
        request_count: run.requests.len(),
        p50_latency_us,
        p95_latency_us,
        p99_latency_us,
        suspects,
    }
}

fn queue_saturation_suspect(run: &Run) -> Option<Suspect> {
    let queue_shares = request_queue_shares(run);
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

    Some(Suspect {
        kind: DiagnosisKind::ApplicationQueueSaturation,
        score: 90,
        evidence,
        next_checks: vec![
            "Inspect queue admission limits and producer burst patterns.".to_string(),
            "Compare queue wait distribution before and after increasing worker parallelism."
                .to_string(),
        ],
    })
}

fn blocking_pressure_suspect(run: &Run) -> Option<Suspect> {
    let blocking_depths = runtime_metric_series(&run.runtime_snapshots, |snapshot| {
        snapshot.blocking_queue_depth
    });
    let p95_blocking_depth = percentile(&blocking_depths, 95, 100)?;

    if p95_blocking_depth == 0 {
        return None;
    }

    Some(Suspect {
        kind: DiagnosisKind::BlockingPoolPressure,
        score: 80,
        evidence: vec![format!(
            "Blocking queue depth p95 is {p95_blocking_depth}, indicating sustained spawn_blocking backlog."
        )],
        next_checks: vec![
            "Audit blocking sections and move avoidable synchronous work out of hot paths.".to_string(),
            "Inspect spawn_blocking callsites for long-running CPU or I/O work.".to_string(),
        ],
    })
}

fn executor_pressure_suspect(run: &Run) -> Option<Suspect> {
    let global_queue_depths = runtime_metric_series(&run.runtime_snapshots, |snapshot| {
        snapshot.global_queue_depth
    });
    let p95_global_depth = percentile(&global_queue_depths, 95, 100)?;

    if p95_global_depth == 0 {
        return None;
    }

    Some(Suspect {
        kind: DiagnosisKind::ExecutorPressureSuspected,
        score: 65,
        evidence: vec![format!(
            "Runtime global queue depth p95 is {p95_global_depth}, suggesting scheduler contention."
        )],
        next_checks: vec![
            "Check for long polls without yielding and uneven task fan-out.".to_string(),
            "Compare with per-stage timings to isolate overloaded async stages.".to_string(),
        ],
    })
}

fn downstream_stage_suspect(run: &Run) -> Option<Suspect> {
    let mut stage_totals: HashMap<&str, u64> = HashMap::new();
    for stage in &run.stages {
        *stage_totals.entry(stage.stage.as_str()).or_default() = stage_totals
            .get(stage.stage.as_str())
            .copied()
            .unwrap_or_default()
            .saturating_add(stage.latency_us);
    }

    let (dominant_stage, total_latency) = stage_totals
        .iter()
        .max_by(|left, right| left.1.cmp(right.1))
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

    Some(Suspect {
        kind: DiagnosisKind::DownstreamStageDominates,
        score: 60,
        evidence: vec![
            format!(
                "Stage '{dominant_stage}' has p95 latency {stage_p95} us across {stage_count} samples."
            ),
            format!("Stage '{dominant_stage}' cumulative latency is {total_latency} us."),
        ],
        next_checks: vec![
            format!("Inspect downstream dependency behind stage '{dominant_stage}'."),
            "Collect downstream service timings and retry behavior during tail windows.".to_string(),
        ],
    })
}

fn request_queue_shares(run: &Run) -> Vec<u64> {
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

    run.requests
        .iter()
        .filter_map(|request| {
            if request.latency_us == 0 {
                return None;
            }

            let queue_wait = total_queue_wait_by_request
                .get(request.request_id.as_str())
                .copied()
                .unwrap_or_default();
            Some(queue_wait.saturating_mul(1_000) / request.latency_us)
        })
        .collect()
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
    ];

    for (index, suspect) in report.suspects.iter().enumerate() {
        lines.push(format!(
            "{}. {} (score={})",
            index + 1,
            suspect.kind.as_str(),
            suspect.score
        ));

        for evidence in &suspect.evidence {
            lines.push(format!("   evidence: {evidence}"));
        }

        for next_check in &suspect.next_checks {
            lines.push(format!("   next: {next_check}"));
        }
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use tailscope_core::{
        CaptureMode, QueueEvent, RequestEvent, Run, RunMetadata, RuntimeSnapshot, StageEvent,
    };

    use super::{analyze_run, DiagnosisKind};

    fn fixture_run() -> Run {
        let mut run = Run::new(RunMetadata {
            run_id: "run-test".to_string(),
            service_name: "svc".to_string(),
            service_version: None,
            started_at_unix_ms: 1,
            finished_at_unix_ms: 2,
            mode: CaptureMode::Light,
            host: None,
            pid: Some(42),
        });

        run.requests = vec![
            RequestEvent {
                request_id: "r1".to_string(),
                route: "/a".to_string(),
                kind: None,
                started_at_unix_ms: 1,
                finished_at_unix_ms: 2,
                latency_us: 100,
                outcome: "ok".to_string(),
            },
            RequestEvent {
                request_id: "r2".to_string(),
                route: "/a".to_string(),
                kind: None,
                started_at_unix_ms: 1,
                finished_at_unix_ms: 2,
                latency_us: 200,
                outcome: "ok".to_string(),
            },
            RequestEvent {
                request_id: "r3".to_string(),
                route: "/a".to_string(),
                kind: None,
                started_at_unix_ms: 1,
                finished_at_unix_ms: 2,
                latency_us: 300,
                outcome: "ok".to_string(),
            },
        ];

        run
    }

    #[test]
    fn prioritizes_queue_saturation_when_queue_share_is_high() {
        let mut run = fixture_run();
        run.queues = vec![
            QueueEvent {
                request_id: "r1".to_string(),
                queue: "q".to_string(),
                waited_from_unix_ms: 1,
                waited_until_unix_ms: 2,
                wait_us: 80,
                depth_at_start: Some(3),
            },
            QueueEvent {
                request_id: "r2".to_string(),
                queue: "q".to_string(),
                waited_from_unix_ms: 1,
                waited_until_unix_ms: 2,
                wait_us: 150,
                depth_at_start: Some(4),
            },
            QueueEvent {
                request_id: "r3".to_string(),
                queue: "q".to_string(),
                waited_from_unix_ms: 1,
                waited_until_unix_ms: 2,
                wait_us: 220,
                depth_at_start: Some(5),
            },
        ];

        let report = analyze_run(&run);
        assert_eq!(
            report.suspects.first().map(|suspect| &suspect.kind),
            Some(&DiagnosisKind::ApplicationQueueSaturation)
        );
    }

    #[test]
    fn detects_blocking_pool_pressure_from_runtime_snapshots() {
        let mut run = fixture_run();
        run.runtime_snapshots = vec![
            RuntimeSnapshot {
                at_unix_ms: 1,
                alive_tasks: Some(10),
                global_queue_depth: Some(0),
                local_queue_depth: None,
                blocking_queue_depth: Some(2),
                remote_schedule_count: None,
            },
            RuntimeSnapshot {
                at_unix_ms: 2,
                alive_tasks: Some(11),
                global_queue_depth: Some(0),
                local_queue_depth: None,
                blocking_queue_depth: Some(3),
                remote_schedule_count: None,
            },
        ];

        let report = analyze_run(&run);
        assert!(report
            .suspects
            .iter()
            .any(|suspect| suspect.kind == DiagnosisKind::BlockingPoolPressure));
    }

    #[test]
    fn falls_back_to_insufficient_evidence_without_signals() {
        let run = fixture_run();

        let report = analyze_run(&run);
        assert_eq!(
            report.suspects.first().map(|suspect| &suspect.kind),
            Some(&DiagnosisKind::InsufficientEvidence)
        );
    }

    #[test]
    fn can_identify_dominant_stage() {
        let mut run = fixture_run();
        run.stages = vec![
            StageEvent {
                request_id: "r1".to_string(),
                stage: "db".to_string(),
                started_at_unix_ms: 1,
                finished_at_unix_ms: 2,
                latency_us: 70,
                success: true,
            },
            StageEvent {
                request_id: "r2".to_string(),
                stage: "db".to_string(),
                started_at_unix_ms: 1,
                finished_at_unix_ms: 2,
                latency_us: 90,
                success: true,
            },
            StageEvent {
                request_id: "r3".to_string(),
                stage: "db".to_string(),
                started_at_unix_ms: 1,
                finished_at_unix_ms: 2,
                latency_us: 120,
                success: true,
            },
        ];

        let report = analyze_run(&run);
        assert!(report
            .suspects
            .iter()
            .any(|suspect| suspect.kind == DiagnosisKind::DownstreamStageDominates));
    }
}
