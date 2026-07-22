use tailtriage_core::{
    CaptureMode, EffectiveCoreConfig, QueueEvent, RequestEvent, Run, RunMetadata, RuntimeSnapshot,
    StageEvent, SCHEMA_VERSION,
};

use super::temporal::{
    apply_temporal_overlap_attribution_warning, has_material_p95_shift,
    TEMPORAL_OVERLAP_ATTRIBUTION_WARNING, TEMPORAL_P95_SHIFT_WARNING,
    TEMPORAL_SUSPECT_SHIFT_WARNING, TEMPORAL_WALL_CLOCK_FALLBACK_WARNING,
};
use crate::{
    analyze_run, analyze_run_internal, analyze_run_json_pretty, evidence, render_json,
    render_json_pretty, render_text, validate_artifact_strict, AnalyzeConfigError, AnalyzeOptions,
    ArtifactValidationError, Confidence, DiagnosisKind, EvidenceQuality, EvidenceQualityLevel,
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
                started_at_run_us: None,
                finished_at_unix_ms: 2,
                finished_at_run_us: None,
                latency_us: 1_000,
                outcome: "ok".to_owned(),
            },
            RequestEvent {
                request_id: "req-2".to_owned(),
                route: "/test".to_owned(),
                kind: None,
                started_at_unix_ms: 2,
                started_at_run_us: None,
                finished_at_unix_ms: 3,
                finished_at_run_us: None,
                latency_us: 1_000,
                outcome: "ok".to_owned(),
            },
            RequestEvent {
                request_id: "req-3".to_owned(),
                route: "/test".to_owned(),
                kind: None,
                started_at_unix_ms: 3,
                started_at_run_us: None,
                finished_at_unix_ms: 4,
                finished_at_run_us: None,
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
        started_at_run_us: None,
        finished_at_unix_ms: id + 1,
        finished_at_run_us: None,
        latency_us: 1_000,
        outcome: "ok".into(),
    }
}

fn precise_request(id: &str, latency_us: u64) -> RequestEvent {
    RequestEvent {
        request_id: id.to_owned(),
        route: "/precise".into(),
        kind: None,
        started_at_unix_ms: 10,
        started_at_run_us: Some(0),
        finished_at_unix_ms: 11,
        finished_at_run_us: Some(latency_us),
        latency_us,
        outcome: "ok".into(),
    }
}

fn precise_queue(id: &str, start: u64, end: u64, wait_us: u64) -> QueueEvent {
    QueueEvent {
        request_id: id.to_owned(),
        queue: "worker".into(),
        waited_from_unix_ms: 10,
        waited_from_run_us: Some(start),
        waited_until_unix_ms: 11,
        waited_until_run_us: Some(end),
        wait_us,
        depth_at_start: Some(1),
    }
}

#[test]
fn overlapping_precise_queues_are_union_attributed() {
    let mut run = test_run();
    run.requests = vec![precise_request("req-overlap", 100)];
    run.queues = vec![
        precise_queue("req-overlap", 0, 60, 60),
        precise_queue("req-overlap", 40, 90, 50),
    ];

    let report = analyze_run(&run, AnalyzeOptions::default());

    assert_eq!(report.p95_queue_share_permille, Some(900));
    assert_eq!(report.p95_service_share_permille, Some(100));
}

#[test]
fn missing_run_relative_queue_endpoint_falls_back_to_capped_duration_sum() {
    let mut run = test_run();
    run.requests = vec![precise_request("req-approx", 100)];
    run.queues = vec![
        precise_queue("req-approx", 0, 40, 40),
        QueueEvent {
            request_id: "req-approx".into(),
            queue: "worker".into(),
            waited_from_unix_ms: 10,
            waited_from_run_us: None,
            waited_until_unix_ms: 11,
            waited_until_run_us: None,
            wait_us: 90,
            depth_at_start: Some(1),
        },
    ];

    let report = analyze_run(&run, AnalyzeOptions::default());

    assert_eq!(report.p95_queue_share_permille, Some(1000));
    assert_eq!(report.p95_service_share_permille, Some(0));
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("precise_interval_validation_unavailable")));
}

#[test]
fn out_of_parent_precise_queue_is_excluded_before_attribution_not_clipped() {
    let mut run = test_run();
    run.requests = vec![precise_request("req-boundary", 100)];
    run.queues = vec![
        precise_queue("req-boundary", 10, 40, 30),
        precise_queue("req-boundary", 80, 120, 40),
    ];

    let report = analyze_run(&run, AnalyzeOptions::default());

    assert_eq!(report.p95_queue_share_permille, Some(300));
    assert_eq!(report.p95_service_share_permille, Some(700));
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("child_interval_outside_request")
            && warning.contains("queue")));
}

#[test]
fn non_overlapping_queue_attribution_remains_stable() {
    let mut run = test_run();
    run.requests = vec![precise_request("req-stable", 100)];
    run.queues = vec![
        precise_queue("req-stable", 0, 20, 20),
        precise_queue("req-stable", 40, 70, 30),
    ];

    let report = analyze_run(&run, AnalyzeOptions::default());

    assert_eq!(report.p95_queue_share_permille, Some(500));
    assert_eq!(report.p95_service_share_permille, Some(500));
}

#[test]
fn repeated_analysis_is_deterministic_for_overlap_safe_queue_attribution() {
    let mut run = test_run();
    run.requests = vec![precise_request("req-deterministic", 100)];
    run.queues = vec![
        precise_queue("req-deterministic", 40, 90, 50),
        precise_queue("req-deterministic", 0, 60, 60),
    ];

    let first = analyze_run(&run, AnalyzeOptions::default());
    let first_json = render_json(&first).expect("render first report");
    for _ in 0..10 {
        let next = analyze_run(&run, AnalyzeOptions::default());
        let next_json = render_json(&next).expect("render next report");
        assert_eq!(next, first);
        assert_eq!(next_json, first_json);
    }
}

#[test]
fn interleaved_queue_events_group_by_request_and_preserve_request_order() {
    let mut run = test_run();
    run.requests = vec![precise_request("req-a", 200), precise_request("req-b", 100)];
    run.queues = vec![
        precise_queue("req-a", 0, 30, 30),
        precise_queue("req-b", 0, 20, 20),
        precise_queue("req-a", 100, 150, 50),
        precise_queue("req-b", 40, 80, 40),
    ];

    let shares = super::request_time_shares(&run);

    assert_eq!(shares.queue, vec![400, 600]);
    assert_eq!(shares.service, vec![600, 400]);

    let report = analyze_run(&run, AnalyzeOptions::default());
    assert_eq!(report.p95_queue_share_permille, Some(600));
    assert_eq!(report.p95_service_share_permille, Some(600));
}

#[test]
fn duplicate_completed_request_ids_emit_warning_without_panic() {
    let mut run = test_run();
    run.requests[1].request_id = "req-1".to_owned();
    run.queues = vec![QueueEvent {
        request_id: "req-1".to_owned(),
        queue: "worker".to_owned(),
        waited_from_unix_ms: 1,
        waited_from_run_us: None,
        waited_until_unix_ms: 2,
        waited_until_run_us: None,
        wait_us: 500,
        depth_at_start: Some(3),
    }];

    let report = analyze_run(&run, AnalyzeOptions::default());

    assert_eq!(report.request_count, 1);
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("duplicate_completed_request_id")
            && warning.contains("request_id")));
}

#[test]
fn unique_completed_request_ids_do_not_emit_duplicate_warning() {
    let report = analyze_run(&test_run(), AnalyzeOptions::default());

    assert!(!report
        .warnings
        .iter()
        .any(|warning| warning.contains("duplicate_completed_request_id")));
}

#[test]
fn strict_artifact_validation_fails_duplicate_completed_request_ids() {
    let mut run = test_run();
    run.requests[2].request_id = "req-1".to_owned();

    let err =
        validate_artifact_strict(&run).expect_err("duplicate ids should fail strict validation");

    assert!(matches!(
        err,
        ArtifactValidationError::DuplicateCompletedRequestId { ref request_ids }
            if request_ids == &vec!["req-1".to_owned()]
    ));
}

#[test]
fn strict_artifact_validation_fails_orphan_stage_and_queue_request_ids() {
    let mut stage_run = test_run();
    stage_run.stages = vec![StageEvent {
        request_id: "missing-stage-request".to_owned(),
        stage: "db".to_owned(),
        started_at_unix_ms: 1,
        started_at_run_us: None,
        finished_at_unix_ms: 2,
        finished_at_run_us: None,
        latency_us: 100,
        success: true,
    }];
    let stage_err = validate_artifact_strict(&stage_run)
        .expect_err("orphan stage id should fail strict validation");
    assert!(matches!(
        stage_err,
        ArtifactValidationError::OrphanRequestScopedEvent {
            section: "stage",
            ref request_ids,
        } if request_ids == &vec!["missing-stage-request".to_owned()]
    ));

    let mut queue_run = test_run();
    queue_run.queues = vec![QueueEvent {
        request_id: "missing-queue-request".to_owned(),
        queue: "worker".to_owned(),
        waited_from_unix_ms: 1,
        waited_from_run_us: None,
        waited_until_unix_ms: 2,
        waited_until_run_us: None,
        wait_us: 100,
        depth_at_start: Some(1),
    }];
    let queue_err = validate_artifact_strict(&queue_run)
        .expect_err("orphan queue id should fail strict validation");
    assert!(matches!(
        queue_err,
        ArtifactValidationError::OrphanRequestScopedEvent {
            section: "queue",
            ref request_ids,
        } if request_ids == &vec!["missing-queue-request".to_owned()]
    ));
}

#[test]
fn strict_artifact_validation_simultaneous_stage_and_queue_orphans_return_core_with_source() {
    let mut run = test_run();
    run.stages = vec![StageEvent {
        request_id: "missing-stage-request".to_owned(),
        stage: "db".to_owned(),
        started_at_unix_ms: 1,
        started_at_run_us: None,
        finished_at_unix_ms: 2,
        finished_at_run_us: None,
        latency_us: 100,
        success: true,
    }];
    run.queues = vec![QueueEvent {
        request_id: "missing-queue-request".to_owned(),
        queue: "worker".to_owned(),
        waited_from_unix_ms: 1,
        waited_from_run_us: None,
        waited_until_unix_ms: 2,
        waited_until_run_us: None,
        wait_us: 100,
        depth_at_start: Some(1),
    }];

    let err = validate_artifact_strict(&run)
        .expect_err("multi-section orphan failures should preserve core report");

    assert!(matches!(err, ArtifactValidationError::Core(_)));
    assert!(std::error::Error::source(&err).is_some());
}

#[test]
fn strict_artifact_validation_duplicate_plus_metadata_returns_core_with_source() {
    let mut run = test_run();
    run.metadata.service_name = " ".to_owned();
    run.requests[2].request_id = "req-1".to_owned();

    let err = validate_artifact_strict(&run).expect_err("mixed failures should use core");

    assert!(matches!(err, ArtifactValidationError::Core(_)));
    assert!(std::error::Error::source(&err).is_some());
}

#[test]
fn strict_artifact_validation_orphan_plus_timing_returns_core() {
    let mut run = test_run();
    run.stages = vec![StageEvent {
        request_id: "missing-stage-request".to_owned(),
        stage: "db".to_owned(),
        started_at_unix_ms: 2,
        started_at_run_us: None,
        finished_at_unix_ms: 1,
        finished_at_run_us: None,
        latency_us: 100,
        success: true,
    }];

    let err = validate_artifact_strict(&run).expect_err("mixed failures should use core");

    assert!(matches!(err, ArtifactValidationError::Core(_)));
}

#[test]
fn strict_artifact_validation_compatibility_variants_have_no_source() {
    let mut run = test_run();
    run.requests[2].request_id = "req-1".to_owned();
    let err = validate_artifact_strict(&run).expect_err("duplicate ids should fail");
    assert!(std::error::Error::source(&err).is_none());

    let mut orphan_run = test_run();
    orphan_run.stages = vec![StageEvent {
        request_id: "missing-stage-request".to_owned(),
        stage: "db".to_owned(),
        started_at_unix_ms: 1,
        started_at_run_us: None,
        finished_at_unix_ms: 2,
        finished_at_run_us: None,
        latency_us: 100,
        success: true,
    }];
    let err = validate_artifact_strict(&orphan_run).expect_err("orphan should fail");
    assert!(std::error::Error::source(&err).is_none());
}

