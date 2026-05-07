use tailtriage_core::{
    CaptureMode, EffectiveCoreConfig, QueueEvent, RequestEvent, Run, RunMetadata, RuntimeSnapshot,
    StageEvent, SCHEMA_VERSION,
};

use super::temporal::{
    apply_temporal_overlap_attribution_warning, has_material_p95_shift,
    TEMPORAL_OVERLAP_ATTRIBUTION_WARNING, TEMPORAL_P95_SHIFT_WARNING,
    TEMPORAL_SUSPECT_SHIFT_WARNING,
};
use crate::{
    analyze_run, analyze_run_internal, evidence, render_text, AnalyzeOptions, Confidence,
    DiagnosisKind, EvidenceQuality, EvidenceQualityLevel, InflightTrend, Report,
    SignalCoverageStatus, Suspect, ROUTE_DIVERGENCE_WARNING, ROUTE_RUNTIME_ATTRIBUTION_WARNING,
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
    let report = analyze_run(&run, AnalyzeOptions::default());
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
        "db_query"
    ));
    assert!(!super::scoring::stage_correlates_with_blocking_pool(
        "retry_attempt"
    ));
    assert!(super::scoring::stage_correlates_with_blocking_pool(
        "spawn_blocking_path"
    ));
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
    let eq = evidence::evidence_quality(&run);
    let mut suspects = vec![Suspect::new(
        DiagnosisKind::ApplicationQueueSaturation,
        100,
        vec![],
        vec![],
    )];
    super::confidence::apply_evidence_aware_confidence_caps(&mut suspects, &run, &eq);
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
    let eq = evidence::evidence_quality(&run);
    let mut suspects = vec![Suspect::new(
        DiagnosisKind::DownstreamStageDominates,
        100,
        vec![],
        vec![],
    )];
    super::confidence::apply_evidence_aware_confidence_caps(&mut suspects, &run, &eq);
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
    let eq = evidence::evidence_quality(&run);
    let mut suspects = vec![Suspect::new(
        DiagnosisKind::BlockingPoolPressure,
        100,
        vec![],
        vec![],
    )];
    super::confidence::apply_evidence_aware_confidence_caps(&mut suspects, &run, &eq);
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
    let eq = evidence::evidence_quality(&run);
    let mut suspects = vec![Suspect::new(
        DiagnosisKind::ExecutorPressureSuspected,
        100,
        vec![],
        vec![],
    )];
    super::confidence::apply_evidence_aware_confidence_caps(&mut suspects, &run, &eq);
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
    let eq = evidence::evidence_quality(&run);
    super::confidence::apply_evidence_aware_confidence_caps(&mut suspects, &run, &eq);
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
    let eq = evidence::evidence_quality(&run);
    super::confidence::apply_evidence_aware_confidence_caps(&mut suspects, &run, &eq);

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
    let eq = evidence::evidence_quality(&run);
    super::confidence::apply_evidence_aware_confidence_caps(&mut suspects, &run, &eq);
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
            waited_until_unix_ms: 1,
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
    let global = analyze_run_internal(&run);
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
            waited_until_unix_ms: 2,
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
            waited_until_unix_ms: i + 1,
            depth_at_start: Some(9),
        });
    }
    for i in 11..=20 {
        run.stages.push(StageEvent {
            request_id: format!("req-{i}"),
            stage: "db".into(),
            started_at_unix_ms: i,
            finished_at_unix_ms: i + 1,
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

    assert!(!has_material_p95_shift(Some(0), Some(5_000)));
    assert!(!has_material_p95_shift(None, Some(5_000)));
    assert!(!has_material_p95_shift(Some(10), None));
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
            waited_until_unix_ms: i + 1,
            depth_at_start: Some(9),
        });
    }
    let global = analyze_run_internal(&run);
    let report = analyze_run(&run, AnalyzeOptions::default());
    assert_eq!(report.primary_suspect.kind, global.primary_suspect.kind);
    assert_eq!(report.primary_suspect.score, global.primary_suspect.score);
}

#[test]
fn sparse_timestamp_filtered_runtime_inflight_alone_do_not_emit_temporal_segments() {
    let mut run = test_run();
    run.requests = (0..20).map(|i| sample_request(i + 1)).collect();
    run.runtime_snapshots = vec![RuntimeSnapshot {
        at_unix_ms: 1,
        global_queue_depth: Some(2),
        local_queue_depth: Some(1),
        alive_tasks: Some(5),
        blocking_queue_depth: Some(0),
        remote_schedule_count: None,
    }];
    run.inflight = vec![tailtriage_core::InFlightSnapshot {
        at_unix_ms: 1,
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
            waited_until_unix_ms: i + 1,
            depth_at_start: Some(12),
        });
    }
    for i in 11..=20 {
        run.stages.push(StageEvent {
            request_id: format!("req-{i}"),
            stage: "db".into(),
            started_at_unix_ms: i,
            finished_at_unix_ms: i + 1,
            latency_us: 9_000,
            success: true,
        });
    }
    run.runtime_snapshots = vec![runtime_snapshot(Some(1), Some(1), Some(1))];
    run.inflight = vec![tailtriage_core::InFlightSnapshot {
        at_unix_ms: 1,
        gauge: "http.server.requests".into(),
        count: 1,
    }];

    let global = analyze_run_internal(&run);
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
            waited_until_unix_ms: i + 1,
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
            finished_at_unix_ms: i_u64 + 1,
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
fn render_json_pretty_matches_serde_json() {
    let report = analyze_run(&test_run(), AnalyzeOptions::default());
    assert_eq!(
        crate::render_json_pretty(&report).expect("render_json_pretty should serialize"),
        serde_json::to_string_pretty(&report).expect("serde pretty serialization should succeed")
    );
}

#[test]
fn render_json_matches_serde_json() {
    let report = analyze_run(&test_run(), AnalyzeOptions::default());
    assert_eq!(
        crate::render_json(&report).expect("render_json should serialize"),
        serde_json::to_string(&report).expect("serde compact serialization should succeed")
    );
}

#[test]
fn analyze_run_json_pretty_matches_analyze_then_render() {
    let run = test_run();
    let expected = crate::render_json_pretty(&analyze_run(&run, AnalyzeOptions::default()))
        .expect("render_json_pretty should serialize");
    let actual = crate::analyze_run_json_pretty(&run, AnalyzeOptions::default())
        .expect("analyze_run_json_pretty should serialize");
    assert_eq!(actual, expected);
}

#[test]
fn compact_json_matches_pretty_json_value() {
    let report = analyze_run(&test_run(), AnalyzeOptions::default());
    let compact = crate::render_json(&report).expect("render_json should serialize");
    let pretty = crate::render_json_pretty(&report).expect("render_json_pretty should serialize");

    let compact_value: serde_json::Value =
        serde_json::from_str(&compact).expect("compact output should parse as valid JSON");
    let pretty_value: serde_json::Value =
        serde_json::from_str(&pretty).expect("pretty output should parse as valid JSON");

    assert_eq!(compact_value, pretty_value);
}
