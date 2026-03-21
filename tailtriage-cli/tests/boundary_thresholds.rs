use std::path::Path;

use tailtriage_cli::analyze::{analyze_run, DiagnosisKind};
use tailtriage_core::{
    CaptureMode, QueueEvent, RequestEvent, Run, RunMetadata, RuntimeSnapshot, StageEvent,
    TruncationSummary,
};

fn load_fixture(name: &str) -> Run {
    let path = Path::new("tests/fixtures").join(name);
    let content = std::fs::read_to_string(path).expect("fixture should exist");
    serde_json::from_str(&content).expect("fixture should deserialize")
}

fn base_run() -> Run {
    Run {
        metadata: RunMetadata {
            run_id: "threshold-run".to_string(),
            service_name: "svc".to_string(),
            service_version: None,
            started_at_unix_ms: 1,
            finished_at_unix_ms: 2,
            mode: CaptureMode::Light,
            host: None,
            pid: Some(7),
        },
        requests: vec![
            RequestEvent {
                request_id: "r1".to_string(),
                route: "/a".to_string(),
                kind: None,
                started_at_unix_ms: 1,
                finished_at_unix_ms: 2,
                latency_us: 1_000,
                outcome: "ok".to_string(),
            },
            RequestEvent {
                request_id: "r2".to_string(),
                route: "/a".to_string(),
                kind: None,
                started_at_unix_ms: 1,
                finished_at_unix_ms: 2,
                latency_us: 1_000,
                outcome: "ok".to_string(),
            },
            RequestEvent {
                request_id: "r3".to_string(),
                route: "/a".to_string(),
                kind: None,
                started_at_unix_ms: 1,
                finished_at_unix_ms: 2,
                latency_us: 1_000,
                outcome: "ok".to_string(),
            },
        ],
        stages: Vec::new(),
        queues: Vec::new(),
        inflight: Vec::new(),
        runtime_snapshots: Vec::new(),
        truncation: TruncationSummary::default(),
    }
}

#[test]
fn queue_share_threshold_uses_300_permille_boundary() {
    let mut below = base_run();
    below.queues = vec![QueueEvent {
        request_id: "r3".to_string(),
        queue: "worker".to_string(),
        waited_from_unix_ms: 1,
        waited_until_unix_ms: 2,
        wait_us: 299,
        depth_at_start: Some(2),
    }];

    let below_report = analyze_run(&below);
    assert_eq!(below_report.p95_queue_share_permille, Some(299));
    assert_ne!(
        below_report.primary_suspect.kind,
        DiagnosisKind::ApplicationQueueSaturation,
        "queue saturation should not trigger below the 300 permille threshold"
    );

    let mut above = base_run();
    above.queues = vec![QueueEvent {
        request_id: "r3".to_string(),
        queue: "worker".to_string(),
        waited_from_unix_ms: 1,
        waited_until_unix_ms: 2,
        wait_us: 300,
        depth_at_start: Some(2),
    }];

    let above_report = analyze_run(&above);
    assert_eq!(above_report.p95_queue_share_permille, Some(300));
    assert_eq!(
        above_report.primary_suspect.kind,
        DiagnosisKind::ApplicationQueueSaturation,
        "queue saturation should trigger at the 300 permille threshold"
    );
}

#[test]
fn blocking_and_executor_pressure_require_nonzero_p95_depth() {
    let mut zero = base_run();
    zero.runtime_snapshots = vec![
        RuntimeSnapshot {
            at_unix_ms: 1,
            alive_tasks: Some(10),
            global_queue_depth: Some(0),
            local_queue_depth: None,
            blocking_queue_depth: Some(0),
            remote_schedule_count: None,
        },
        RuntimeSnapshot {
            at_unix_ms: 2,
            alive_tasks: Some(10),
            global_queue_depth: Some(0),
            local_queue_depth: None,
            blocking_queue_depth: Some(0),
            remote_schedule_count: None,
        },
    ];

    let zero_report = analyze_run(&zero);
    assert!(zero_report
        .secondary_suspects
        .iter()
        .all(
            |suspect| suspect.kind != DiagnosisKind::BlockingPoolPressure
                && suspect.kind != DiagnosisKind::ExecutorPressureSuspected
        ));
    assert_ne!(
        zero_report.primary_suspect.kind,
        DiagnosisKind::BlockingPoolPressure
    );
    assert_ne!(
        zero_report.primary_suspect.kind,
        DiagnosisKind::ExecutorPressureSuspected
    );

    let mut nonzero = base_run();
    nonzero.runtime_snapshots = vec![
        RuntimeSnapshot {
            at_unix_ms: 1,
            alive_tasks: Some(10),
            global_queue_depth: Some(0),
            local_queue_depth: None,
            blocking_queue_depth: Some(1),
            remote_schedule_count: None,
        },
        RuntimeSnapshot {
            at_unix_ms: 2,
            alive_tasks: Some(10),
            global_queue_depth: Some(2),
            local_queue_depth: None,
            blocking_queue_depth: Some(0),
            remote_schedule_count: None,
        },
    ];

    let nonzero_report = analyze_run(&nonzero);
    let kinds = std::iter::once(&nonzero_report.primary_suspect)
        .chain(nonzero_report.secondary_suspects.iter())
        .map(|suspect| suspect.kind.clone())
        .collect::<Vec<_>>();
    assert!(kinds.contains(&DiagnosisKind::BlockingPoolPressure));
    assert!(kinds.contains(&DiagnosisKind::ExecutorPressureSuspected));
}