#[test]
fn permissive_analysis_warns_but_accepts_orphan_request_scoped_events() {
    let mut run = test_run();
    run.stages = vec![StageEvent {
        request_id: "missing-stage-request".to_owned(),
        stage: "db".to_owned(),
        started_at_unix_ms: 1,
        started_at_run_us: None,
        finished_at_unix_ms: 2,
        finished_at_run_us: None,
        latency_us: 100,
        success: true,
    }];
    run.queues = vec![QueueEvent {
        request_id: "missing-queue-request".to_owned(),
        queue: "worker".to_owned(),
        waited_from_unix_ms: 1,
        waited_from_run_us: None,
        waited_until_unix_ms: 2,
        waited_until_run_us: None,
        wait_us: 100,
        depth_at_start: Some(1),
    }];

    let report = analyze_run(&run, AnalyzeOptions::default());

    assert_eq!(report.request_count, 3);
    assert!(report.warnings.iter().any(|warning| {
        warning.contains("orphan_request_scoped_event") && warning.contains("stage")
    }));
    assert!(report.warnings.iter().any(|warning| {
        warning.contains("orphan_request_scoped_event") && warning.contains("queue")
    }));
}

#[test]
fn matching_unique_request_scoped_events_do_not_add_request_id_limitations() {
    let mut run = test_run();
    run.stages = vec![StageEvent {
        request_id: "req-1".to_owned(),
        stage: "db".to_owned(),
        started_at_unix_ms: 1,
        started_at_run_us: None,
        finished_at_unix_ms: 2,
        finished_at_run_us: None,
        latency_us: 100,
        success: true,
    }];
    run.queues = vec![QueueEvent {
        request_id: "req-2".to_owned(),
        queue: "worker".to_owned(),
        waited_from_unix_ms: 1,
        waited_from_run_us: None,
        waited_until_unix_ms: 2,
        waited_until_run_us: None,
        wait_us: 100,
        depth_at_start: Some(1),
    }];

    let report = analyze_run(&run, AnalyzeOptions::default());

    assert!(!report
        .evidence_quality
        .limitations
        .iter()
        .any(|limitation| limitation
            == "Stage or queue evidence with no matching completed request_id cannot be reliably attributed."));
}

#[test]
fn latency_percentiles_use_duration_fields_not_timestamp_subtraction() {
    let mut run = test_run();
    run.metadata.started_at_unix_ms = 10;
    run.metadata.finalized_at_unix_ms = Some(11);
    run.requests = vec![RequestEvent {
        request_id: "req-duration".to_owned(),
        route: "/timing".to_owned(),
        kind: None,
        started_at_unix_ms: 10,
        started_at_run_us: Some(1_000),
        finished_at_unix_ms: 11,
        finished_at_run_us: Some(2_000),
        latency_us: 50_000,
        outcome: "ok".to_owned(),
    }];

    let report = analyze_run(&run, AnalyzeOptions::default());

    assert_eq!(report.p50_latency_us, Some(50_000));
    assert_eq!(report.p95_latency_us, Some(50_000));
    assert_eq!(report.p99_latency_us, Some(50_000));
}

fn runtime_snapshot(
    global: Option<u64>,
    local: Option<u64>,
    blocking: Option<u64>,
) -> RuntimeSnapshot {
    RuntimeSnapshot {
        at_unix_ms: 1,
        at_run_us: None,
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
            started_at_run_us: None,
            finished_at_unix_ms: 2,
            finished_at_run_us: None,
            latency_us: 300,
            success: true,
        },
        StageEvent {
            request_id: "req-2".to_owned(),
            stage: "stage_a".to_owned(),
            started_at_unix_ms: 2,
            started_at_run_us: None,
            finished_at_unix_ms: 3,
            finished_at_run_us: None,
            latency_us: 300,
            success: true,
        },
        StageEvent {
            request_id: "req-3".to_owned(),
            stage: "stage_a".to_owned(),
            started_at_unix_ms: 3,
            started_at_run_us: None,
            finished_at_unix_ms: 4,
            finished_at_run_us: None,
            latency_us: 300,
            success: true,
        },
        StageEvent {
            request_id: "req-1".to_owned(),
            stage: "stage_b".to_owned(),
            started_at_unix_ms: 1,
            started_at_run_us: None,
            finished_at_unix_ms: 2,
            finished_at_run_us: None,
            latency_us: 300,
            success: true,
        },
        StageEvent {
            request_id: "req-2".to_owned(),
            stage: "stage_b".to_owned(),
            started_at_unix_ms: 2,
            started_at_run_us: None,
            finished_at_unix_ms: 3,
            finished_at_run_us: None,
            latency_us: 300,
            success: true,
        },
        StageEvent {
            request_id: "req-3".to_owned(),
            stage: "stage_b".to_owned(),
            started_at_unix_ms: 3,
            started_at_run_us: None,
            finished_at_unix_ms: 4,
            finished_at_run_us: None,
            latency_us: 300,
            success: true,
        },
    ];

    let report = analyze_run(&run, AnalyzeOptions::default());
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
            at_run_us: None,
            count: 3,
        },
        tailtriage_core::InFlightSnapshot {
            gauge: "http".to_owned(),
            at_unix_ms: 20,
            at_run_us: None,
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
            at_run_us: None,
            count: 1,
        },
        tailtriage_core::InFlightSnapshot {
            gauge: "http".to_owned(),
            at_unix_ms: 20,
            at_run_us: None,
            count: 4,
        },
        tailtriage_core::InFlightSnapshot {
            gauge: "http".to_owned(),
            at_unix_ms: 30,
            at_run_us: None,
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
        analyzer_config: None,
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
        analyzer_config: None,
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

    let report = analyze_run(&run, AnalyzeOptions::default());
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
    let report = analyze_run(&test_run(), AnalyzeOptions::default());
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
            waited_from_run_us: None,
            waited_until_unix_ms: 1,
            waited_until_run_us: None,
            depth_at_start: Some(9),
        },
        QueueEvent {
            request_id: "req-2".into(),
            queue: "q".into(),
            wait_us: 900,
            waited_from_unix_ms: 1,
            waited_from_run_us: None,
            waited_until_unix_ms: 2,
            waited_until_run_us: None,
            depth_at_start: Some(9),
        },
        QueueEvent {
            request_id: "req-3".into(),
            queue: "q".into(),
            wait_us: 900,
            waited_from_unix_ms: 2,
            waited_from_run_us: None,
            waited_until_unix_ms: 3,
            waited_until_run_us: None,
            depth_at_start: Some(9),
        },
    ];
    let report = analyze_run(&run, AnalyzeOptions::default());
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
fn runtime_missing_warning_uses_configured_high_confidence_threshold() {
    let mut run = test_run();
    run.queues = vec![
        QueueEvent {
            request_id: "req-1".into(),
            queue: "q".into(),
            wait_us: 900,
            waited_from_unix_ms: 0,
            waited_from_run_us: None,
            waited_until_unix_ms: 1,
            waited_until_run_us: None,
            depth_at_start: Some(9),
        },
        QueueEvent {
            request_id: "req-2".into(),
            queue: "q".into(),
            wait_us: 900,
            waited_from_unix_ms: 1,
            waited_from_run_us: None,
            waited_until_unix_ms: 2,
            waited_until_run_us: None,
            depth_at_start: Some(9),
        },
    ];

    let default_report = analyze_run(&run, AnalyzeOptions::default());
    assert_eq!(
        default_report.primary_suspect.kind,
        DiagnosisKind::ApplicationQueueSaturation
    );
    assert!(default_report.primary_suspect.score >= 85);
    assert!(default_report.primary_suspect.score < 95);
    assert!(!default_report
        .warnings
        .iter()
        .any(|w| w.contains("No runtime snapshots captured")));
    assert!(default_report.analyzer_config.is_none());

    let strict_options = AnalyzeOptions::default().with_confidence(|o| o.high_score_threshold = 95);
    let strict_report = analyze_run(&run, strict_options);
    assert!(strict_report
        .warnings
        .iter()
        .any(|w| w.contains("No runtime snapshots captured")));
    assert!(strict_report.analyzer_config.is_some());
}

#[test]
fn runtime_warning_emitted_when_insufficient_evidence() {
    let report = analyze_run(&test_run(), AnalyzeOptions::default());
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
            started_at_run_us: None,
            finished_at_unix_ms: 2,
            finished_at_run_us: None,
            latency_us: 900,
            success: true,
        },
        StageEvent {
            request_id: "req-2".into(),
            stage: "db".into(),
            started_at_unix_ms: 2,
            started_at_run_us: None,
            finished_at_unix_ms: 3,
            finished_at_run_us: None,
            latency_us: 900,
            success: true,
        },
        StageEvent {
            request_id: "req-3".into(),
            stage: "db".into(),
            started_at_unix_ms: 3,
            started_at_run_us: None,
            finished_at_unix_ms: 4,
            finished_at_run_us: None,
            latency_us: 900,
            success: true,
        },
    ];
    run.runtime_snapshots = vec![runtime_snapshot(Some(2), Some(1), Some(1)); 5];
    let report = analyze_run(&run, AnalyzeOptions::default());
    assert_eq!(
        report.primary_suspect.kind,
        DiagnosisKind::DownstreamStageDominates
    );
}

#[test]
fn score_100_is_reserved_for_overwhelming_queue_evidence() {
    let mut run = test_run();
    run.requests = (0_u64..40)
        .map(|i| RequestEvent {
            request_id: format!("req-{i}"),
            route: "/test".into(),
            kind: None,
            started_at_unix_ms: i,
            started_at_run_us: None,
            finished_at_unix_ms: i + 1,
            finished_at_run_us: None,
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
            waited_from_run_us: None,
            waited_until_unix_ms: 2,
            waited_until_run_us: None,
            wait_us: 990,
            depth_at_start: Some(20),
        })
        .collect();
    let report = analyze_run(&run, AnalyzeOptions::default());
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
    assert!(super::ambiguity_warning(&suspects, &AnalyzeOptions::default()).is_some());
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
            started_at_run_us: None,
            finished_at_unix_ms: i + 1,
            finished_at_run_us: None,
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
            started_at_run_us: None,
            finished_at_unix_ms: 2,
            finished_at_run_us: None,
            latency_us: 3_900_000,
            success: true,
        })
        .collect();
    run.runtime_snapshots = vec![runtime_snapshot(Some(1), Some(1), Some(240)); 80];
    let report = analyze_run(&run, AnalyzeOptions::default());
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
    assert!(!super::scoring::stage_correlates_with_blocking_pool(
        "db_query",
        &AnalyzeOptions::default()
    ));
    assert!(!super::scoring::stage_correlates_with_blocking_pool(
        "retry_attempt",
        &AnalyzeOptions::default()
    ));
    assert!(super::scoring::stage_correlates_with_blocking_pool(
        "spawn_blocking_path",
        &AnalyzeOptions::default()
    ));
}

#[test]
fn downstream_blocking_correlation_margin_changes_downstream_cap_behavior() {
    let mut run = test_run();
    run.requests = (0..40)
        .map(|i| RequestEvent {
            request_id: format!("req-{i}"),
            route: "/test".into(),
            kind: None,
            started_at_unix_ms: i,
            started_at_run_us: None,
            finished_at_unix_ms: i + 1,
            finished_at_run_us: None,
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
            started_at_run_us: None,
            finished_at_unix_ms: 2,
            finished_at_run_us: None,
            latency_us: 3_900_000,
            success: true,
        })
        .collect();
    run.runtime_snapshots = vec![runtime_snapshot(Some(1), Some(1), Some(240)); 80];

    let downstream_score_for = |margin: u8| {
        let options = AnalyzeOptions::default()
            .with_downstream(|o| o.blocking_correlation_score_margin = margin);
        let report = analyze_run(&run, options);
        report
            .secondary_suspects
            .iter()
            .find(|s| s.kind == DiagnosisKind::DownstreamStageDominates)
            .map(|s| s.score)
            .expect("downstream suspect should be present")
    };

    let no_margin_score = downstream_score_for(0);
    let large_margin_score = downstream_score_for(10);
    assert!(large_margin_score < no_margin_score);
}

#[test]
fn non_default_overrides_are_sorted_and_include_downstream_margin_override() {
    let options = AnalyzeOptions::default()
        .with_temporal(|o| o.min_request_count = 25)
        .with_downstream(|o| o.blocking_correlation_score_margin = 7)
        .with_queueing(|o| o.trigger_permille = 250);
    let overrides = options.non_default_overrides();
    let paths = overrides
        .iter()
        .map(|o| o.path.as_str())
        .collect::<Vec<_>>();
    let mut sorted = paths.clone();
    sorted.sort_unstable();
    assert_eq!(paths, sorted);
    assert!(overrides
        .iter()
        .any(|o| { o.path == "downstream.blocking_correlation_score_margin" && o.value == "7" }));
}

#[test]
fn truncation_warnings_remain_additive() {
    let mut run = test_run();
    run.truncation.dropped_requests = 1;
    run.truncation.dropped_stages = 1;
    run.truncation.dropped_runtime_snapshots = 1;
    let report = analyze_run(&run, AnalyzeOptions::default());
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
    let report = analyze_run(&test_run(), AnalyzeOptions::default());
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
    let report = analyze_run(&run, AnalyzeOptions::default());
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
            started_at_run_us: None,
            finished_at_unix_ms: i + 1,
            finished_at_run_us: None,
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
            waited_from_run_us: None,
            waited_until_unix_ms: 2,
            waited_until_run_us: None,
            depth_at_start: Some(2),
        })
        .collect();
    run.runtime_snapshots = vec![runtime_snapshot(Some(1), None, Some(1)); 10];
    let report = analyze_run(&run, AnalyzeOptions::default());
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
            started_at_run_us: None,
            finished_at_unix_ms: i + 1,
            finished_at_run_us: None,
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
            waited_from_run_us: None,
            waited_until_unix_ms: 2,
            waited_until_run_us: None,
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
            started_at_run_us: None,
            finished_at_unix_ms: 2,
            finished_at_run_us: None,
            latency_us: 400,
            success: true,
        })
        .collect();
    let report = analyze_run(&run, AnalyzeOptions::default());
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
            started_at_run_us: None,
            finished_at_unix_ms: i + 1,
            finished_at_run_us: None,
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
            waited_from_run_us: None,
            waited_until_unix_ms: 2,
            waited_until_run_us: None,
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
            started_at_run_us: None,
            finished_at_unix_ms: 2,
            finished_at_run_us: None,
            latency_us: 400,
            success: true,
        })
        .collect();
    run.truncation.dropped_queues = 2;

    let report = analyze_run(&run, AnalyzeOptions::default());
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
            started_at_run_us: None,
            finished_at_unix_ms: i + 1,
            finished_at_run_us: None,
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
            waited_from_run_us: None,
            waited_until_unix_ms: 2,
            waited_until_run_us: None,
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
            started_at_run_us: None,
            finished_at_unix_ms: 2,
            finished_at_run_us: None,
            latency_us: 800,
            success: true,
        })
        .collect();
    run.truncation.dropped_requests = 1;
    let report = analyze_run(&run, AnalyzeOptions::default());
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
            started_at_run_us: None,
            finished_at_unix_ms: i + 1,
            finished_at_run_us: None,
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
            waited_from_run_us: None,
            waited_until_unix_ms: 2,
            waited_until_run_us: None,
            wait_us: 990,
            depth_at_start: Some(18),
        })
        .collect();
    let report = analyze_run(&run, AnalyzeOptions::default());
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
            started_at_run_us: None,
            finished_at_unix_ms: i + 1,
            finished_at_run_us: None,
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
            waited_from_run_us: None,
            waited_until_unix_ms: 2,
            waited_until_run_us: None,
            wait_us: 985,
            depth_at_start: Some(15),
        })
        .collect();
    run.inflight = vec![
        tailtriage_core::InFlightSnapshot {
            gauge: "http".into(),
            at_unix_ms: 1,
            at_run_us: None,
            count: 1,
        },
        tailtriage_core::InFlightSnapshot {
            gauge: "http".into(),
            at_unix_ms: 2,
            at_run_us: None,
            count: 10,
        },
    ];
    let report = analyze_run(&run, AnalyzeOptions::default());
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
            started_at_run_us: None,
            finished_at_unix_ms: i + 1,
            finished_at_run_us: None,
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
            waited_from_run_us: None,
            waited_until_unix_ms: 2,
            waited_until_run_us: None,
            wait_us: 990,
            depth_at_start: Some(15),
        })
        .collect();
    run.truncation.dropped_queues = 1;
    let report = analyze_run(&run, AnalyzeOptions::default());
    assert!(report
        .primary_suspect
        .confidence_notes
        .iter()
        .any(|n| n
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
    let eq = evidence::evidence_quality(&run, &AnalyzeOptions::default());
    let mut suspects = vec![Suspect::new(
        DiagnosisKind::ApplicationQueueSaturation,
        100,
        vec![],
        vec![],
    )];
    super::confidence::apply_evidence_aware_confidence_caps(
        &mut suspects,
        &run,
        &eq,
        &AnalyzeOptions::default(),
    );
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
            started_at_run_us: None,
            finished_at_unix_ms: i + 1,
            finished_at_run_us: None,
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
            started_at_run_us: None,
            finished_at_unix_ms: 10,
            finished_at_run_us: None,
            latency_us: 4_800,
            success: true,
        })
        .collect();
    run.truncation.dropped_stages = 1;
    let report = analyze_run(&run, AnalyzeOptions::default());
    assert!(report
        .primary_suspect
        .confidence_notes
        .iter()
        .any(|n| n
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
    let eq = evidence::evidence_quality(&run, &AnalyzeOptions::default());
    let mut suspects = vec![Suspect::new(
        DiagnosisKind::DownstreamStageDominates,
        100,
        vec![],
        vec![],
    )];
    super::confidence::apply_evidence_aware_confidence_caps(
        &mut suspects,
        &run,
        &eq,
        &AnalyzeOptions::default(),
    );
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
            at_run_us: None,
            alive_tasks: Some(1),
            global_queue_depth: Some(5),
            local_queue_depth: Some(2),
            blocking_queue_depth: None,
            remote_schedule_count: Some(0),
        })
        .collect();
    let eq = evidence::evidence_quality(&run, &AnalyzeOptions::default());
    let mut suspects = vec![Suspect::new(
        DiagnosisKind::BlockingPoolPressure,
        100,
        vec![],
        vec![],
    )];
    super::confidence::apply_evidence_aware_confidence_caps(
        &mut suspects,
        &run,
        &eq,
        &AnalyzeOptions::default(),
    );
    assert_eq!(suspects[0].confidence, Confidence::Medium);
    assert!(suspects[0]
            .confidence_notes
            .iter()
            .any(|n| n == "Runtime snapshots are partial; missing runtime queue-depth fields limit executor/blocking confidence."));
    assert!(!suspects[0]
        .confidence_notes
        .iter()
        .any(|n| n == "Missing runtime snapshots limit executor/blocking confidence."));
}

#[test]
fn missing_runtime_snapshots_use_missing_runtime_note() {
    let mut run = test_run();
    run.requests = vec![sample_request(1)];
    run.runtime_snapshots.clear();
    let eq = evidence::evidence_quality(&run, &AnalyzeOptions::default());
    let mut suspects = vec![Suspect::new(
        DiagnosisKind::ExecutorPressureSuspected,
        100,
        vec![],
        vec![],
    )];
    super::confidence::apply_evidence_aware_confidence_caps(
        &mut suspects,
        &run,
        &eq,
        &AnalyzeOptions::default(),
    );
    assert!(suspects[0]
        .confidence_notes
        .iter()
        .any(|n| n == "Missing runtime snapshots limit executor/blocking confidence."));
}

#[test]
fn ambiguity_cap_adds_note_to_close_top_suspects() {
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
    let eq = evidence::evidence_quality(&run, &AnalyzeOptions::default());
    super::confidence::apply_evidence_aware_confidence_caps(
        &mut suspects,
        &run,
        &eq,
        &AnalyzeOptions::default(),
    );
    assert_eq!(suspects[0].confidence, Confidence::Medium);
    assert_eq!(suspects[1].confidence, Confidence::Medium);
    assert!(suspects[0]
        .confidence_notes
        .iter()
        .any(|n| n == "Top suspects are close in score; confidence is capped by ambiguity."));
    assert!(suspects[1]
        .confidence_notes
        .iter()
        .any(|n| n == "Top suspects are close in score; confidence is capped by ambiguity."));
}

#[test]
fn ambiguity_capping_preserves_order_and_scores() {
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
    let eq = evidence::evidence_quality(&run, &AnalyzeOptions::default());
    super::confidence::apply_evidence_aware_confidence_caps(
        &mut suspects,
        &run,
        &eq,
        &AnalyzeOptions::default(),
    );

    assert_eq!(suspects[0].score, 100);
    assert_eq!(suspects[1].score, 100);
    assert_eq!(suspects[0].kind, DiagnosisKind::ApplicationQueueSaturation);
    assert_eq!(suspects[1].kind, DiagnosisKind::DownstreamStageDominates);
    assert!(suspects[0]
        .confidence_notes
        .iter()
        .any(|n| n == "Top suspects are close in score; confidence is capped by ambiguity."));
    assert!(suspects[1]
        .confidence_notes
        .iter()
        .any(|n| n == "Top suspects are close in score; confidence is capped by ambiguity."));
}

#[test]
fn non_ambiguous_clean_evidence_keeps_high_confidence() {
    let mut run = test_run();
    run.requests = (0..45)
        .map(|i| RequestEvent {
            request_id: format!("req-{i}"),
            route: "/q".into(),
            kind: None,
            started_at_unix_ms: i,
            started_at_run_us: None,
            finished_at_unix_ms: i + 1,
            finished_at_run_us: None,
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
            waited_from_run_us: None,
            waited_until_unix_ms: 2,
            waited_until_run_us: None,
            wait_us: 990,
            depth_at_start: Some(15),
        })
        .collect();
    let mut suspects = vec![
        Suspect::new(
            DiagnosisKind::ApplicationQueueSaturation,
            100,
            vec![],
            vec![],
        ),
        Suspect::new(DiagnosisKind::DownstreamStageDominates, 10, vec![], vec![]),
    ];
    suspects[0].confidence = Confidence::High;
    let eq = evidence::evidence_quality(&run, &AnalyzeOptions::default());
    super::confidence::apply_evidence_aware_confidence_caps(
        &mut suspects,
        &run,
        &eq,
        &AnalyzeOptions::default(),
    );
    assert_eq!(suspects[0].confidence, Confidence::High);
    assert!(suspects[0].confidence_notes.is_empty());
}

#[test]
fn route_breakdowns_empty_for_single_route() {
    let report = analyze_run(&test_run(), AnalyzeOptions::default());
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
    let report = analyze_run(&run, AnalyzeOptions::default());
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
            waited_from_run_us: None,
            waited_until_unix_ms: 1,
            waited_until_run_us: None,
            depth_at_start: Some(9),
        });
    }
    for req_id in ["req-5", "req-6", "req-7"] {
        run.stages.push(StageEvent {
            request_id: req_id.to_owned(),
            stage: "db".into(),
            started_at_unix_ms: 1,
            started_at_run_us: None,
            finished_at_unix_ms: 2,
            finished_at_run_us: None,
            latency_us: 1_900,
            success: true,
        });
    }
    run.runtime_snapshots = vec![runtime_snapshot(Some(200), Some(140), Some(180))];
    let report = analyze_run(&run, AnalyzeOptions::default());
    assert_eq!(report.route_breakdowns.len(), 2);
    assert_eq!(report.route_breakdowns[0].route, "/a");
    assert_eq!(report.route_breakdowns[1].route, "/b");
    assert_eq!(
        report.route_breakdowns[0].primary_suspect.kind,
        DiagnosisKind::ApplicationQueueSaturation
    );
    assert_eq!(
        report.route_breakdowns[1].primary_suspect.kind,
        DiagnosisKind::DownstreamStageDominates
    );
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
fn route_divergence_warning_respects_emit_toggle_even_when_breakdowns_emit_from_p95_disparity() {
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
    for req_id in ["req-1", "req-2", "req-3", "req-4"] {
        run.queues.push(QueueEvent {
            request_id: req_id.to_owned(),
            queue: "ingress".into(),
            wait_us: 9_000,
            waited_from_unix_ms: 0,
            waited_from_run_us: None,
            waited_until_unix_ms: 1,
            waited_until_run_us: None,
            depth_at_start: Some(9),
        });
    }
    for req_id in ["req-5", "req-6", "req-7"] {
        run.stages.push(StageEvent {
            request_id: req_id.to_owned(),
            stage: "db".into(),
            started_at_unix_ms: 1,
            started_at_run_us: None,
            finished_at_unix_ms: 2,
            finished_at_run_us: None,
            latency_us: 1_900,
            success: true,
        });
    }

    let default_report = analyze_run(&run, AnalyzeOptions::default());
    assert_eq!(default_report.route_breakdowns.len(), 2);
    assert!(default_report
        .warnings
        .iter()
        .any(|warning| warning == ROUTE_DIVERGENCE_WARNING));

    let mut options = AnalyzeOptions::default();
    options.route.emit_on_divergent_suspects = false;
    let toggled_report = analyze_run(&run, options);
    assert_eq!(toggled_report.route_breakdowns.len(), 2);
    assert!(toggled_report
        .warnings
        .iter()
        .all(|warning| warning != ROUTE_DIVERGENCE_WARNING));
}