#[test]
fn downstream_stage_requires_at_least_three_samples() {
    let mut two_samples = base_run();
    two_samples.stages = vec![
        StageEvent {
            request_id: "r1".to_string(),
            stage: "db".to_string(),
            started_at_unix_ms: 1,
            finished_at_unix_ms: 2,
            latency_us: 350,
            success: true,
        },
        StageEvent {
            request_id: "r2".to_string(),
            stage: "db".to_string(),
            started_at_unix_ms: 1,
            finished_at_unix_ms: 2,
            latency_us: 360,
            success: true,
        },
    ];

    let two_samples_report = analyze_run(&two_samples);
    assert_ne!(
        two_samples_report.primary_suspect.kind,
        DiagnosisKind::DownstreamStageDominates,
        "downstream stage suspect requires at least three samples"
    );

    let mut three_samples = base_run();
    three_samples.stages = vec![
        StageEvent {
            request_id: "r1".to_string(),
            stage: "db".to_string(),
            started_at_unix_ms: 1,
            finished_at_unix_ms: 2,
            latency_us: 350,
            success: true,
        },
        StageEvent {
            request_id: "r2".to_string(),
            stage: "db".to_string(),
            started_at_unix_ms: 1,
            finished_at_unix_ms: 2,
            latency_us: 360,
            success: true,
        },
        StageEvent {
            request_id: "r3".to_string(),
            stage: "db".to_string(),
            started_at_unix_ms: 1,
            finished_at_unix_ms: 2,
            latency_us: 340,
            success: true,
        },
    ];

    let three_samples_report = analyze_run(&three_samples);
    assert_eq!(
        three_samples_report.primary_suspect.kind,
        DiagnosisKind::DownstreamStageDominates,
        "downstream stage suspect should trigger once three samples exist"
    );
}

#[test]
fn mixed_signal_fixtures_rank_higher_score_first() {
    let queue_vs_blocking = analyze_run(&load_fixture("mixed_queue_vs_blocking.json"));
    assert_eq!(
        queue_vs_blocking.primary_suspect.kind,
        DiagnosisKind::ApplicationQueueSaturation
    );
    assert!(
        queue_vs_blocking
            .secondary_suspects
            .iter()
            .any(|suspect| suspect.kind == DiagnosisKind::BlockingPoolPressure),
        "mixed fixture should still include blocking pressure as a plausible secondary suspect"
    );
    assert!(
        queue_vs_blocking
            .secondary_suspects
            .first()
            .is_some_and(|suspect| suspect.kind == DiagnosisKind::BlockingPoolPressure),
        "blocking pressure should be the top secondary suspect by score"
    );

    let blocking_vs_downstream = analyze_run(&load_fixture("mixed_blocking_vs_downstream.json"));
    assert_eq!(
        blocking_vs_downstream.primary_suspect.kind,
        DiagnosisKind::BlockingPoolPressure
    );
    assert!(
        blocking_vs_downstream
            .secondary_suspects
            .iter()
            .any(|suspect| suspect.kind == DiagnosisKind::DownstreamStageDominates),
        "mixed fixture should still include downstream stage dominance as a plausible secondary suspect"
    );
    assert!(
        blocking_vs_downstream
            .secondary_suspects
            .first()
            .is_some_and(|suspect| suspect.kind == DiagnosisKind::DownstreamStageDominates),
        "downstream stage suspect should rank ahead of lower-scoring alternatives"
    );
}