#[test]
fn multi_route_same_primary_keeps_route_breakdowns_empty() {
    let mut run = test_run();
    run.requests.clear();
    run.queues.clear();
    run.stages.clear();
    for idx in 1..=3 {
        let mut req = sample_request(idx);
        req.route = "/a".into();
        req.latency_us = 8_000;
        run.requests.push(req);
    }
    for idx in 4..=6 {
        let mut req = sample_request(idx);
        req.route = "/b".into();
        req.latency_us = 8_500;
        run.requests.push(req);
    }
    for req_id in ["req-1", "req-2", "req-3", "req-4", "req-5", "req-6"] {
        run.queues.push(QueueEvent {
            request_id: req_id.to_owned(),
            queue: "ingress".into(),
            wait_us: 7_400,
            waited_from_unix_ms: 0,
            waited_from_run_us: None,
            waited_until_unix_ms: 1,
            waited_until_run_us: None,
            depth_at_start: Some(7),
        });
    }
    let report = analyze_run(&run, AnalyzeOptions::default());
    assert!(report.route_breakdowns.is_empty());
    assert!(report
        .warnings
        .iter()
        .all(|warning| warning != ROUTE_DIVERGENCE_WARNING));
}

#[test]
fn route_breakdowns_do_not_change_global_primary_suspect() {
    let mut run = test_run();
    run.runtime_snapshots = vec![runtime_snapshot(Some(300), Some(250), Some(200))];
    let global = analyze_run_internal(&run, &AnalyzeOptions::default());
    let report = analyze_run(&run, AnalyzeOptions::default());
    assert_eq!(report.primary_suspect.kind, global.primary_suspect.kind);
    assert_eq!(report.primary_suspect.score, global.primary_suspect.score);
}

#[test]
fn temporal_segments_present_and_empty_below_threshold() {
    let report = analyze_run(&test_run(), AnalyzeOptions::default());
    let value = serde_json::to_value(&report).expect("serialize");
    assert!(value.get("temporal_segments").is_some());
    assert!(report.temporal_segments.is_empty());
}

#[test]
fn temporal_segment_window_uses_max_finish_timestamp() {
    let mut run = test_run();
    run.requests = (0..20).map(|i| sample_request(i + 1)).collect();
    run.requests[9].finished_at_unix_ms = 1000;
    run.requests[9].started_at_unix_ms = 10;
    run.requests[10].started_at_unix_ms = 11;
    run.requests[10].finished_at_unix_ms = 12;
    let early_ids: Vec<String> = run
        .requests
        .iter()
        .take(10)
        .map(|r| r.request_id.clone())
        .collect();
    for id in &early_ids {
        run.queues.push(QueueEvent {
            request_id: id.clone(),
            queue: "q".into(),
            wait_us: 900,
            waited_from_unix_ms: 1,
            waited_from_run_us: None,
            waited_until_unix_ms: 2,
            waited_until_run_us: None,
            depth_at_start: Some(9),
        });
    }
    let report = analyze_run(&run, AnalyzeOptions::default());
    assert_eq!(report.temporal_segments.len(), 2);
    let early = report
        .temporal_segments
        .iter()
        .find(|s| s.name == "early")
        .expect("early temporal segment should be emitted");
    assert_eq!(early.finished_at_unix_ms, Some(1000));
}

#[test]
fn temporal_sort_prefers_run_relative_start_when_unix_starts_match() {
    let mut run = test_run();
    run.requests = (1..=20).map(sample_request).collect();

    for request in &mut run.requests {
        let id = request
            .request_id
            .strip_prefix("req-")
            .expect("test request id should use req- prefix")
            .parse::<u64>()
            .expect("test request id should end with an integer");
        request.started_at_unix_ms = 100;
        request.finished_at_unix_ms = 101;
        request.started_at_run_us = Some((21 - id) * 1_000);
        request.finished_at_run_us = Some((21 - id) * 1_000 + 100);
    }

    for id in 11..=20 {
        run.queues.push(QueueEvent {
            request_id: format!("req-{id}"),
            queue: "ingress".into(),
            wait_us: 900,
            waited_from_unix_ms: 100,
            waited_from_run_us: None,
            waited_until_unix_ms: 101,
            waited_until_run_us: None,
            depth_at_start: Some(9),
        });
    }
    for id in 1..=10 {
        run.stages.push(StageEvent {
            request_id: format!("req-{id}"),
            stage: "db".into(),
            started_at_unix_ms: 100,
            started_at_run_us: None,
            finished_at_unix_ms: 101,
            finished_at_run_us: None,
            latency_us: 5_000,
            success: true,
        });
    }

    let report = analyze_run(&run, AnalyzeOptions::default());

    assert_eq!(report.temporal_segments.len(), 2);
    let early = report
        .temporal_segments
        .iter()
        .find(|segment| segment.name == "early")
        .expect("early temporal segment should be emitted");
    let late = report
        .temporal_segments
        .iter()
        .find(|segment| segment.name == "late")
        .expect("late temporal segment should be emitted");
    assert_eq!(
        early.primary_suspect.kind,
        DiagnosisKind::ApplicationQueueSaturation
    );
    assert_eq!(
        late.primary_suspect.kind,
        DiagnosisKind::DownstreamStageDominates
    );
}

#[test]
fn temporal_runtime_and_inflight_filtering_uses_run_relative_times() {
    let mut run = test_run();
    run.requests = (1..=20).map(sample_request).collect();

    for (idx, request) in run.requests.iter_mut().enumerate() {
        let idx = u64::try_from(idx).expect("test index should fit in u64");
        request.started_at_unix_ms = 1;
        request.finished_at_unix_ms = 1;
        if idx < 10 {
            request.started_at_run_us = Some(1_000 + idx * 100);
            request.finished_at_run_us = Some(1_100 + idx * 100);
        } else {
            request.started_at_run_us = Some(10_000 + idx * 100);
            request.finished_at_run_us = Some(16_000 + idx * 100);
            request.latency_us = 6_000;
        }
    }

    run.runtime_snapshots = vec![
        RuntimeSnapshot {
            at_unix_ms: 1,
            at_run_us: Some(1_200),
            global_queue_depth: Some(50),
            local_queue_depth: Some(50),
            alive_tasks: Some(100),
            blocking_queue_depth: Some(0),
            remote_schedule_count: None,
        },
        RuntimeSnapshot {
            at_unix_ms: 1,
            at_run_us: Some(11_200),
            global_queue_depth: Some(1),
            local_queue_depth: Some(1),
            alive_tasks: Some(100),
            blocking_queue_depth: Some(0),
            remote_schedule_count: None,
        },
    ];
    run.inflight = vec![
        tailtriage_core::InFlightSnapshot {
            at_unix_ms: 1,
            at_run_us: Some(1_200),
            gauge: "http.server.requests".into(),
            count: 2,
        },
        tailtriage_core::InFlightSnapshot {
            at_unix_ms: 1,
            at_run_us: Some(11_200),
            gauge: "http.server.requests".into(),
            count: 9,
        },
    ];

    let report = analyze_run(&run, AnalyzeOptions::default());

    assert_eq!(report.temporal_segments.len(), 2);
    let early = report
        .temporal_segments
        .iter()
        .find(|segment| segment.name == "early")
        .expect("early temporal segment should be emitted");
    let late = report
        .temporal_segments
        .iter()
        .find(|segment| segment.name == "late")
        .expect("late temporal segment should be emitted");

    assert_eq!(early.evidence_quality.runtime_snapshot_count, 1);
    assert_eq!(early.evidence_quality.inflight_snapshot_count, 1);
    assert_eq!(late.evidence_quality.runtime_snapshot_count, 1);
    assert_eq!(late.evidence_quality.inflight_snapshot_count, 1);
    assert!(early.p95_latency_us < late.p95_latency_us);
    for segment in &report.temporal_segments {
        assert!(!segment.warnings.iter().any(|warning| warning
            == "Temporal segment used wall-clock timestamp fallback; attribution is approximate for artifacts without complete run-relative timing."));
    }
}

#[test]
fn temporal_runtime_and_inflight_mixed_clock_snapshots_fall_back_per_sample() {
    let mut run = test_run();
    run.requests = (1..=20).map(sample_request).collect();

    for (idx, request) in run.requests.iter_mut().enumerate() {
        let idx = u64::try_from(idx + 1).expect("test index should fit in u64");
        request.started_at_run_us = Some(idx * 10_000);
        request.finished_at_run_us = Some(idx * 10_000 + 1_000);
        if idx > 10 {
            request.latency_us = 6_000;
            request.finished_at_run_us = Some(idx * 10_000 + 6_000);
        }
    }

    run.runtime_snapshots = vec![
        RuntimeSnapshot {
            at_unix_ms: 5,
            at_run_us: None,
            global_queue_depth: Some(50),
            local_queue_depth: Some(50),
            alive_tasks: Some(100),
            blocking_queue_depth: Some(0),
            remote_schedule_count: None,
        },
        RuntimeSnapshot {
            at_unix_ms: 15,
            at_run_us: None,
            global_queue_depth: Some(1),
            local_queue_depth: Some(1),
            alive_tasks: Some(100),
            blocking_queue_depth: Some(0),
            remote_schedule_count: None,
        },
        RuntimeSnapshot {
            at_unix_ms: 5,
            at_run_us: Some(150_000),
            global_queue_depth: Some(1),
            local_queue_depth: Some(1),
            alive_tasks: Some(100),
            blocking_queue_depth: Some(0),
            remote_schedule_count: None,
        },
    ];
    run.inflight = vec![
        tailtriage_core::InFlightSnapshot {
            at_unix_ms: 5,
            at_run_us: None,
            gauge: "http.server.requests".into(),
            count: 2,
        },
        tailtriage_core::InFlightSnapshot {
            at_unix_ms: 15,
            at_run_us: None,
            gauge: "http.server.requests".into(),
            count: 9,
        },
        tailtriage_core::InFlightSnapshot {
            at_unix_ms: 5,
            at_run_us: Some(150_000),
            gauge: "http.server.requests".into(),
            count: 9,
        },
    ];

    let report = analyze_run(&run, AnalyzeOptions::default());

    assert_eq!(report.temporal_segments.len(), 2);
    let early = report
        .temporal_segments
        .iter()
        .find(|segment| segment.name == "early")
        .expect("early temporal segment should be emitted");
    let late = report
        .temporal_segments
        .iter()
        .find(|segment| segment.name == "late")
        .expect("late temporal segment should be emitted");

    assert_eq!(early.evidence_quality.runtime_snapshot_count, 1);
    assert_eq!(early.evidence_quality.inflight_snapshot_count, 1);
    assert_eq!(late.evidence_quality.runtime_snapshot_count, 2);
    assert_eq!(late.evidence_quality.inflight_snapshot_count, 2);
    for segment in &report.temporal_segments {
        assert!(segment
            .warnings
            .iter()
            .any(|warning| warning == TEMPORAL_WALL_CLOCK_FALLBACK_WARNING));
    }
}

#[test]
fn temporal_segments_fallback_for_older_artifacts_warns() {
    let mut run = test_run();
    run.requests = (1..=20).map(sample_request).collect();
    for request in run.requests.iter_mut().skip(10) {
        request.latency_us = 6_000;
    }
    run.runtime_snapshots = vec![
        RuntimeSnapshot {
            at_unix_ms: 2,
            at_run_us: None,
            global_queue_depth: Some(50),
            local_queue_depth: Some(50),
            alive_tasks: Some(100),
            blocking_queue_depth: Some(0),
            remote_schedule_count: None,
        },
        RuntimeSnapshot {
            at_unix_ms: 12,
            at_run_us: None,
            global_queue_depth: Some(1),
            local_queue_depth: Some(1),
            alive_tasks: Some(100),
            blocking_queue_depth: Some(0),
            remote_schedule_count: None,
        },
    ];
    run.inflight = vec![
        tailtriage_core::InFlightSnapshot {
            at_unix_ms: 2,
            at_run_us: None,
            gauge: "http.server.requests".into(),
            count: 2,
        },
        tailtriage_core::InFlightSnapshot {
            at_unix_ms: 12,
            at_run_us: None,
            gauge: "http.server.requests".into(),
            count: 9,
        },
    ];

    let report = analyze_run(&run, AnalyzeOptions::default());

    assert_eq!(report.request_count, 20);
    assert_eq!(report.temporal_segments.len(), 2);
    for segment in &report.temporal_segments {
        assert!(segment.warnings.iter().any(|warning| warning
            == "Temporal segment used wall-clock timestamp fallback; attribution is approximate for artifacts without complete run-relative timing."));
    }
}

#[test]
fn temporal_segments_with_complete_run_relative_fields_do_not_warn_about_fallback() {
    let mut run = test_run();
    run.requests = (1..=20).map(sample_request).collect();
    for (idx, request) in run.requests.iter_mut().enumerate() {
        let idx = u64::try_from(idx).expect("test index should fit in u64");
        request.started_at_run_us = Some(idx * 1_000);
        request.finished_at_run_us = Some(idx * 1_000 + 1_000);
        if idx >= 10 {
            request.latency_us = 6_000;
            request.finished_at_run_us = Some(idx * 1_000 + 6_000);
        }
    }

    let report = analyze_run(&run, AnalyzeOptions::default());

    assert_eq!(report.temporal_segments.len(), 2);
    for segment in &report.temporal_segments {
        assert!(!segment.warnings.iter().any(|warning| warning
            == "Temporal segment used wall-clock timestamp fallback; attribution is approximate for artifacts without complete run-relative timing."));
    }
}

#[test]
fn temporal_segments_sort_complete_run_relative_starts_by_run_time() {
    let mut run = test_run();
    run.requests = (0..20)
        .map(|idx| RequestEvent {
            request_id: format!("req-{idx:02}"),
            route: "/t".into(),
            kind: None,
            started_at_unix_ms: 1,
            started_at_run_us: Some((19 - idx) * 1_000),
            finished_at_unix_ms: 1,
            finished_at_run_us: Some((19 - idx) * 1_000 + if idx >= 10 { 1_000 } else { 6_000 }),
            latency_us: if idx >= 10 { 1_000 } else { 6_000 },
            outcome: "ok".into(),
        })
        .collect();

    let report = analyze_run(&run, AnalyzeOptions::default());

    assert_eq!(report.temporal_segments.len(), 2);
    assert_eq!(report.temporal_segments[0].name, "early");
    assert_eq!(report.temporal_segments[1].name, "late");
    assert_eq!(report.temporal_segments[0].p95_latency_us, Some(1_000));
    assert_eq!(report.temporal_segments[1].p95_latency_us, Some(6_000));
}

#[test]
fn temporal_segments_sort_partial_run_relative_starts_by_unix_time() {
    let mut run = test_run();
    run.requests = (0..20)
        .map(|idx| RequestEvent {
            request_id: format!("req-{idx:02}"),
            route: "/t".into(),
            kind: None,
            started_at_unix_ms: idx + 1,
            started_at_run_us: if idx >= 10 {
                Some((19 - idx) * 1_000)
            } else {
                None
            },
            finished_at_unix_ms: idx + 2,
            finished_at_run_us: if idx >= 10 {
                Some((19 - idx) * 1_000 + 100)
            } else {
                None
            },
            latency_us: if idx < 10 { 1_000 } else { 6_000 },
            outcome: "ok".into(),
        })
        .collect();

    let report = analyze_run(&run, AnalyzeOptions::default());

    assert_eq!(report.temporal_segments.len(), 2);
    assert_eq!(report.temporal_segments[0].name, "early");
    assert_eq!(report.temporal_segments[1].name, "late");
    assert_eq!(report.temporal_segments[0].p95_latency_us, Some(1_000));
    assert_eq!(report.temporal_segments[1].p95_latency_us, Some(6_000));
}

#[test]
fn temporal_segments_not_emitted_when_no_meaningful_difference() {
    let mut run = test_run();
    run.requests = (0..20).map(|i| sample_request(i + 1)).collect();
    let report = analyze_run(&run, AnalyzeOptions::default());
    assert!(report.temporal_segments.is_empty());
    assert!(!report
        .warnings
        .iter()
        .any(|w| w == TEMPORAL_SUSPECT_SHIFT_WARNING));
}

#[test]
fn temporal_segments_emitted_when_primary_suspects_differ() {
    let mut run = test_run();
    run.requests = (0..20).map(|i| sample_request(i + 1)).collect();
    for i in 1..=10 {
        run.queues.push(QueueEvent {
            request_id: format!("req-{i}"),
            queue: "q".into(),
            wait_us: 900,
            waited_from_unix_ms: i,
            waited_from_run_us: None,
            waited_until_unix_ms: i + 1,
            waited_until_run_us: None,
            depth_at_start: Some(9),
        });
    }
    for i in 11..=20 {
        run.stages.push(StageEvent {
            request_id: format!("req-{i}"),
            stage: "db".into(),
            started_at_unix_ms: i,
            started_at_run_us: None,
            finished_at_unix_ms: i + 1,
            finished_at_run_us: None,
            latency_us: 5_000,
            success: true,
        });
    }
    let report = analyze_run(&run, AnalyzeOptions::default());
    assert_eq!(report.temporal_segments.len(), 2);
    assert_ne!(
        report.temporal_segments[0].primary_suspect.kind,
        report.temporal_segments[1].primary_suspect.kind
    );
    assert!(report
        .warnings
        .iter()
        .any(|w| w == TEMPORAL_SUSPECT_SHIFT_WARNING));
    assert!(!report
        .warnings
        .iter()
        .any(|w| w == TEMPORAL_P95_SHIFT_WARNING));
}

#[test]
fn temporal_p95_shift_emits_segments_and_ignores_missing_or_zero_lower_p95() {
    let mut run = test_run();
    run.requests = (0..20).map(|i| sample_request(i + 1)).collect();
    for i in 10usize..20 {
        if let Some(req) = run.requests.get_mut(i) {
            req.latency_us = 5_000;
        }
    }
    let shifted = analyze_run(&run, AnalyzeOptions::default());
    assert_eq!(shifted.temporal_segments.len(), 2);
    assert!(shifted
        .warnings
        .iter()
        .any(|w| w == TEMPORAL_P95_SHIFT_WARNING));

    assert!(!has_material_p95_shift(
        Some(0),
        Some(5_000),
        &AnalyzeOptions::default()
    ));
    assert!(!has_material_p95_shift(
        None,
        Some(5_000),
        &AnalyzeOptions::default()
    ));
    assert!(!has_material_p95_shift(
        Some(10),
        None,
        &AnalyzeOptions::default()
    ));
}

#[test]
fn temporal_segments_do_not_change_global_primary_suspect_or_score() {
    let mut run = test_run();
    run.requests = (0..20).map(|i| sample_request(i + 1)).collect();
    for i in 1..=10 {
        run.queues.push(QueueEvent {
            request_id: format!("req-{i}"),
            queue: "q".into(),
            wait_us: 900,
            waited_from_unix_ms: i,
            waited_from_run_us: None,
            waited_until_unix_ms: i + 1,
            waited_until_run_us: None,
            depth_at_start: Some(9),
        });
    }
    let global = analyze_run_internal(&run, &AnalyzeOptions::default());
    let report = analyze_run(&run, AnalyzeOptions::default());
    assert_eq!(report.primary_suspect.kind, global.primary_suspect.kind);
    assert_eq!(report.primary_suspect.score, global.primary_suspect.score);
}

fn run_with_temporal_shift_and_run_relative_offsets() -> Run {
    let mut run = test_run();
    run.requests = (0..20)
        .map(|i| {
            let mut request = sample_request(i + 1);
            let id = i + 1;
            let start_run_us = id * 10_000;
            request.started_at_run_us = Some(start_run_us);
            request.finished_at_run_us = Some(start_run_us + 1_000);
            request
        })
        .collect();
    for i in 1..=10 {
        run.queues.push(QueueEvent {
            request_id: format!("req-{i}"),
            queue: "q".into(),
            wait_us: 2_000,
            waited_from_unix_ms: i,
            waited_from_run_us: Some(i * 10_000),
            waited_until_unix_ms: i + 1,
            waited_until_run_us: Some(i * 10_000 + 2_000),
            depth_at_start: Some(12),
        });
    }
    for i in 11usize..=20usize {
        if let Some(req) = run.requests.get_mut(i - 1) {
            req.latency_us = 8_000;
            req.finished_at_run_us = req.started_at_run_us.map(|start| start + 8_000);
        }
        let i_u64 = u64::try_from(i).expect("test index should fit in u64");
        run.stages.push(StageEvent {
            request_id: format!("req-{i}"),
            stage: "db".into(),
            started_at_unix_ms: i_u64,
            started_at_run_us: Some(i_u64 * 10_000),
            finished_at_unix_ms: i_u64 + 1,
            finished_at_run_us: Some(i_u64 * 10_000 + 7_000),
            latency_us: 7_000,
            success: true,
        });
    }
    run.runtime_snapshots = vec![
        RuntimeSnapshot {
            at_unix_ms: 5,
            at_run_us: Some(50_000),
            global_queue_depth: Some(1),
            local_queue_depth: Some(1),
            alive_tasks: Some(20),
            blocking_queue_depth: Some(0),
            remote_schedule_count: None,
        },
        RuntimeSnapshot {
            at_unix_ms: 15,
            at_run_us: Some(150_000),
            global_queue_depth: Some(1),
            local_queue_depth: Some(1),
            alive_tasks: Some(20),
            blocking_queue_depth: Some(0),
            remote_schedule_count: None,
        },
    ];
    run.inflight = vec![
        tailtriage_core::InFlightSnapshot {
            at_unix_ms: 5,
            at_run_us: Some(50_000),
            gauge: "http.server.requests".into(),
            count: 1,
        },
        tailtriage_core::InFlightSnapshot {
            at_unix_ms: 15,
            at_run_us: Some(150_000),
            gauge: "http.server.requests".into(),
            count: 1,
        },
    ];
    run
}

#[test]
fn temporal_segments_warn_only_when_run_relative_timing_is_incomplete() {
    let complete_report = analyze_run(
        &run_with_temporal_shift_and_run_relative_offsets(),
        AnalyzeOptions::default(),
    );
    assert_eq!(complete_report.temporal_segments.len(), 2);
    for segment in &complete_report.temporal_segments {
        assert!(!segment
            .warnings
            .iter()
            .any(|w| w == TEMPORAL_WALL_CLOCK_FALLBACK_WARNING));
    }

    let mut incomplete_run = run_with_temporal_shift_and_run_relative_offsets();
    for request in &mut incomplete_run.requests {
        request.started_at_run_us = None;
        request.finished_at_run_us = None;
    }
    let incomplete_report = analyze_run(&incomplete_run, AnalyzeOptions::default());
    assert_eq!(incomplete_report.temporal_segments.len(), 2);
    for segment in &incomplete_report.temporal_segments {
        assert!(segment
            .warnings
            .iter()
            .any(|w| w == TEMPORAL_WALL_CLOCK_FALLBACK_WARNING));
    }
}

#[test]
fn sparse_timestamp_filtered_runtime_inflight_alone_do_not_emit_temporal_segments() {
    let mut run = test_run();
    run.requests = (0..20).map(|i| sample_request(i + 1)).collect();
    run.runtime_snapshots = vec![RuntimeSnapshot {
        at_unix_ms: 1,
        at_run_us: None,
        global_queue_depth: Some(2),
        local_queue_depth: Some(1),
        alive_tasks: Some(5),
        blocking_queue_depth: Some(0),
        remote_schedule_count: None,
    }];
    run.inflight = vec![tailtriage_core::InFlightSnapshot {
        at_unix_ms: 1,
        at_run_us: None,
        gauge: "http.server.requests".into(),
        count: 1,
    }];
    let report = analyze_run(&run, AnalyzeOptions::default());
    assert!(report.temporal_segments.is_empty());
}

#[test]
fn queue_to_downstream_shift_emits_temporal_segments_when_runtime_samples_are_sparse() {
    let mut run = test_run();
    run.requests = (0..20).map(|i| sample_request(i + 1)).collect();
    for i in 1..=10 {
        run.queues.push(QueueEvent {
            request_id: format!("req-{i}"),
            queue: "q".into(),
            wait_us: 2_000,
            waited_from_unix_ms: i,
            waited_from_run_us: None,
            waited_until_unix_ms: i + 1,
            waited_until_run_us: None,
            depth_at_start: Some(12),
        });
    }
    for i in 11..=20 {
        run.stages.push(StageEvent {
            request_id: format!("req-{i}"),
            stage: "db".into(),
            started_at_unix_ms: i,
            started_at_run_us: None,
            finished_at_unix_ms: i + 1,
            finished_at_run_us: None,
            latency_us: 9_000,
            success: true,
        });
    }
    run.runtime_snapshots = vec![runtime_snapshot(Some(1), Some(1), Some(1))];
    run.inflight = vec![tailtriage_core::InFlightSnapshot {
        at_unix_ms: 1,
        at_run_us: None,
        gauge: "http.server.requests".into(),
        count: 1,
    }];

    let global = analyze_run_internal(&run, &AnalyzeOptions::default());
    let report = analyze_run(&run, AnalyzeOptions::default());

    assert_eq!(report.temporal_segments.len(), 2);
    assert_ne!(
        report.temporal_segments[0].primary_suspect.kind,
        report.temporal_segments[1].primary_suspect.kind
    );
    assert!(report
        .warnings
        .iter()
        .any(|w| w == TEMPORAL_SUSPECT_SHIFT_WARNING));
    assert_eq!(report.primary_suspect.kind, global.primary_suspect.kind);
    assert_eq!(report.primary_suspect.score, global.primary_suspect.score);
}

#[test]
fn temporal_segments_emit_both_global_warnings_when_p95_and_suspect_shift_apply() {
    let mut run = test_run();
    run.requests = (0..20).map(|i| sample_request(i + 1)).collect();
    for i in 1..=10 {
        run.queues.push(QueueEvent {
            request_id: format!("req-{i}"),
            queue: "q".into(),
            wait_us: 2_000,
            waited_from_unix_ms: i,
            waited_from_run_us: None,
            waited_until_unix_ms: i + 1,
            waited_until_run_us: None,
            depth_at_start: Some(12),
        });
    }
    for i in 11usize..=20usize {
        if let Some(req) = run.requests.get_mut(i - 1) {
            req.latency_us = 8_000;
        }
        let i_u64 = u64::try_from(i).expect("test index should fit in u64");
        run.stages.push(StageEvent {
            request_id: format!("req-{i}"),
            stage: "db".into(),
            started_at_unix_ms: i_u64,
            started_at_run_us: None,
            finished_at_unix_ms: i_u64 + 1,
            finished_at_run_us: None,
            latency_us: 9_000,
            success: true,
        });
    }
    let report = analyze_run(&run, AnalyzeOptions::default());
    assert!(report
        .warnings
        .iter()
        .any(|w| w == TEMPORAL_SUSPECT_SHIFT_WARNING));
    assert!(report
        .warnings
        .iter()
        .any(|w| w == TEMPORAL_P95_SHIFT_WARNING));
}

#[test]
fn overlapping_temporal_windows_warn_runtime_inflight_attribution_is_approximate() {
    let mut run = test_run();
    run.requests = (0..20).map(|i| sample_request(i + 1)).collect();
    run.requests[9].finished_at_unix_ms = 1_000;
    run.requests[10].started_at_unix_ms = 100;
    run.requests[10].finished_at_unix_ms = 101;
    for i in 10usize..20 {
        if let Some(req) = run.requests.get_mut(i) {
            req.latency_us = 5_000;
        }
    }
    run.runtime_snapshots = vec![runtime_snapshot(Some(2), Some(2), Some(2))];
    let report = analyze_run(&run, AnalyzeOptions::default());
    assert_eq!(report.temporal_segments.len(), 2);
    for segment in &report.temporal_segments {
        assert!(segment
            .warnings
            .iter()
            .any(|w| w == TEMPORAL_OVERLAP_ATTRIBUTION_WARNING));
    }
}

#[test]
fn non_overlapping_temporal_windows_do_not_add_overlap_warning() {
    let mut run = test_run();
    run.requests = (0..20).map(|i| sample_request(i + 1)).collect();
    run.requests[9].finished_at_unix_ms = 10;
    run.requests[10].started_at_unix_ms = 20;
    run.requests[10].finished_at_unix_ms = 21;
    for i in 10usize..20 {
        if let Some(req) = run.requests.get_mut(i) {
            req.latency_us = 5_000;
        }
    }
    run.runtime_snapshots = vec![runtime_snapshot(Some(2), Some(2), Some(2))];
    let report = analyze_run(&run, AnalyzeOptions::default());
    assert_eq!(report.temporal_segments.len(), 2);
    for segment in &report.temporal_segments {
        assert!(!segment
            .warnings
            .iter()
            .any(|w| w == TEMPORAL_OVERLAP_ATTRIBUTION_WARNING));
    }
}

#[test]
fn missing_late_finish_timestamp_does_not_add_overlap_warning() {
    let mut run = test_run();
    run.requests = (0..20).map(|i| sample_request(i + 1)).collect();
    run.requests[9].finished_at_unix_ms = 1_000;
    run.requests[10].started_at_unix_ms = 100;
    run.requests[10].finished_at_unix_ms = 101;
    for i in 10usize..20 {
        if let Some(req) = run.requests.get_mut(i) {
            req.latency_us = 5_000;
        }
    }
    run.runtime_snapshots = vec![runtime_snapshot(Some(2), Some(2), Some(2))];
    let mut report = analyze_run(&run, AnalyzeOptions::default());
    assert_eq!(report.temporal_segments.len(), 2);
    for segment in &mut report.temporal_segments {
        segment.warnings.clear();
    }
    report.temporal_segments[1].finished_at_unix_ms = None;
    let (early, late) = report.temporal_segments.split_at_mut(1);
    apply_temporal_overlap_attribution_warning(&mut early[0], &mut late[0]);
    for segment in &report.temporal_segments {
        assert!(!segment
            .warnings
            .iter()
            .any(|w| w == TEMPORAL_OVERLAP_ATTRIBUTION_WARNING));
    }
}

#[test]
fn public_api_supports_report_text_and_json_contract_fields() {
    let run = test_run();
    let report: Report = analyze_run(&run, AnalyzeOptions::default());
    let text = render_text(&report);
    assert!(!text.is_empty(), "rendered text should not be empty");

    let report_json =
        serde_json::to_string_pretty(&report).expect("report should serialize to json");
    assert!(report_json.contains("\"evidence_quality\""));
    assert!(report_json.contains("\"confidence_notes\""));
    assert!(report_json.contains("\"route_breakdowns\""));
    assert!(report_json.contains("\"temporal_segments\""));
}

#[test]
fn render_json_pretty_matches_serde_json_pretty() {
    let report = analyze_run(&test_run(), AnalyzeOptions::default());
    let actual = render_json_pretty(&report).expect("report json should render");
    let expected = serde_json::to_string_pretty(&report).expect("report json should render");
    assert_eq!(actual, expected);
}

#[test]
fn render_json_matches_serde_json_compact() {
    let report = analyze_run(&test_run(), AnalyzeOptions::default());
    let actual = render_json(&report).expect("report json should render");
    let expected = serde_json::to_string(&report).expect("report json should render");
    assert_eq!(actual, expected);
}

#[test]
fn analyze_run_json_pretty_matches_analyze_then_render_json_pretty() {
    let run = test_run();
    let actual = analyze_run_json_pretty(&run, AnalyzeOptions::default())
        .expect("analyze+json should render");
    let expected_report = analyze_run(&run, AnalyzeOptions::default());
    let expected = render_json_pretty(&expected_report).expect("report json should render");
    assert_eq!(actual, expected);
}

#[test]
fn compact_and_pretty_report_json_are_value_equivalent() {
    let report = analyze_run(&test_run(), AnalyzeOptions::default());
    let compact = render_json(&report).expect("compact report json should render");
    let pretty = render_json_pretty(&report).expect("pretty report json should render");
    let compact_value: serde_json::Value =
        serde_json::from_str(&compact).expect("compact report json should parse");
    let pretty_value: serde_json::Value =
        serde_json::from_str(&pretty).expect("pretty report json should parse");
    assert_eq!(compact_value, pretty_value);
}

#[test]
fn analyze_options_defaults_match_v1_surface() {
    let options = AnalyzeOptions::default();
    assert_eq!(options.queueing.trigger_permille, 300);
    assert_eq!(options.blocking.min_nonzero_samples_for_signal, 2);
    assert_eq!(options.blocking.strong_p95_threshold, 12);
    assert_eq!(options.blocking.strong_peak_threshold, 20);
    assert_eq!(options.blocking.strong_nonzero_share_permille, 700);
    assert_eq!(options.blocking.strong_min_samples, 30);
    assert_eq!(options.executor.min_global_queue_p95_for_signal, 1);
    assert_eq!(options.downstream.min_stage_samples, 3);
    assert_eq!(
        options.downstream.blocking_correlated_stage_patterns,
        vec!["spawn_blocking", "blocking_path", "blocking"]
    );
    assert_eq!(options.downstream.blocking_correlation_score_margin, 2);
    assert_eq!(options.confidence.medium_score_threshold, 65);
    assert_eq!(options.confidence.high_score_threshold, 85);
    assert_eq!(options.confidence.ambiguity_min_score, 60);
    assert_eq!(options.confidence.ambiguity_score_gap, 4);
    assert_eq!(options.evidence.low_completed_request_threshold, 20);
    assert_eq!(options.route.min_request_count, 3);
    assert_eq!(options.route.breakdown_limit, 10);
    assert!(options.route.emit_on_divergent_suspects);
    assert_eq!(options.route.slowest_to_fastest_p95_ratio_numerator, 3);
    assert_eq!(options.route.slowest_to_fastest_p95_ratio_denominator, 2);
    assert_eq!(options.route.slowest_to_global_p95_ratio_numerator, 5);
    assert_eq!(options.route.slowest_to_global_p95_ratio_denominator, 4);
    assert_eq!(options.temporal.min_request_count, 20);
    assert_eq!(options.temporal.min_segment_request_count, 8);
    assert_eq!(options.temporal.share_shift_permille, 200);
    assert_eq!(options.temporal.p95_shift_ratio_numerator, 3);
    assert_eq!(options.temporal.p95_shift_ratio_denominator, 2);
    assert!(options.temporal.emit_on_suspect_shift);
    assert!(
        options
            .temporal
            .suppress_runtime_sparse_suspect_shift_without_supporting_movement
    );
}

#[test]
fn analyze_options_default_validates() {
    assert!(AnalyzeOptions::default().validate().is_ok());
}

#[test]
fn analyze_options_validate_rejects_invalid_classes() {
    assert!(AnalyzeOptions::default()
        .with_queueing(|o| o.trigger_permille = 1001)
        .validate()
        .is_err());
    assert!(AnalyzeOptions::default()
        .with_blocking(|o| o.strong_nonzero_share_permille = 1001)
        .validate()
        .is_err());
    assert!(AnalyzeOptions::default()
        .with_confidence(|o| {
            o.medium_score_threshold = 90;
            o.high_score_threshold = 80;
        })
        .validate()
        .is_err());
    assert!(AnalyzeOptions::default()
        .with_confidence(|o| o.high_score_threshold = 101)
        .validate()
        .is_err());
    assert!(AnalyzeOptions::default()
        .with_confidence(|o| o.ambiguity_min_score = 101)
        .validate()
        .is_err());
    assert!(AnalyzeOptions::default()
        .with_confidence(|o| o.ambiguity_score_gap = 101)
        .validate()
        .is_err());
    assert!(AnalyzeOptions::default()
        .with_downstream(|o| o.blocking_correlation_score_margin = 101)
        .validate()
        .is_err());
    assert!(AnalyzeOptions::default()
        .with_route(|o| o.breakdown_limit = 0)
        .validate()
        .is_err());
    assert!(AnalyzeOptions::default()
        .with_route(|o| o.slowest_to_fastest_p95_ratio_numerator = 0)
        .validate()
        .is_err());
    assert!(AnalyzeOptions::default()
        .with_route(|o| o.slowest_to_fastest_p95_ratio_numerator = 1)
        .with_route(|o| o.slowest_to_fastest_p95_ratio_denominator = 2)
        .validate()
        .is_err());
    assert!(AnalyzeOptions::default()
        .with_temporal(|o| o.min_segment_request_count = 0)
        .validate()
        .is_err());
    assert!(AnalyzeOptions::default()
        .with_temporal(|o| o.min_segment_request_count = 11)
        .validate()
        .is_err());
    assert!(AnalyzeOptions::default()
        .with_temporal(|o| o.share_shift_permille = 1001)
        .validate()
        .is_err());
    assert!(AnalyzeOptions::default()
        .with_temporal(|o| o.p95_shift_ratio_numerator = 0)
        .validate()
        .is_err());
    assert!(AnalyzeOptions::default()
        .with_temporal(|o| {
            o.p95_shift_ratio_numerator = 1;
            o.p95_shift_ratio_denominator = 2;
        })
        .validate()
        .is_err());
    assert!(AnalyzeOptions::default()
        .with_downstream(|o| o.blocking_correlated_stage_patterns = Vec::new())
        .validate()
        .is_err());
    assert!(AnalyzeOptions::default()
        .with_downstream(|o| o.blocking_correlated_stage_patterns = vec!["  ".to_string()])
        .validate()
        .is_err());
}

#[test]
fn validate_ratio_zero_denominators_report_exact_paths() {
    let err = AnalyzeOptions::default()
        .with_route(|o| o.slowest_to_fastest_p95_ratio_denominator = 0)
        .validate()
        .expect_err("fastest ratio denominator zero should fail");
    assert!(matches!(
        err,
        AnalyzeConfigError::InvalidConfigValue {
            path: "route.slowest_to_fastest_p95_ratio_denominator",
            ..
        }
    ));

    let err = AnalyzeOptions::default()
        .with_route(|o| o.slowest_to_global_p95_ratio_denominator = 0)
        .validate()
        .expect_err("global ratio denominator zero should fail");
    assert!(matches!(
        err,
        AnalyzeConfigError::InvalidConfigValue {
            path: "route.slowest_to_global_p95_ratio_denominator",
            ..
        }
    ));

    let err = AnalyzeOptions::default()
        .with_temporal(|o| o.p95_shift_ratio_denominator = 0)
        .validate()
        .expect_err("temporal p95 ratio denominator zero should fail");
    assert!(matches!(
        err,
        AnalyzeConfigError::InvalidConfigValue {
            path: "temporal.p95_shift_ratio_denominator",
            ..
        }
    ));
}

#[test]
fn try_analyze_run_rejects_invalid_options() {
    let run = test_run();
    let options = AnalyzeOptions::default().with_queueing(|o| o.trigger_permille = 1001);
    assert!(crate::try_analyze_run(&run, options).is_err());
}

#[test]
fn analyze_run_still_works_with_default_options() {
    let run = test_run();
    let report = analyze_run(&run, AnalyzeOptions::default());
    assert_eq!(report.request_count, 3);
}

#[test]
fn queueing_trigger_descriptor_direction_text_is_correct() {
    let descriptor = crate::analyze_option_descriptors()
        .iter()
        .find(|d| d.path == "queueing.trigger_permille")
        .expect("queueing.trigger_permille descriptor exists");
    assert!(descriptor
        .increasing
        .expect("increasing text")
        .contains("harder"));
    assert!(descriptor
        .decreasing
        .expect("decreasing text")
        .contains("easier"));
}

#[test]
fn descriptors_have_unique_and_exact_v1_paths() {
    let descriptors = crate::analyze_option_descriptors();
    let paths = descriptors
        .iter()
        .map(|d| d.path)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(paths.len(), descriptors.len());
    let expected = [
        "queueing.trigger_permille",
        "blocking.min_nonzero_samples_for_signal",
        "blocking.strong_p95_threshold",
        "blocking.strong_peak_threshold",
        "blocking.strong_nonzero_share_permille",
        "blocking.strong_min_samples",
        "executor.min_global_queue_p95_for_signal",
        "downstream.min_stage_samples",
        "downstream.blocking_correlated_stage_patterns",
        "downstream.blocking_correlation_score_margin",
        "confidence.medium_score_threshold",
        "confidence.high_score_threshold",
        "confidence.ambiguity_min_score",
        "confidence.ambiguity_score_gap",
        "evidence.low_completed_request_threshold",
        "route.min_request_count",
        "route.breakdown_limit",
        "route.emit_on_divergent_suspects",
        "route.slowest_to_fastest_p95_ratio_numerator",
        "route.slowest_to_fastest_p95_ratio_denominator",
        "route.slowest_to_global_p95_ratio_numerator",
        "route.slowest_to_global_p95_ratio_denominator",
        "temporal.min_request_count",
        "temporal.min_segment_request_count",
        "temporal.share_shift_permille",
        "temporal.p95_shift_ratio_numerator",
        "temporal.p95_shift_ratio_denominator",
        "temporal.emit_on_suspect_shift",
        "temporal.suppress_runtime_sparse_suspect_shift_without_supporting_movement",
    ]
    .into_iter()
    .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(paths, expected);
}

#[test]
#[allow(clippy::too_many_lines)]
fn descriptor_defaults_match_analyze_options_defaults() {
    let opts = AnalyzeOptions::default();
    let expected = std::collections::BTreeMap::from([
        (
            "queueing.trigger_permille",
            opts.queueing.trigger_permille.to_string(),
        ),
        (
            "blocking.min_nonzero_samples_for_signal",
            opts.blocking.min_nonzero_samples_for_signal.to_string(),
        ),
        (
            "blocking.strong_p95_threshold",
            opts.blocking.strong_p95_threshold.to_string(),
        ),
        (
            "blocking.strong_peak_threshold",
            opts.blocking.strong_peak_threshold.to_string(),
        ),
        (
            "blocking.strong_nonzero_share_permille",
            opts.blocking.strong_nonzero_share_permille.to_string(),
        ),
        (
            "blocking.strong_min_samples",
            opts.blocking.strong_min_samples.to_string(),
        ),
        (
            "executor.min_global_queue_p95_for_signal",
            opts.executor.min_global_queue_p95_for_signal.to_string(),
        ),
        (
            "downstream.min_stage_samples",
            opts.downstream.min_stage_samples.to_string(),
        ),
        (
            "downstream.blocking_correlated_stage_patterns",
            format!(
                "[\"{}\", \"{}\", \"{}\"]",
                opts.downstream.blocking_correlated_stage_patterns[0],
                opts.downstream.blocking_correlated_stage_patterns[1],
                opts.downstream.blocking_correlated_stage_patterns[2]
            ),
        ),
        (
            "downstream.blocking_correlation_score_margin",
            opts.downstream
                .blocking_correlation_score_margin
                .to_string(),
        ),
        (
            "confidence.medium_score_threshold",
            opts.confidence.medium_score_threshold.to_string(),
        ),
        (
            "confidence.high_score_threshold",
            opts.confidence.high_score_threshold.to_string(),
        ),
        (
            "confidence.ambiguity_min_score",
            opts.confidence.ambiguity_min_score.to_string(),
        ),
        (
            "confidence.ambiguity_score_gap",
            opts.confidence.ambiguity_score_gap.to_string(),
        ),
        (
            "evidence.low_completed_request_threshold",
            opts.evidence.low_completed_request_threshold.to_string(),
        ),
        (
            "route.min_request_count",
            opts.route.min_request_count.to_string(),
        ),
        (
            "route.breakdown_limit",
            opts.route.breakdown_limit.to_string(),
        ),
        (
            "route.emit_on_divergent_suspects",
            opts.route.emit_on_divergent_suspects.to_string(),
        ),
        (
            "route.slowest_to_fastest_p95_ratio_numerator",
            opts.route
                .slowest_to_fastest_p95_ratio_numerator
                .to_string(),
        ),
        (
            "route.slowest_to_fastest_p95_ratio_denominator",
            opts.route
                .slowest_to_fastest_p95_ratio_denominator
                .to_string(),
        ),
        (
            "route.slowest_to_global_p95_ratio_numerator",
            opts.route.slowest_to_global_p95_ratio_numerator.to_string(),
        ),
        (
            "route.slowest_to_global_p95_ratio_denominator",
            opts.route
                .slowest_to_global_p95_ratio_denominator
                .to_string(),
        ),
        (
            "temporal.min_request_count",
            opts.temporal.min_request_count.to_string(),
        ),
        (
            "temporal.min_segment_request_count",
            opts.temporal.min_segment_request_count.to_string(),
        ),
        (
            "temporal.share_shift_permille",
            opts.temporal.share_shift_permille.to_string(),
        ),
        (
            "temporal.p95_shift_ratio_numerator",
            opts.temporal.p95_shift_ratio_numerator.to_string(),
        ),
        (
            "temporal.p95_shift_ratio_denominator",
            opts.temporal.p95_shift_ratio_denominator.to_string(),
        ),
        (
            "temporal.emit_on_suspect_shift",
            opts.temporal.emit_on_suspect_shift.to_string(),
        ),
        (
            "temporal.suppress_runtime_sparse_suspect_shift_without_supporting_movement",
            opts.temporal
                .suppress_runtime_sparse_suspect_shift_without_supporting_movement
                .to_string(),
        ),
    ]);
    for descriptor in crate::analyze_option_descriptors() {
        assert_eq!(
            Some(&descriptor.default_value.to_string()),
            expected.get(descriptor.path)
        );
    }
}

fn assert_default_report_has_no_analyzer_config(report: &Report) {
    assert!(report.analyzer_config.is_none());
}

#[test]
fn default_options_compat_queue_saturation_case() {
    let mut run = test_run();
    run.queues = run
        .requests
        .iter()
        .map(|r| QueueEvent {
            request_id: r.request_id.clone(),
            queue: "q".into(),
            wait_us: 900,
            waited_from_unix_ms: 1,
            waited_from_run_us: None,
            waited_until_unix_ms: 2,
            waited_until_run_us: None,
            depth_at_start: Some(9),
        })
        .collect();
    let report = analyze_run(&run, AnalyzeOptions::default());
    assert_eq!(
        report.primary_suspect.kind,
        DiagnosisKind::ApplicationQueueSaturation
    );
    assert_default_report_has_no_analyzer_config(&report);
}

#[test]
fn default_options_compat_blocking_pool_pressure_case() {
    let mut run = test_run();
    run.requests = (0..40).map(sample_request).collect();
    run.stages = run
        .requests
        .iter()
        .map(|r| StageEvent {
            request_id: r.request_id.clone(),
            stage: "spawn_blocking_path".into(),
            started_at_unix_ms: 1,
            started_at_run_us: None,
            finished_at_unix_ms: 2,
            finished_at_run_us: None,
            latency_us: 3_900_000,
            success: true,
        })
        .collect();
    run.runtime_snapshots = vec![runtime_snapshot(Some(1), Some(1), Some(240)); 80];
    let report = analyze_run(&run, AnalyzeOptions::default());
    assert_eq!(
        report.primary_suspect.kind,
        DiagnosisKind::BlockingPoolPressure
    );
    assert_default_report_has_no_analyzer_config(&report);
}

#[test]
fn default_options_compat_insufficient_and_weak_evidence_case() {
    let report = analyze_run(&test_run(), AnalyzeOptions::default());
    assert_eq!(
        report.primary_suspect.kind,
        DiagnosisKind::InsufficientEvidence
    );
    assert!(report
        .warnings
        .iter()
        .any(|w| w.contains("Low completed-request count")));
    assert_eq!(report.evidence_quality.quality, EvidenceQualityLevel::Weak);
    assert_default_report_has_no_analyzer_config(&report);
}

#[test]
fn default_options_compat_downstream_stage_dominates_case() {
    let mut run = test_run();
    run.stages = run
        .requests
        .iter()
        .map(|r| StageEvent {
            request_id: r.request_id.clone(),
            stage: "db".into(),
            started_at_unix_ms: 1,
            started_at_run_us: None,
            finished_at_unix_ms: 2,
            finished_at_run_us: None,
            latency_us: 900,
            success: true,
        })
        .collect();
    run.runtime_snapshots = vec![runtime_snapshot(Some(2), Some(1), Some(1)); 5];
    let report = analyze_run(&run, AnalyzeOptions::default());
    assert_eq!(
        report.primary_suspect.kind,
        DiagnosisKind::DownstreamStageDominates
    );
    assert_default_report_has_no_analyzer_config(&report);
}

#[test]
fn default_options_compat_truncated_evidence_case() {
    let mut run = test_run();
    run.truncation.dropped_requests = 2;
    run.truncation.limits_hit = true;
    let report = analyze_run(&run, AnalyzeOptions::default());
    assert!(report
        .warnings
        .iter()
        .any(|w| w.contains("dropped evidence can reduce diagnosis completeness and confidence")));
    assert!(report.evidence_quality.truncated);
    assert_default_report_has_no_analyzer_config(&report);
}

#[test]
fn default_options_compat_ambiguous_top_suspects_case() {
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
    assert!(super::ambiguity_warning(&suspects, &AnalyzeOptions::default()).is_some());
}

#[test]
fn default_options_compat_route_breakdowns_case() {
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
    for req_id in ["req-1", "req-2", "req-3", "req-4"] {
        run.queues.push(QueueEvent {
            request_id: req_id.to_owned(),
            queue: "ingress".into(),
            wait_us: 9_000,
            waited_from_unix_ms: 0,
            waited_from_run_us: None,
            waited_until_unix_ms: 1,
            waited_until_run_us: None,
            depth_at_start: Some(9),
        });
    }
    for req_id in ["req-5", "req-6", "req-7"] {
        run.stages.push(StageEvent {
            request_id: req_id.to_owned(),
            stage: "db".into(),
            started_at_unix_ms: 1,
            started_at_run_us: None,
            finished_at_unix_ms: 2,
            finished_at_run_us: None,
            latency_us: 1_900,
            success: true,
        });
    }
    run.runtime_snapshots = vec![runtime_snapshot(Some(200), Some(140), Some(180))];
    let report = analyze_run(&run, AnalyzeOptions::default());
    assert!(!report.route_breakdowns.is_empty());
    assert!(report
        .warnings
        .iter()
        .any(|w| w == ROUTE_DIVERGENCE_WARNING));
    assert_default_report_has_no_analyzer_config(&report);
}

#[test]
fn default_options_compat_temporal_segments_case() {
    let mut run = test_run();
    run.requests = (0..40)
        .map(|i| RequestEvent {
            request_id: format!("req-{i}"),
            route: "/test".into(),
            kind: None,
            started_at_unix_ms: i,
            started_at_run_us: None,
            finished_at_unix_ms: i + 1,
            finished_at_run_us: None,
            latency_us: if i < 20 { 2_000 } else { 5_000 },
            outcome: "ok".into(),
        })
        .collect();
    run.queues = run
        .requests
        .iter()
        .enumerate()
        .map(|(i, r)| QueueEvent {
            request_id: r.request_id.clone(),
            queue: "q".into(),
            wait_us: if i < 20 { 1_500 } else { 100 },
            waited_from_unix_ms: 1,
            waited_from_run_us: None,
            waited_until_unix_ms: 2,
            waited_until_run_us: None,
            depth_at_start: Some(3),
        })
        .collect();
    run.stages = run
        .requests
        .iter()
        .enumerate()
        .map(|(i, r)| StageEvent {
            request_id: r.request_id.clone(),
            stage: "db".into(),
            started_at_unix_ms: 1,
            started_at_run_us: None,
            finished_at_unix_ms: 2,
            finished_at_run_us: None,
            latency_us: if i < 20 { 200 } else { 4_400 },
            success: true,
        })
        .collect();
    let report = analyze_run(&run, AnalyzeOptions::default());
    assert!(!report.temporal_segments.is_empty());
    assert_default_report_has_no_analyzer_config(&report);
}

#[test]
fn analyzer_config_transparency_default_report_omits_config() {
    let run = test_run();
    let report = analyze_run(&run, AnalyzeOptions::default());

    assert!(report.analyzer_config.is_none());

    let report_json = serde_json::to_value(&report).expect("serialize report");
    assert!(
        report_json.get("analyzer_config").is_none(),
        "default report JSON must not include analyzer_config"
    );

    let text = render_text(&report);
    assert!(
        !text.contains("Analyzer config:"),
        "default report text must not include analyzer config section"
    );
}

#[test]
fn analyzer_config_transparency_non_default_report_includes_config() {
    let run = test_run();
    let mut options = AnalyzeOptions::default();
    options.queueing.trigger_permille = 400;
    options.temporal.min_request_count = 30;

    let report = analyze_run(&run, options);
    let config = report
        .analyzer_config
        .as_ref()
        .expect("non-default options should surface analyzer_config");
    assert_eq!(config.schema_version, 1);
    assert_eq!(
        config.non_default_options.len(),
        2,
        "only explicitly changed options should be surfaced"
    );
    assert_eq!(
        config.non_default_options[0].path,
        "queueing.trigger_permille"
    );
    assert_eq!(config.non_default_options[0].value, "400");
    assert_eq!(
        config.non_default_options[1].path,
        "temporal.min_request_count"
    );
    assert_eq!(config.non_default_options[1].value, "30");

    let report_json = serde_json::to_value(&report).expect("serialize report");
    let json_overrides = report_json
        .get("analyzer_config")
        .and_then(|config| config.get("non_default_options"))
        .and_then(serde_json::Value::as_array)
        .expect("analyzer_config.non_default_options should be present");
    assert_eq!(json_overrides.len(), 2);
    assert_eq!(
        json_overrides[0]
            .get("path")
            .and_then(serde_json::Value::as_str),
        Some("queueing.trigger_permille")
    );
    assert_eq!(
        json_overrides[0]
            .get("value")
            .and_then(serde_json::Value::as_str),
        Some("400")
    );
    assert_eq!(
        json_overrides[1]
            .get("path")
            .and_then(serde_json::Value::as_str),
        Some("temporal.min_request_count")
    );
    assert_eq!(
        json_overrides[1]
            .get("value")
            .and_then(serde_json::Value::as_str),
        Some("30")
    );

    let text = render_text(&report);
    assert!(text.contains("Analyzer config:"));
    assert!(text.contains("- queueing.trigger_permille=400"));
    assert!(text.contains("- temporal.min_request_count=30"));
}

fn option_run_twenty_requests() -> Run {
    let mut run = test_run();
    run.requests = (0..20).map(|i| sample_request(i + 1)).collect();
    run
}

#[test]
fn option_queueing_trigger_permille_changes_queue_suspect() {
    let mut run = option_run_twenty_requests();
    for i in 1..=20 {
        run.queues.push(QueueEvent {
            request_id: format!("req-{i}"),
            queue: "q".into(),
            wait_us: 400,
            waited_from_unix_ms: i,
            waited_from_run_us: None,
            waited_until_unix_ms: i + 1,
            waited_until_run_us: None,
            depth_at_start: Some(3),
        });
    }
    let default_report = analyze_run(&run, AnalyzeOptions::default());
    assert_eq!(
        default_report.primary_suspect.kind,
        DiagnosisKind::ApplicationQueueSaturation
    );
    let strict = AnalyzeOptions::default().with_queueing(|o| o.trigger_permille = 600);
    let strict_report = analyze_run(&run, strict);
    assert_ne!(
        strict_report.primary_suspect.kind,
        DiagnosisKind::ApplicationQueueSaturation
    );
}

#[test]
fn option_blocking_min_nonzero_samples_changes_signal_emission() {
    let mut run = option_run_twenty_requests();
    run.runtime_snapshots = vec![runtime_snapshot(Some(0), Some(0), Some(0)); 100];
    run.runtime_snapshots[0].blocking_queue_depth = Some(1);
    let default_report = analyze_run(&run, AnalyzeOptions::default());
    assert_ne!(
        default_report.primary_suspect.kind,
        DiagnosisKind::BlockingPoolPressure
    );
    let relaxed = AnalyzeOptions::default().with_blocking(|o| o.min_nonzero_samples_for_signal = 1);
    let relaxed_report = analyze_run(&run, relaxed);
    assert_eq!(
        relaxed_report.primary_suspect.kind,
        DiagnosisKind::BlockingPoolPressure
    );
}

#[test]
fn option_executor_min_global_queue_p95_changes_signal_emission() {
    let mut run = option_run_twenty_requests();
    run.runtime_snapshots = vec![runtime_snapshot(Some(1), Some(0), Some(0)); 20];
    let default_report = analyze_run(&run, AnalyzeOptions::default());
    assert_eq!(
        default_report.primary_suspect.kind,
        DiagnosisKind::ExecutorPressureSuspected
    );
    let strict = AnalyzeOptions::default().with_executor(|o| o.min_global_queue_p95_for_signal = 2);
    let strict_report = analyze_run(&run, strict);
    assert_ne!(
        strict_report.primary_suspect.kind,
        DiagnosisKind::ExecutorPressureSuspected
    );
}

#[test]
fn option_confidence_high_score_threshold_changes_scoring_suspect_bucket() {
    let mut run = option_run_twenty_requests();
    for i in 1..=20 {
        run.queues.push(QueueEvent {
            request_id: format!("req-{i}"),
            queue: "q".into(),
            wait_us: 800,
            waited_from_unix_ms: i,
            waited_from_run_us: None,
            waited_until_unix_ms: i + 1,
            waited_until_run_us: None,
            depth_at_start: Some(12),
        });
    }

    let default_report = analyze_run(&run, AnalyzeOptions::default());
    assert_eq!(
        default_report.primary_suspect.kind,
        DiagnosisKind::ApplicationQueueSaturation
    );
    assert_eq!(default_report.primary_suspect.score, 90);
    assert_eq!(default_report.primary_suspect.confidence, Confidence::High);

    let strict = AnalyzeOptions::default().with_confidence(|o| o.high_score_threshold = 91);
    let strict_report = analyze_run(&run, strict);
    assert_eq!(
        strict_report.primary_suspect.kind,
        DiagnosisKind::ApplicationQueueSaturation
    );
    assert_eq!(strict_report.primary_suspect.score, 90);
    assert_eq!(strict_report.primary_suspect.confidence, Confidence::Medium);
}

#[test]
fn analyzer_toml_full_parses() {
    let input = include_str!("../../examples/analyzer-config.toml");
    let options = AnalyzeOptions::from_toml_str(input).expect("parse full analyzer toml");
    assert_eq!(options.queueing.trigger_permille, 400);
}

#[test]
fn analyzer_toml_sparse_preserves_defaults() {
    let input = "[analyzer]\nschema_version=1\n[analyzer.queueing]\ntrigger_permille=450\n";
    let options = AnalyzeOptions::from_toml_str(input).expect("parse sparse toml");
    assert_eq!(options.queueing.trigger_permille, 450);
    assert_eq!(options.blocking, AnalyzeOptions::default().blocking);
}

#[test]
fn analyzer_toml_merge_sparse_preserves_unrelated_non_default_base_values() {
    let base = AnalyzeOptions::default().with_blocking(|o| o.strong_p95_threshold = 99);
    let merged = base
        .merge_toml_str("[analyzer]\nschema_version=1\n[analyzer.queueing]\ntrigger_permille=410\n")
        .expect("merge");
    assert_eq!(merged.queueing.trigger_permille, 410);
    assert_eq!(merged.blocking.strong_p95_threshold, 99);
}

#[test]
fn analyzer_toml_missing_analyzer_fails() {
    assert!(matches!(
        AnalyzeOptions::from_toml_str("[other]\na=1\n"),
        Err(AnalyzeConfigError::MissingAnalyzerTable)
    ));
}
#[test]
fn analyzer_toml_root_level_queueing_group_is_rejected() {
    assert!(matches!(
        AnalyzeOptions::from_toml_str("[queueing]\ntrigger_permille=400\n"),
        Err(AnalyzeConfigError::MissingAnalyzerTable)
    ));
}

#[test]
fn analyzer_toml_missing_schema_fails() {
    assert!(matches!(
        AnalyzeOptions::from_toml_str("[analyzer]\n"),
        Err(AnalyzeConfigError::MissingSchemaVersion)
    ));
}
#[test]
fn analyzer_toml_unsupported_schema_fails() {
    assert!(matches!(
        AnalyzeOptions::from_toml_str("[analyzer]\nschema_version=2\n"),
        Err(AnalyzeConfigError::UnsupportedSchemaVersion {
            found: 2,
            supported: 1
        })
    ));
}
#[test]
fn analyzer_toml_unknown_top_level_sibling_ignored() {
    let input = "[controller]\nmode='light'\n[analyzer]\nschema_version=1\n";
    assert!(AnalyzeOptions::from_toml_str(input).is_ok());
}
#[test]
fn analyzer_toml_unknown_field_under_analyzer_fails() {
    assert!(matches!(
        AnalyzeOptions::from_toml_str("[analyzer]\nschema_version=1\nfoo=1\n"),
        Err(AnalyzeConfigError::InvalidToml { .. })
    ));
}
#[test]
fn analyzer_toml_unknown_subgroup_fails() {
    assert!(matches!(
        AnalyzeOptions::from_toml_str("[analyzer]\nschema_version=1\n[analyzer.unknown]\na=1\n"),
        Err(AnalyzeConfigError::InvalidToml { .. })
    ));
}
#[test]
fn analyzer_toml_unknown_field_in_known_subgroup_fails() {
    assert!(matches!(
        AnalyzeOptions::from_toml_str(
            "[analyzer]\nschema_version=1\n[analyzer.queueing]\nnope=1\n"
        ),
        Err(AnalyzeConfigError::InvalidToml { .. })
    ));
}
#[test]
fn analyzer_toml_invalid_type_fails() {
    assert!(matches!(
        AnalyzeOptions::from_toml_str(
            "[analyzer]\nschema_version=1\n[analyzer.queueing]\ntrigger_permille='bad'\n"
        ),
        Err(AnalyzeConfigError::InvalidToml { .. })
    ));
}
#[test]
fn analyzer_toml_invalid_range_fails_validation() {
    let err = AnalyzeOptions::from_toml_str(
        "[analyzer]\nschema_version=1\n[analyzer.queueing]\ntrigger_permille=1001\n",
    )
    .expect_err("invalid range");
    assert!(matches!(
        err,
        AnalyzeConfigError::InvalidConfigValue {
            path: "queueing.trigger_permille",
            ..
        }
    ));
}
#[test]
fn analyzer_toml_canonical_example_path_parses() {
    let _ = AnalyzeOptions::from_toml_str(include_str!("../../examples/analyzer-config.toml"))
        .expect("canonical repo-root example parse");
}

#[test]
fn analyzer_toml_example_file_has_v1_namespaced_groups_only() {
    let input = include_str!("../../examples/analyzer-config.toml");
    assert!(input.contains("[analyzer]"));
    assert!(input.contains("schema_version = 1"));
    for group in [
        "queueing",
        "blocking",
        "executor",
        "downstream",
        "confidence",
        "evidence",
        "route",
        "temporal",
    ] {
        assert!(input.contains(&format!("[analyzer.{group}]")));
        assert!(!input.contains(&format!("[{group}]")));
    }
}
#[test]
fn analyzer_toml_downstream_patterns_list_parses() {
    let input = "[analyzer]\nschema_version=1\n[analyzer.downstream]\nblocking_correlated_stage_patterns=['db','cache']\n";
    let opts = AnalyzeOptions::from_toml_str(input).expect("parse list");
    assert_eq!(
        opts.downstream.blocking_correlated_stage_patterns,
        vec!["db", "cache"]
    );
}
#[test]
fn analyzer_toml_empty_pattern_fails_validation() {
    let err = AnalyzeOptions::from_toml_str("[analyzer]\nschema_version=1\n[analyzer.downstream]\nblocking_correlated_stage_patterns=['']\n").expect_err("must fail");
    assert!(matches!(
        err,
        AnalyzeConfigError::InvalidConfigValue {
            path: "downstream.blocking_correlated_stage_patterns",
            ..
        }
    ));
}
