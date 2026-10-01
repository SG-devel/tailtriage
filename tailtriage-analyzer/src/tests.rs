use tailtriage_core::{
    normalize_run_permissive, CaptureMode, EffectiveCoreConfig, InFlightSnapshot, QueueEvent,
    RequestEvent, Run, RunMetadata, RuntimeSnapshot, StageEvent, StageRelation, StageRelations,
    SCHEMA_VERSION,
};

use super::temporal::{
    apply_temporal_overlap_attribution_warning, has_material_p95_shift,
    TEMPORAL_OVERLAP_ATTRIBUTION_WARNING, TEMPORAL_P95_SHIFT_WARNING,
    TEMPORAL_SUSPECT_SHIFT_WARNING, TEMPORAL_WALL_CLOCK_FALLBACK_WARNING,
};
use crate::{
    analyze_run, analyze_run_internal, evidence, render_json, render_json_pretty, render_text,
    AnalyzeConfigError, AnalyzeOptions, Confidence, DiagnosisKind, EvidenceQuality,
    EvidenceQualityLevel, InflightTrend, RelatedEvidenceMeasurement, Report, SignalCoverageStatus,
    Suspect, ROUTE_DIVERGENCE_WARNING, ROUTE_RUNTIME_ATTRIBUTION_WARNING,
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

fn precise_stage(
    request_id: &str,
    stage: &str,
    start: Option<u64>,
    end: Option<u64>,
    latency_us: u64,
) -> StageEvent {
    StageEvent {
        request_id: request_id.to_owned(),
        stage: stage.to_owned(),
        relations: tailtriage_core::StageRelations::default(),
        started_at_unix_ms: 10,
        started_at_run_us: start,
        finished_at_unix_ms: 10,
        finished_at_run_us: end,
        latency_us,
        success: true,
        completed: true,
    }
}

fn downstream_suspect(report: &Report) -> &Suspect {
    std::iter::once(&report.primary_suspect)
        .chain(report.secondary_suspects.iter())
        .find(|suspect| suspect.kind == DiagnosisKind::DownstreamStageDominance)
        .expect("downstream suspect")
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
        completed: true,
    }
}

fn literal_scored(
    kind: DiagnosisKind,
    score: u8,
    confidence: Confidence,
) -> super::candidate::SupportedCandidate {
    let mut suspect = Suspect::new(
        kind,
        score,
        vec![format!("evidence-{score}")],
        vec![format!("check-{score}")],
    );
    suspect.confidence = confidence;
    super::candidate::SupportedCandidate {
        suspect,
        basis: super::partial_evidence::EvidenceBasis::Completed,
        executor_limitation: None,
        relevant_support: 20,
        blocking_measurement: None,
        downstream_measurement: None,
    }
}

fn finalized_literal_order(
    mut suspects: Vec<super::candidate::SupportedCandidate>,
) -> Vec<DiagnosisKind> {
    suspects.sort_by(super::final_suspect_order);
    suspects.into_iter().map(|s| s.suspect.kind).collect()
}

// TT-TEST: support
#[test]
fn family_candidate_owner_emits_at_most_one_candidate_per_real_family() {
    let mut families = super::candidate::FamilyCandidates::default();
    families.set_queue(Some(literal_scored(
        DiagnosisKind::ApplicationQueuePressure,
        70,
        Confidence::Medium,
    )));
    families.set_queue(Some(literal_scored(
        DiagnosisKind::ApplicationQueuePressure,
        71,
        Confidence::Medium,
    )));
    families.set_blocking(Some(literal_scored(
        DiagnosisKind::BlockingPoolPressure,
        72,
        Confidence::Medium,
    )));
    families.set_executor(Some(literal_scored(
        DiagnosisKind::ExecutorPressure,
        73,
        Confidence::Medium,
    )));
    families.set_downstream(Some(literal_scored(
        DiagnosisKind::DownstreamStageDominance,
        74,
        Confidence::Medium,
    )));

    let candidates = families.into_cross_family_candidates();
    assert_eq!(candidates.len(), 4);
    assert_eq!(candidates[0].suspect.score, 71);
    for kind in [
        DiagnosisKind::ApplicationQueuePressure,
        DiagnosisKind::BlockingPoolPressure,
        DiagnosisKind::ExecutorPressure,
        DiagnosisKind::DownstreamStageDominance,
    ] {
        assert_eq!(
            candidates
                .iter()
                .filter(|candidate| candidate.suspect.kind == kind)
                .count(),
            1
        );
    }
}

// TT-TEST: A05 primary
#[test]
fn final_confidence_ranking_selects_primary_before_raw_score() {
    let expected = vec![
        DiagnosisKind::DownstreamStageDominance,
        DiagnosisKind::ApplicationQueuePressure,
        DiagnosisKind::BlockingPoolPressure,
        DiagnosisKind::ExecutorPressure,
        DiagnosisKind::InsufficientEvidence,
    ];
    let candidates = vec![
        literal_scored(DiagnosisKind::InsufficientEvidence, 100, Confidence::High),
        literal_scored(DiagnosisKind::ExecutorPressure, 70, Confidence::Medium),
        literal_scored(
            DiagnosisKind::ApplicationQueuePressure,
            90,
            Confidence::Medium,
        ),
        literal_scored(
            DiagnosisKind::DownstreamStageDominance,
            80,
            Confidence::High,
        ),
        literal_scored(DiagnosisKind::BlockingPoolPressure, 90, Confidence::Medium),
    ];
    assert_eq!(finalized_literal_order(candidates.clone()), expected);
    let mut reversed = candidates;
    reversed.reverse();
    assert_eq!(finalized_literal_order(reversed.clone()), expected);
    reversed.rotate_left(2);
    assert_eq!(finalized_literal_order(reversed), expected);
}

// TT-TEST: A05 primary
#[test]
fn final_confidence_ties_break_by_raw_score_then_kind() {
    let candidates = vec![
        literal_scored(
            DiagnosisKind::DownstreamStageDominance,
            88,
            Confidence::Medium,
        ),
        literal_scored(DiagnosisKind::ExecutorPressure, 88, Confidence::Medium),
        literal_scored(DiagnosisKind::BlockingPoolPressure, 91, Confidence::Medium),
        literal_scored(
            DiagnosisKind::ApplicationQueuePressure,
            88,
            Confidence::Medium,
        ),
    ];
    let expected = vec![
        DiagnosisKind::BlockingPoolPressure,
        DiagnosisKind::ApplicationQueuePressure,
        DiagnosisKind::ExecutorPressure,
        DiagnosisKind::DownstreamStageDominance,
    ];
    assert_eq!(finalized_literal_order(candidates.clone()), expected);
    let mut reversed = candidates.clone();
    reversed.reverse();
    assert_eq!(finalized_literal_order(reversed), expected);
    assert_eq!(
        finalized_literal_order(vec![
            candidates[2].clone(),
            candidates[0].clone(),
            candidates[3].clone(),
            candidates[1].clone(),
        ]),
        expected
    );
}

fn cap_flip_run() -> Run {
    let mut run = test_run();
    run.requests = (0..45)
        .map(|i| RequestEvent {
            request_id: format!("req-{i}"),
            route: "/flip".into(),
            kind: None,
            started_at_unix_ms: i,
            started_at_run_us: Some(i * 2_000),
            finished_at_unix_ms: i + 1,
            finished_at_run_us: Some(i * 2_000 + 1_000),
            latency_us: 1_000,
            outcome: "ok".into(),
        })
        .collect();
    run.queues = (0..45)
        .map(|i| QueueEvent {
            request_id: format!("req-{i}"),
            queue: "worker".into(),
            waited_from_unix_ms: i,
            waited_from_run_us: Some(i * 2_000),
            waited_until_unix_ms: i + 1,
            waited_until_run_us: Some(i * 2_000 + 920),
            wait_us: 920,
            depth_at_start: Some(20),
            completed: false,
        })
        .collect();
    run.stages = (0..45)
        .map(|i| StageEvent {
            request_id: format!("req-{i}"),
            stage: "db".into(),
            relations: tailtriage_core::StageRelations::default(),
            started_at_unix_ms: i,
            started_at_run_us: Some(i * 2_000 + 100),
            finished_at_unix_ms: i + 1,
            finished_at_run_us: Some(i * 2_000 + 600),
            latency_us: 500,
            success: true,
            completed: true,
        })
        .collect();
    run
}

fn assert_scoped_flip(
    primary: &Suspect,
    secondary: &[Suspect],
    expected_primary_score: u8,
    expected_primary_evidence: &[String],
    expected_secondary_evidence: &[String],
) {
    assert_eq!(primary.kind, DiagnosisKind::ApplicationQueuePressure);
    assert_eq!(primary.score, 95);
    assert_eq!(primary.confidence, Confidence::Medium);
    assert_eq!(primary.evidence, expected_secondary_evidence);
    assert!(primary
        .confidence_notes
        .iter()
        .any(|note| note == super::partial_evidence::PARTIAL_QUEUE_CONFIDENCE_NOTE));
    assert_eq!(secondary.len(), 1);
    assert_eq!(secondary[0].kind, DiagnosisKind::DownstreamStageDominance);
    assert_eq!(secondary[0].score, expected_primary_score);
    assert_eq!(secondary[0].confidence, Confidence::Medium);
    assert_eq!(secondary[0].evidence, expected_primary_evidence);
}

fn scoped_evidence(
    stage_us: u64,
    samples: usize,
    cumulative_us: u64,
    queue_us: u8,
) -> (Vec<String>, Vec<String>) {
    (
        vec![
            format!("Stage 'db' has p95 latency {stage_us} us across {samples} samples."),
            format!("Stage 'db' cumulative latency is {cumulative_us} us (500 permille of request latency)."),
            "Stage 'db' contributes 500 permille of tail request latency.".to_string(),
        ],
        vec![
            "Completed-only queue wait at p95 is 0.0% of request time.".to_string(),
            format!("Observed queue-wait lower bound at p95 is {queue_us}.0% of request time and includes {samples} partial queue event(s)."),
            "Observed queue depth sample up to 20.".to_string(),
        ],
    )
}

// TT-TEST: A06 primary
#[test]
fn low_pre_ambiguity_confidence_excludes_candidate_from_cluster() {
    let options = {
        let mut options = AnalyzeOptions::default();
        {
            let o = &mut options.confidence;
            o.ambiguity_min_score = 90;
            o.ambiguity_score_gap = 4;
        }
        options
    };
    let candidates = vec![
        literal_scored(DiagnosisKind::ApplicationQueuePressure, 95, Confidence::Low),
        literal_scored(
            DiagnosisKind::DownstreamStageDominance,
            92,
            Confidence::High,
        ),
        literal_scored(DiagnosisKind::BlockingPoolPressure, 70, Confidence::High),
        literal_scored(DiagnosisKind::InsufficientEvidence, 100, Confidence::High),
    ];
    let expected: Vec<DiagnosisKind> = vec![];
    let permutations = vec![
        candidates.clone(),
        candidates.iter().cloned().rev().collect::<Vec<_>>(),
        vec![
            candidates[2].clone(),
            candidates[0].clone(),
            candidates[3].clone(),
            candidates[1].clone(),
        ],
    ];

    for permutation in permutations {
        let cluster = super::confidence::current_ambiguity_cluster_indices(&permutation, &options)
            .into_iter()
            .map(|idx| permutation[idx].suspect.kind.clone())
            .collect::<Vec<_>>();
        assert_eq!(cluster, expected);
    }
}

// TT-TEST: A05 primary
#[test]
fn evidence_cap_can_promote_lower_raw_score_candidate() {
    let report = analyze_run(&cap_flip_run(), AnalyzeOptions::default())
        .expect("analyzer options should be valid");
    assert_eq!(
        report.primary_suspect.kind,
        DiagnosisKind::ApplicationQueuePressure
    );
    assert_eq!(
        (
            report.primary_suspect.score,
            report.primary_suspect.confidence
        ),
        (95, Confidence::Medium)
    );
    assert_eq!(
        report.secondary_suspects[0].kind,
        DiagnosisKind::DownstreamStageDominance
    );
    assert_eq!(
        (
            report.secondary_suspects[0].score,
            report.secondary_suspects[0].confidence
        ),
        (83, Confidence::Medium)
    );
}

// TT-TEST: A05 primary
#[test]
fn cap_induced_primary_flip_has_exact_json() {
    let report = analyze_run(&cap_flip_run(), AnalyzeOptions::default())
        .expect("analyzer options should be valid");
    let expected_json = r#"{"request_count":45,"p50_latency_us":1000,"p95_latency_us":1000,"p99_latency_us":1000,"p95_queue_share_permille":0,"p95_service_share_permille":1000,"inflight_trend":null,"warnings":["Partial queue/stage observations are lower bounds; completed-duration percentiles exclude them."],"evidence_quality":{"request_count":45,"queue_event_count":45,"stage_event_count":45,"runtime_snapshot_count":0,"inflight_snapshot_count":0,"requests":"present","queues":"partial","stages":"present","runtime_snapshots":"missing","inflight_snapshots":"missing","truncated":false,"dropped_requests":0,"dropped_stages":0,"dropped_queues":0,"dropped_inflight_snapshots":0,"dropped_runtime_snapshots":0,"quality":"partial","limitations":["Partial evidence captured: queues 0 completed/45 partial; stages 45 completed/0 partial. Partial durations are observed lower bounds.","Runtime snapshots are missing, limiting executor and blocking-pressure interpretation."]},"primary_suspect":{"kind":"application_queue_pressure","score":95,"confidence":"medium","evidence":["Completed-only queue wait at p95 is 0.0% of request time.","Observed queue-wait lower bound at p95 is 92.0% of request time and includes 45 partial queue event(s).","Observed queue depth sample up to 20."],"next_checks":["Inspect queue admission limits and producer burst patterns.","Compare queue wait distribution before and after increasing worker parallelism."],"confidence_notes":["Partial queue evidence materially contributes to this suspect; confidence cannot exceed medium because partial durations are lower bounds."]},"secondary_suspects":[{"kind":"downstream_stage_dominance","score":83,"confidence":"medium","evidence":["Stage 'db' has p95 latency 500 us across 45 samples.","Stage 'db' cumulative latency is 22500 us (500 permille of request latency).","Stage 'db' contributes 500 permille of tail request latency."],"next_checks":["Inspect downstream dependency behind stage 'db'.","Collect downstream service timings and retry behavior during tail windows.","Review downstream SLO/error budget and align retry budget/backoff with it."],"confidence_notes":[]}],"route_breakdowns":[],"temporal_segments":[]}"#;
    assert_eq!(render_json(&report).unwrap(), expected_json);
}

// TT-TEST: A05 primary
#[test]
fn cap_induced_primary_flip_has_exact_text() {
    let report = analyze_run(&cap_flip_run(), AnalyzeOptions::default())
        .expect("analyzer options should be valid");
    let expected_text = "tailtriage diagnosis\nRequests analyzed: 45\nLatency (us): p50 1000, p95 1000, p99 1000\nRequest time at p95: queue 0.0%, non-queue service 100.0%\nInflight trend: none\nPrimary suspect: application queue pressure (medium confidence, score 95)\nEvidence quality: partial (Partial evidence captured: queues 0 completed/45 partial; stages 45 completed/0 partial. Partial durations are observed lower bounds.)\nWarnings:\n- Partial queue/stage observations are lower bounds; completed-duration percentiles exclude them.\nEvidence:\n- Completed-only queue wait at p95 is 0.0% of request time.\n- Observed queue-wait lower bound at p95 is 92.0% of request time and includes 45 partial queue event(s).\n- Observed queue depth sample up to 20.\nNext checks:\n- Inspect queue admission limits and producer burst patterns.\n- Compare queue wait distribution before and after increasing worker parallelism.\nSecondary suspects:\n- downstream stage dominance (medium confidence, score 83)";
    assert_eq!(render_text(&report), expected_text);
}

// TT-TEST: A06 primary
#[test]
fn raw_score_ambiguity_caps_all_cluster_members_uniformly() {
    let mut run = cap_flip_run();
    for stage in &mut run.stages {
        stage.latency_us = 900;
        stage.finished_at_run_us = stage.started_at_run_us.map(|s| s + 900);
    }
    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    let warning = "Top suspects are close in score; treat ranking as ambiguous and validate both with next checks.".to_string();
    let ambiguity_note =
        "Top suspects are close in score; confidence is capped by ambiguity.".to_string();
    let partial_queue_note = super::partial_evidence::PARTIAL_QUEUE_CONFIDENCE_NOTE.to_string();

    assert_eq!(
        report.primary_suspect.kind,
        DiagnosisKind::ApplicationQueuePressure
    );
    assert_eq!(report.primary_suspect.score, 95);
    assert_eq!(report.primary_suspect.confidence, Confidence::Medium);
    assert_eq!(report.primary_suspect.evidence, vec![
        "Completed-only queue wait at p95 is 0.0% of request time.".to_string(),
        "Observed queue-wait lower bound at p95 is 92.0% of request time and includes 45 partial queue event(s).".to_string(),
        "Observed queue depth sample up to 20.".to_string(),
    ]);
    assert_eq!(
        report.primary_suspect.next_checks,
        vec![
            "Inspect queue admission limits and producer burst patterns.".to_string(),
            "Compare queue wait distribution before and after increasing worker parallelism."
                .to_string(),
        ]
    );
    assert_eq!(
        report.primary_suspect.confidence_notes,
        vec![partial_queue_note.clone(), ambiguity_note.clone()]
    );

    let secondary = &report.secondary_suspects[0];
    assert_eq!(secondary.kind, DiagnosisKind::DownstreamStageDominance);
    assert_eq!(secondary.score, 95);
    assert_eq!(secondary.confidence, Confidence::Medium);
    assert_eq!(
        secondary.evidence,
        vec![
            "Stage 'db' has p95 latency 900 us across 45 samples.".to_string(),
            "Stage 'db' cumulative latency is 40500 us (900 permille of request latency)."
                .to_string(),
            "Stage 'db' contributes 900 permille of tail request latency.".to_string(),
        ]
    );
    assert_eq!(
        secondary.next_checks,
        vec![
            "Inspect downstream dependency behind stage 'db'.".to_string(),
            "Collect downstream service timings and retry behavior during tail windows."
                .to_string(),
            "Review downstream SLO/error budget and align retry budget/backoff with it."
                .to_string(),
        ]
    );
    assert_eq!(secondary.confidence_notes, vec![ambiguity_note.clone()]);
    assert_eq!(
        report
            .secondary_suspects
            .iter()
            .map(|s| s.kind.clone())
            .collect::<Vec<_>>(),
        vec![DiagnosisKind::DownstreamStageDominance]
    );
    assert_eq!(
        report.warnings,
        vec![
            warning,
            super::partial_evidence::PARTIAL_WARNING.to_string()
        ]
    );

    for suspect in std::iter::once(&report.primary_suspect).chain(report.secondary_suspects.iter())
    {
        assert_eq!(suspect.confidence, Confidence::Medium);
        assert!(suspect.confidence_notes.contains(&ambiguity_note));
        assert!(
            !(suspect.confidence == Confidence::High
                && suspect.confidence_notes.contains(&ambiguity_note))
        );
    }
}

// TT-TEST: A06 primary
#[test]
fn equal_final_confidence_without_raw_score_proximity_is_not_ambiguous() {
    let mut run = cap_flip_run();
    for stage in &mut run.stages {
        stage.latency_us = 500;
        stage.finished_at_run_us = stage.started_at_run_us.map(|s| s + 500);
    }
    let options = {
        let mut options = AnalyzeOptions::default();
        {
            let o = &mut options.confidence;
            o.high_score_threshold = 96;
        }
        options
    };
    let report = analyze_run(&run, options.clone()).expect("analyzer options should be valid");
    let primary = &report.primary_suspect;
    let secondary = &report.secondary_suspects[0];
    assert_eq!(primary.kind, DiagnosisKind::ApplicationQueuePressure);
    assert_eq!(secondary.kind, DiagnosisKind::DownstreamStageDominance);
    assert_eq!(primary.score, 95);
    assert_eq!(secondary.score, 83);
    assert_eq!(primary.confidence, Confidence::Medium);
    assert_eq!(secondary.confidence, Confidence::Medium);
    let raw_score_difference = primary.score.abs_diff(secondary.score);
    assert_eq!(raw_score_difference, 12);
    assert_eq!(options.confidence.ambiguity_score_gap, 4);
    assert!(raw_score_difference > options.confidence.ambiguity_score_gap);
    assert_eq!(
        report
            .secondary_suspects
            .iter()
            .map(|s| s.kind.clone())
            .collect::<Vec<_>>(),
        vec![DiagnosisKind::DownstreamStageDominance]
    );
    assert_eq!(
        report.warnings,
        vec![
            "No runtime snapshots captured; executor and blocking-pressure interpretation is limited.".to_string(),
            super::partial_evidence::PARTIAL_WARNING.to_string(),
        ]
    );
    assert_eq!(
        primary.confidence_notes,
        vec![super::partial_evidence::PARTIAL_QUEUE_CONFIDENCE_NOTE.to_string()]
    );
    assert!(secondary.confidence_notes.is_empty());
    assert!(!report.warnings.iter().any(|w| w.contains("close in score")));
    assert!(!std::iter::once(primary)
        .chain(report.secondary_suspects.iter())
        .any(|s| s
            .confidence_notes
            .iter()
            .any(|n| n.contains("capped by ambiguity"))));
}

// TT-TEST: support
#[test]
fn candidate_order_is_stable_under_irrelevant_input_reordering() {
    let run = cap_flip_run();
    let mut reordered = run.clone();
    reordered.queues.reverse();
    reordered.stages.reverse();
    reordered.requests.reverse();
    let a = analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    let b = analyze_run(&reordered, AnalyzeOptions::default())
        .expect("analyzer options should be valid");
    assert_eq!(a.primary_suspect, b.primary_suspect);
    assert_eq!(a.secondary_suspects, b.secondary_suspects);
    assert_eq!(a.warnings, b.warnings);
    assert_eq!(a.route_breakdowns, b.route_breakdowns);
    assert_eq!(a.temporal_segments, b.temporal_segments);
    assert_eq!(render_json(&a).unwrap(), render_json(&b).unwrap());
    assert_eq!(render_text(&a), render_text(&b));
}

fn scoped_flip_report() -> Report {
    let mut run = cap_flip_run();
    for (i, req) in run.requests.iter_mut().enumerate() {
        req.route = if i < 23 { "/completed" } else { "/partial" }.into();
        if i >= 23 {
            req.latency_us = 2_000;
            req.finished_at_run_us = req.started_at_run_us.map(|s| s + 2_000);
        }
    }
    for (i, queue) in run.queues.iter_mut().enumerate() {
        if i >= 23 {
            queue.waited_until_run_us = queue.waited_from_run_us.map(|s| s + 1_840);
            queue.wait_us = 1_840;
        }
    }
    for (i, stage) in run.stages.iter_mut().enumerate() {
        if i >= 23 {
            stage.finished_at_run_us = stage.started_at_run_us.map(|s| s + 1_000);
            stage.latency_us = 1_000;
        }
    }
    let options = {
        let mut options = AnalyzeOptions::default();
        {
            let o = &mut options.route;
            o.min_request_count = 10;
        }
        {
            let o = &mut options.temporal;
            o.min_request_count = 20;
            o.min_segment_request_count = 10;
        }
        options
    };
    analyze_run(&run, options).expect("analyzer options should be valid")
}

// TT-TEST: support
#[test]
fn global_route_and_temporal_share_final_confidence_ordering() {
    let report = scoped_flip_report();
    assert_eq!(report.route_breakdowns.len(), 2);
    assert_eq!(report.temporal_segments.len(), 2);
    assert_eq!(
        report.primary_suspect.kind,
        DiagnosisKind::ApplicationQueuePressure
    );
    assert_eq!(report.primary_suspect.score, 95);
    assert_eq!(report.primary_suspect.confidence, Confidence::Medium);
    assert_eq!(
        report
            .secondary_suspects
            .iter()
            .map(|s| s.kind.clone())
            .collect::<Vec<_>>(),
        vec![DiagnosisKind::DownstreamStageDominance]
    );
    assert_eq!(report.secondary_suspects[0].score, 83);
    assert_eq!(report.secondary_suspects[0].confidence, Confidence::Medium);
    assert!(report.primary_suspect.score > report.secondary_suspects[0].score);

    let completed = report
        .route_breakdowns
        .iter()
        .find(|r| r.route == "/completed")
        .unwrap();
    let partial = report
        .route_breakdowns
        .iter()
        .find(|r| r.route == "/partial")
        .unwrap();
    let completed_evidence = scoped_evidence(500, 23, 11500, 92);
    assert_scoped_flip(
        &completed.primary_suspect,
        &completed.secondary_suspects,
        83,
        &completed_evidence.0,
        &completed_evidence.1,
    );
    let partial_evidence = scoped_evidence(1000, 22, 22000, 92);
    assert_scoped_flip(
        &partial.primary_suspect,
        &partial.secondary_suspects,
        83,
        &partial_evidence.0,
        &partial_evidence.1,
    );
    assert_eq!(
        completed.warnings,
        vec![ROUTE_RUNTIME_ATTRIBUTION_WARNING.to_string()]
    );
    assert_eq!(
        partial.warnings,
        vec![ROUTE_RUNTIME_ATTRIBUTION_WARNING.to_string()]
    );

    let early = report
        .temporal_segments
        .iter()
        .find(|s| s.name == "early")
        .unwrap();
    let late = report
        .temporal_segments
        .iter()
        .find(|s| s.name == "late")
        .unwrap();
    let early_evidence = scoped_evidence(500, 22, 11000, 92);
    assert_scoped_flip(
        &early.primary_suspect,
        &early.secondary_suspects,
        83,
        &early_evidence.0,
        &early_evidence.1,
    );
    let late_evidence = scoped_evidence(1000, 23, 22500, 92);
    assert_scoped_flip(
        &late.primary_suspect,
        &late.secondary_suspects,
        83,
        &late_evidence.0,
        &late_evidence.1,
    );
    assert!(early.warnings.is_empty());
    assert!(late.warnings.is_empty());
}

// TT-TEST: A02 secondary
#[test]
fn downstream_overlap_uses_request_scoped_stage_attribution_for_score() {
    let mut run = test_run();
    run.requests = vec![
        precise_request("a", 100),
        precise_request("b", 100),
        precise_request("c", 100),
    ];
    run.stages = vec![
        precise_stage("a", "db", Some(0), Some(60), 60),
        precise_stage("a", "db", Some(40), Some(90), 50),
        precise_stage("b", "db", Some(0), Some(20), 20),
        precise_stage("c", "db", Some(0), Some(20), 20),
    ];

    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    let suspect = downstream_suspect(&report);

    assert_eq!(suspect.score, 75);
    assert_eq!(
        suspect.evidence[0],
        "Stage 'db' has p95 latency 90 us across 3 samples."
    );
    assert_eq!(
        suspect.evidence[1],
        "Stage 'db' cumulative latency is 130 us (433 permille of request latency)."
    );
    assert_eq!(
        suspect.evidence[2],
        "Stage 'db' contributes 433 permille of tail request latency."
    );
}

// TT-TEST: support
#[test]
fn downstream_eligibility_uses_distinct_request_samples_not_raw_events() {
    let mut run = test_run();
    run.requests = vec![precise_request("only", 100)];
    for i in 0..10 {
        run.stages.push(precise_stage(
            "only",
            "db",
            Some(i * 10),
            Some(i * 10 + 5),
            5,
        ));
    }

    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");

    assert_ne!(
        report.primary_suspect.kind,
        DiagnosisKind::DownstreamStageDominance
    );
    assert!(report
        .secondary_suspects
        .iter()
        .all(|s| s.kind != DiagnosisKind::DownstreamStageDominance));
}

// TT-TEST: support
#[test]
fn downstream_approximate_stage_group_warns_only_with_canonical_precision_warning() {
    let mut run = test_run();
    run.requests = vec![
        precise_request("a", 100),
        precise_request("b", 100),
        precise_request("c", 100),
    ];
    run.stages = vec![
        precise_stage("a", "db", Some(0), Some(20), 20),
        precise_stage("a", "db", None, None, 90),
        precise_stage("b", "db", Some(0), Some(10), 10),
        precise_stage("c", "db", Some(0), Some(10), 10),
    ];

    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    let suspect = downstream_suspect(&report);

    assert_eq!(suspect.score, 71);
    assert_eq!(
        suspect.evidence[0],
        "Stage 'db' has p95 latency 100 us across 3 samples."
    );
    assert_eq!(
        suspect.evidence[1],
        "Stage 'db' cumulative latency is 120 us (400 permille of request latency)."
    );
    assert_eq!(
        suspect.evidence[2],
        "Stage 'db' contributes 400 permille of tail request latency."
    );
    assert!(report
        .warnings
        .iter()
        .any(|w| w.contains("precise_interval_validation_unavailable")));
    assert!(!report.warnings.iter().any(|w| w.contains("attribution")));
}

// TT-TEST: support
#[test]
fn downstream_stage_attribution_respects_normalization_boundary() {
    let mut run = test_run();
    run.requests = vec![
        precise_request("a", 100),
        precise_request("b", 100),
        precise_request("c", 100),
    ];
    run.stages = vec![
        precise_stage("a", "db", Some(10), Some(40), 30),
        precise_stage("a", "db", Some(80), Some(120), 40),
        precise_stage("b", "db", Some(0), Some(20), 20),
        precise_stage("c", "db", Some(0), Some(20), 20),
    ];

    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    assert_eq!(
        report.primary_suspect.kind,
        DiagnosisKind::InsufficientEvidence
    );

    assert!(report
        .warnings
        .iter()
        .any(|w| w.contains("child_interval_outside_request") && w.contains("stage")));
    assert!(!report.warnings.iter().any(|w| w.contains("attribution")));
}

// TT-TEST: support
#[test]
fn downstream_stage_input_order_invariance_extends_to_canonical_json() {
    let mut first = test_run();
    first.requests = vec![
        precise_request("a", 100),
        precise_request("b", 100),
        precise_request("c", 100),
    ];
    first.stages = vec![
        precise_stage("a", "db", Some(40), Some(90), 50),
        precise_stage("c", "cache", Some(0), Some(20), 20),
        precise_stage("a", "db", Some(0), Some(60), 60),
        precise_stage("b", "db", Some(0), Some(20), 20),
        precise_stage("c", "db", Some(0), Some(20), 20),
    ];
    let mut second = first.clone();
    second.stages = vec![
        first.stages[4].clone(),
        first.stages[2].clone(),
        first.stages[1].clone(),
        first.stages[0].clone(),
        first.stages[3].clone(),
    ];

    let first_report =
        analyze_run(&first, AnalyzeOptions::default()).expect("analyzer options should be valid");
    let second_report =
        analyze_run(&second, AnalyzeOptions::default()).expect("analyzer options should be valid");

    assert_eq!(first_report, second_report);
    assert_eq!(
        render_json(&first_report).unwrap(),
        render_json(&second_report).unwrap()
    );
    assert_eq!(
        render_json_pretty(&first_report).unwrap(),
        render_json_pretty(&second_report).unwrap()
    );
}

// TT-TEST: support
#[test]
fn downstream_non_overlap_single_event_per_request_behavior_remains_stable() {
    let mut run = test_run();
    run.requests = vec![
        precise_request("a", 100),
        precise_request("b", 100),
        precise_request("c", 100),
    ];
    run.stages = vec![
        precise_stage("a", "db", Some(0), Some(60), 60),
        precise_stage("b", "db", Some(0), Some(20), 20),
        precise_stage("c", "db", Some(0), Some(20), 20),
    ];

    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    let suspect = downstream_suspect(&report);

    assert_eq!(suspect.kind, DiagnosisKind::DownstreamStageDominance);
    assert_eq!(suspect.score, 63);
    assert_eq!(
        suspect.evidence,
        vec![
            "Stage 'db' has p95 latency 60 us across 3 samples.".to_string(),
            "Stage 'db' cumulative latency is 100 us (333 permille of request latency)."
                .to_string(),
            "Stage 'db' contributes 333 permille of tail request latency.".to_string(),
        ]
    );
}

// TT-TEST: A02 secondary
#[test]
fn overlapping_precise_queues_are_union_attributed() {
    let mut run = test_run();
    run.requests = vec![precise_request("req-overlap", 100)];
    run.queues = vec![
        precise_queue("req-overlap", 0, 60, 60),
        precise_queue("req-overlap", 40, 90, 50),
    ];

    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");

    assert_eq!(report.p95_queue_share_permille, Some(900));
    assert_eq!(report.p95_service_share_permille, Some(100));
}

// TT-TEST: support
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
            completed: true,
        },
    ];

    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");

    assert_eq!(report.p95_queue_share_permille, Some(1000));
    assert_eq!(report.p95_service_share_permille, Some(0));
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("precise_interval_validation_unavailable")));
}

// TT-TEST: support
#[test]
fn out_of_parent_precise_queue_is_excluded_before_attribution_not_clipped() {
    let mut run = test_run();
    run.requests = vec![precise_request("req-boundary", 100)];
    run.queues = vec![
        precise_queue("req-boundary", 10, 40, 30),
        precise_queue("req-boundary", 80, 120, 40),
    ];

    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");

    assert_eq!(report.p95_queue_share_permille, Some(300));
    assert_eq!(report.p95_service_share_permille, Some(700));
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("child_interval_outside_request")
            && warning.contains("queue")));
}

// TT-TEST: support
#[test]
fn non_overlapping_queue_attribution_remains_stable() {
    let mut run = test_run();
    run.requests = vec![precise_request("req-stable", 100)];
    run.queues = vec![
        precise_queue("req-stable", 0, 20, 20),
        precise_queue("req-stable", 40, 70, 30),
    ];

    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");

    assert_eq!(report.p95_queue_share_permille, Some(500));
    assert_eq!(report.p95_service_share_permille, Some(500));
}

// TT-TEST: support
#[test]
fn repeated_analysis_is_deterministic_for_overlap_safe_queue_attribution() {
    let mut run = test_run();
    run.requests = vec![precise_request("req-deterministic", 100)];
    run.queues = vec![
        precise_queue("req-deterministic", 40, 90, 50),
        precise_queue("req-deterministic", 0, 60, 60),
    ];

    let first =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    let first_json = render_json(&first).expect("render first report");
    for _ in 0..10 {
        let next =
            analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
        let next_json = render_json(&next).expect("render next report");
        assert_eq!(next, first);
        assert_eq!(next_json, first_json);
    }
}

// TT-TEST: support
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

    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    assert_eq!(report.p95_queue_share_permille, Some(600));
    assert_eq!(report.p95_service_share_permille, Some(600));
}

// TT-TEST: support
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
        completed: true,
    }];

    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");

    assert_eq!(report.request_count, 1);
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("duplicate_completed_request_id")
            && warning.contains("request_id")));
}

// TT-TEST: support
#[test]
fn unique_completed_request_ids_do_not_emit_duplicate_warning() {
    let report = analyze_run(&test_run(), AnalyzeOptions::default())
        .expect("analyzer options should be valid");

    assert!(!report
        .warnings
        .iter()
        .any(|warning| warning.contains("duplicate_completed_request_id")));
}

// TT-TEST: support
#[test]
fn permissive_analysis_warns_but_accepts_orphan_request_scoped_events() {
    let mut run = test_run();
    run.stages = vec![StageEvent {
        request_id: "missing-stage-request".to_owned(),
        stage: "db".to_owned(),
        relations: tailtriage_core::StageRelations::default(),
        started_at_unix_ms: 1,
        started_at_run_us: None,
        finished_at_unix_ms: 2,
        finished_at_run_us: None,
        latency_us: 100,
        success: true,
        completed: true,
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
        completed: true,
    }];

    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");

    assert_eq!(report.request_count, 3);
    assert!(report.warnings.iter().any(|warning| {
        warning.contains("orphan_request_scoped_event") && warning.contains("stage")
    }));
    assert!(report.warnings.iter().any(|warning| {
        warning.contains("orphan_request_scoped_event") && warning.contains("queue")
    }));
}

// TT-TEST: support
#[test]
fn matching_unique_request_scoped_events_do_not_add_request_id_limitations() {
    let mut run = test_run();
    run.stages = vec![StageEvent {
        request_id: "req-1".to_owned(),
        stage: "db".to_owned(),
        relations: tailtriage_core::StageRelations::default(),
        started_at_unix_ms: 1,
        started_at_run_us: None,
        finished_at_unix_ms: 2,
        finished_at_run_us: None,
        latency_us: 100,
        success: true,
        completed: true,
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
        completed: true,
    }];

    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");

    assert!(!report
        .evidence_quality
        .limitations
        .iter()
        .any(|limitation| limitation
            == "Stage or queue evidence with no matching completed request_id cannot be reliably attributed."));
}

// TT-TEST: A02 secondary
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

    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");

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
        worker_count: None,
        blocking_queue_depth: blocking,
        remote_schedule_count: None,
    }
}

fn executor_suspect(report: &Report) -> &Suspect {
    std::iter::once(&report.primary_suspect)
        .chain(&report.secondary_suspects)
        .find(|suspect| suspect.kind == DiagnosisKind::ExecutorPressure)
        .expect("executor suspect")
}

fn competing_executor_run(
    global_depth: u64,
    local_depth: u64,
    blocking_depth: u64,
    queue: Option<(u64, u64)>,
    stage_latency: Option<u64>,
) -> Run {
    let mut run = test_run();
    run.requests = (0..45)
        .map(|index| {
            let id = format!("competing-{index}");
            let start = index * 2_000;
            RequestEvent {
                request_id: id,
                route: "/competing".into(),
                kind: None,
                started_at_unix_ms: index,
                started_at_run_us: Some(start),
                finished_at_unix_ms: index + 1,
                finished_at_run_us: Some(start + 1_000),
                latency_us: 1_000,
                outcome: "ok".into(),
            }
        })
        .collect();
    run.queues = Some(queue.unwrap_or((0, 0))).map_or_else(Vec::new, |(wait_us, depth)| {
        (0..45)
            .map(|index| {
                let start = index * 2_000;
                let mut event = precise_queue(
                    &format!("competing-{index}"),
                    start,
                    start + wait_us,
                    wait_us,
                );
                event.depth_at_start = Some(depth);
                event
            })
            .collect()
    });
    run.stages = Some(stage_latency.unwrap_or(0)).map_or_else(Vec::new, |latency_us| {
        (0..45)
            .map(|index| {
                let start = index * 2_000;
                precise_stage(
                    &format!("competing-{index}"),
                    "database",
                    Some(start),
                    Some(start + latency_us),
                    latency_us,
                )
            })
            .collect()
    });
    run.runtime_snapshots = (0..45)
        .map(|index| RuntimeSnapshot {
            at_unix_ms: index,
            at_run_us: Some(index * 2_000),
            global_queue_depth: Some(global_depth),
            local_queue_depth: Some(local_depth),
            alive_tasks: Some(40),
            worker_count: Some(4),
            blocking_queue_depth: Some(blocking_depth),
            remote_schedule_count: None,
        })
        .collect();
    run.inflight = (0..45)
        .map(|index| InFlightSnapshot {
            at_unix_ms: index,
            at_run_us: Some(index * 2_000),
            gauge: "requests".into(),
            count: 1,
        })
        .collect();
    run
}

fn suspect_position(report: &Report, kind: &DiagnosisKind) -> usize {
    std::iter::once(&report.primary_suspect)
        .chain(&report.secondary_suspects)
        .position(|suspect| &suspect.kind == kind)
        .unwrap_or_else(|| panic!("missing expected suspect {kind:?}"))
}

// TT-TEST: support
#[test]
fn normalized_executor_remains_secondary_to_strong_blocking_pressure() {
    let report = analyze_run(
        &competing_executor_run(8, 0, 24, None, None),
        AnalyzeOptions::default(),
    )
    .expect("analyzer options should be valid");
    let executor = executor_suspect(&report);
    assert_eq!(
        report.primary_suspect.kind,
        DiagnosisKind::BlockingPoolPressure
    );
    assert_eq!(report.primary_suspect.confidence, Confidence::Medium);
    assert_eq!(executor.confidence, Confidence::Low);
    assert!(
        suspect_position(&report, &DiagnosisKind::BlockingPoolPressure)
            < suspect_position(&report, &DiagnosisKind::ExecutorPressure)
    );
    assert!(executor
        .evidence
        .iter()
        .any(|e| { e.contains("2000 milli-tasks per worker") && e.contains("worker_count=4") }));
}

// TT-TEST: support
#[test]
fn normalized_executor_remains_secondary_to_strong_downstream_stage() {
    let report = analyze_run(
        &competing_executor_run(8, 0, 0, None, Some(970)),
        AnalyzeOptions::default(),
    )
    .expect("analyzer options should be valid");
    let executor = executor_suspect(&report);
    let downstream = downstream_suspect(&report);
    assert_eq!(
        report.primary_suspect.kind,
        DiagnosisKind::DownstreamStageDominance
    );
    assert_eq!(downstream.confidence, Confidence::High);
    assert_eq!(executor.confidence, Confidence::Low);
    assert!(
        suspect_position(&report, &DiagnosisKind::DownstreamStageDominance)
            < suspect_position(&report, &DiagnosisKind::ExecutorPressure)
    );
    assert!(downstream
        .evidence
        .iter()
        .any(|e| e.contains("Stage 'database' has p95 latency 970 us across 45 samples")));
}

// TT-TEST: support
#[test]
fn application_queue_controls_preserve_normalized_executor_visibility_and_order() {
    let cases = [
        (990, 20, 8, DiagnosisKind::ApplicationQueuePressure),
        (600, 4, 16, DiagnosisKind::ExecutorPressure),
    ];
    for (wait_us, depth, global_depth, expected_primary) in cases {
        let report = analyze_run(
            &competing_executor_run(global_depth, 0, 0, Some((wait_us, depth)), None),
            AnalyzeOptions::default(),
        )
        .expect("analyzer options should be valid");
        let queue = std::iter::once(&report.primary_suspect)
            .chain(&report.secondary_suspects)
            .find(|suspect| suspect.kind == DiagnosisKind::ApplicationQueuePressure)
            .expect("application queue suspect");
        let executor = executor_suspect(&report);
        assert_eq!(report.primary_suspect.kind, expected_primary);
        assert!(suspect_position(&report, &DiagnosisKind::ApplicationQueuePressure) < 2);
        assert!(suspect_position(&report, &DiagnosisKind::ExecutorPressure) < 2);
        if wait_us == 600 {
            assert_eq!(
                (executor.score, executor.confidence),
                (74, Confidence::Medium)
            );
            assert_eq!((queue.score, queue.confidence), (66, Confidence::Medium));
            assert!(executor.score > queue.score);
        } else {
            assert_eq!(queue.confidence, Confidence::High);
            assert_eq!(executor.confidence, Confidence::Low);
        }
    }
}

// TT-TEST: support
#[test]
fn clear_normalized_executor_remains_primary_against_weak_competing_signals() {
    let report = analyze_run(
        &competing_executor_run(32, 0, 0, Some((10, 1)), Some(10)),
        AnalyzeOptions::default(),
    )
    .expect("analyzer options should be valid");
    assert_eq!(report.primary_suspect.kind, DiagnosisKind::ExecutorPressure);
    assert_eq!(report.primary_suspect.confidence, Confidence::High);
    assert!(report
        .primary_suspect
        .evidence
        .iter()
        .any(|e| e.contains("8000 milli-tasks per worker")));
    assert!(report
        .secondary_suspects
        .iter()
        .all(|suspect| suspect.score < report.primary_suspect.score));
}

// TT-TEST: support
#[test]
fn normalized_lower_bound_cap_keeps_higher_score_executor_below_high_confidence_stage() {
    let mut run = competing_executor_run(140, 60, 0, None, Some(500));
    for snapshot in &mut run.runtime_snapshots {
        snapshot.local_queue_depth = None;
    }
    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    let executor = executor_suspect(&report);
    let downstream = downstream_suspect(&report);
    assert_eq!(report.primary_suspect.kind, DiagnosisKind::ExecutorPressure);
    assert_eq!(
        (downstream.score, downstream.confidence),
        (83, Confidence::Medium)
    );
    assert_eq!(
        (executor.score, executor.confidence),
        (89, Confidence::Medium)
    );
    assert!(executor.score > downstream.score);
    assert!(
        suspect_position(&report, &DiagnosisKind::ExecutorPressure)
            < suspect_position(&report, &DiagnosisKind::DownstreamStageDominance)
    );
    assert_eq!(
        executor
            .confidence_notes
            .iter()
            .filter(|note| note.contains("Missing local queue depth"))
            .count(),
        1
    );
}

fn worker_test_run() -> Run {
    let mut run = test_run();
    run.requests = (0..20)
        .map(|index| RequestEvent {
            request_id: format!("worker-{index}"),
            route: "/worker".into(),
            kind: None,
            started_at_unix_ms: 1_000 + index * 10,
            started_at_run_us: Some(index * 10_000),
            finished_at_unix_ms: 1_001 + index * 10,
            finished_at_run_us: Some(index * 10_000 + if index < 10 { 1_000 } else { 3_000 }),
            latency_us: if index < 10 { 1_000 } else { 3_000 },
            outcome: "ok".into(),
        })
        .collect();
    run.runtime_snapshots = (0..20)
        .map(|index| RuntimeSnapshot {
            at_unix_ms: 1_000 + index * 10,
            at_run_us: Some(index * 10_000),
            global_queue_depth: Some(if index < 10 { 64 } else { 128 }),
            local_queue_depth: Some(0),
            alive_tasks: Some(400),
            worker_count: Some(if index < 10 { 4 } else { 8 }),
            blocking_queue_depth: Some(0),
            remote_schedule_count: None,
        })
        .collect();
    run
}

fn temporal_worker_report(run: &Run) -> Report {
    analyze_run(run, {
        let mut options = AnalyzeOptions::default();
        {
            let options = &mut options.temporal;
            options.min_request_count = 20;
            options.min_segment_request_count = 10;
        }
        options
    })
    .expect("analyzer options should be valid")
}

// TT-TEST: support
#[test]
fn temporal_worker_evidence_is_classified_within_each_original_window() {
    let report = temporal_worker_report(&worker_test_run());
    assert_eq!(report.temporal_segments.len(), 2);
    for (segment, worker_count) in report.temporal_segments.iter().zip([4, 8]) {
        let segment_report = Report {
            primary_suspect: segment.primary_suspect.clone(),
            secondary_suspects: segment.secondary_suspects.clone(),
            ..report.clone()
        };
        let suspect = executor_suspect(&segment_report);
        assert!(suspect.evidence.iter().any(|evidence| {
            evidence.contains(&format!("worker_count={worker_count}"))
                && evidence.contains("16000 milli-tasks per worker")
        }));
        assert!(!suspect
            .evidence
            .iter()
            .any(|evidence| evidence.contains("legacy absolute-depth scoring")));
    }
}

// TT-TEST: support
#[test]
fn excluded_requests_do_not_change_canonical_temporal_segments() {
    let baseline = worker_test_run();
    let mut with_duplicate = baseline.clone();
    let mut duplicate = baseline.requests[0].clone();
    duplicate.request_id = "ambiguous-extreme".into();
    duplicate.started_at_unix_ms = 1_000_000;
    duplicate.finished_at_unix_ms = 1_000_001;
    duplicate.started_at_run_us = Some(1_000_000_000);
    duplicate.finished_at_run_us = Some(1_000_001_000);
    with_duplicate.requests.push(duplicate.clone());
    with_duplicate.requests.push(duplicate);

    let baseline_report = temporal_worker_report(&baseline);
    let duplicate_report = temporal_worker_report(&with_duplicate);

    assert_eq!(
        duplicate_report.temporal_segments,
        baseline_report.temporal_segments
    );
    assert!(duplicate_report.warnings.iter().any(|warning| {
        warning.contains("duplicate_completed_request_id") && warning.contains("request_id")
    }));
    assert_eq!(
        duplicate_report
            .temporal_segments
            .iter()
            .map(|segment| segment.evidence_quality.runtime_snapshot_count)
            .collect::<Vec<_>>(),
        [10, 10]
    );
}

// TT-TEST: support
#[test]
fn cleared_optional_request_timing_defines_temporal_order_and_windows() {
    let mut invalid = worker_test_run();
    invalid.requests[9].started_at_run_us = Some(900_000_000);
    invalid.requests[9].finished_at_run_us = Some(1);
    let mut manually_cleared = invalid.clone();
    manually_cleared.requests[9].started_at_run_us = None;
    manually_cleared.requests[9].finished_at_run_us = None;

    let invalid_report = temporal_worker_report(&invalid);
    let cleared_report = temporal_worker_report(&manually_cleared);

    assert_eq!(
        invalid_report.temporal_segments,
        cleared_report.temporal_segments
    );
    assert!(invalid_report
        .warnings
        .iter()
        .any(|warning| warning.contains("inverted_interval")));
    for segment in &invalid_report.temporal_segments {
        assert_eq!(segment.evidence_quality.runtime_snapshot_count, 10);
    }
    assert!(invalid_report.temporal_segments[0]
        .warnings
        .iter()
        .any(|warning| warning == TEMPORAL_WALL_CLOCK_FALLBACK_WARNING));
    assert!(!invalid_report.temporal_segments[1]
        .warnings
        .iter()
        .any(|warning| warning == TEMPORAL_WALL_CLOCK_FALLBACK_WARNING));
}

// TT-TEST: support
#[test]
fn temporal_partial_invalid_and_missing_local_limit_only_the_affected_window() {
    for (name, mutate, expected) in [
        (
            "partial",
            10_usize,
            "Worker-count evidence is Partial; legacy absolute-depth scoring",
        ),
        (
            "invalid-zero",
            11_usize,
            "Worker-count evidence is InvalidZero; legacy absolute-depth scoring",
        ),
        (
            "missing-local",
            12_usize,
            "Runnable queue normalization is a lower bound",
        ),
    ] {
        let mut run = worker_test_run();
        match name {
            "partial" => run.runtime_snapshots[mutate].worker_count = None,
            "invalid-zero" => run.runtime_snapshots[mutate].worker_count = Some(0),
            "missing-local" => run.runtime_snapshots[mutate].local_queue_depth = None,
            _ => unreachable!(),
        }
        let report = temporal_worker_report(&run);
        let early_report = Report {
            primary_suspect: report.temporal_segments[0].primary_suspect.clone(),
            secondary_suspects: report.temporal_segments[0].secondary_suspects.clone(),
            ..report.clone()
        };
        let late_report = Report {
            primary_suspect: report.temporal_segments[1].primary_suspect.clone(),
            secondary_suspects: report.temporal_segments[1].secondary_suspects.clone(),
            ..report.clone()
        };
        let early = executor_suspect(&early_report);
        let late = executor_suspect(&late_report);
        assert!(early.evidence.iter().any(|e| e.contains("worker_count=4")));
        assert!(early
            .confidence_notes
            .iter()
            .all(|note| !note.contains("worker-count") && !note.contains("local queue depth")));
        assert!(late.evidence.iter().any(|e| e.contains(expected)));
        assert_ne!(late.confidence, Confidence::High);
    }
}

// TT-TEST: A03 primary
#[test]
fn ambiguous_worker_fallbacks_match_historical_score_and_explain_the_cap() {
    let mut historical = worker_test_run();
    historical.runtime_snapshots.truncate(10);
    for snapshot in &mut historical.runtime_snapshots {
        snapshot.worker_count = None;
    }
    let historical_report = analyze_run(&historical, AnalyzeOptions::default())
        .expect("analyzer options should be valid");
    let historical_executor = executor_suspect(&historical_report);
    let historical_score = historical_executor.score;
    assert!(historical_executor.evidence.iter().any(|e| e.contains(
        "historical artifact has no worker-count evidence; legacy absolute-depth scoring was used"
    )));
    assert!(historical_executor
        .confidence_notes
        .iter()
        .all(|note| !note.contains("worker-count")));

    for (expected, counts) in [
        ("Partial", vec![Some(4), None]),
        ("Inconsistent", vec![Some(4), Some(8)]),
        ("InvalidZero", vec![Some(4), Some(0)]),
    ] {
        let mut run = historical.clone();
        for (index, snapshot) in run.runtime_snapshots.iter_mut().enumerate() {
            snapshot.worker_count = counts[index % counts.len()];
        }
        let report =
            analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
        let suspect = executor_suspect(&report);
        assert_eq!(suspect.score, historical_score, "{expected}");
        assert_ne!(suspect.confidence, Confidence::High, "{expected}");
        assert!(suspect.evidence.iter().any(|e| e.contains(&format!(
            "Worker-count evidence is {expected}; legacy absolute-depth scoring was used"
        ))));
        assert!(suspect.confidence_notes.iter().any(|note| {
            note.contains(&format!("Ambiguous worker-count evidence ({expected})"))
        }));
    }
}

// TT-TEST: A03 primary
#[test]
fn missing_local_depth_remains_normalized_lower_bound() {
    let mut run = worker_test_run();
    run.runtime_snapshots.truncate(10);
    run.runtime_snapshots[0].global_queue_depth = Some(0);
    run.runtime_snapshots[0].local_queue_depth = None;
    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    let suspect = executor_suspect(&report);
    assert!(suspect
        .evidence
        .iter()
        .any(|e| e.contains("missing local queue depths were treated as zero")));
    assert!(suspect
        .evidence
        .iter()
        .any(|e| e.contains("worker_count=4")));
    assert!(!suspect
        .evidence
        .iter()
        .any(|e| e.contains("legacy absolute-depth scoring")));
    assert_ne!(suspect.confidence, Confidence::High);
}

// TT-TEST: support
#[test]
fn normalized_local_queue_signal_does_not_attribute_contention_to_zero_global_depth() {
    let mut run = worker_test_run();
    run.runtime_snapshots.truncate(10);
    for snapshot in &mut run.runtime_snapshots {
        snapshot.global_queue_depth = Some(0);
        snapshot.local_queue_depth = Some(64);
    }
    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    let suspect = executor_suspect(&report);
    assert!(suspect
        .evidence
        .iter()
        .any(|e| e.contains("16000 milli-tasks per worker")));
    assert!(!suspect.evidence.iter().any(|e| {
        e.contains("global queue depth p95 is 0") && e.contains("suggesting scheduler contention")
    }));
}

// TT-TEST: A03 secondary
#[test]
fn worker_count_enables_normalized_executor_scoring() {
    let mut historical = test_run();
    historical.requests = (0..20).map(sample_request).collect();
    historical.runtime_snapshots = vec![runtime_snapshot(Some(20), Some(0), Some(20)); 20];
    let historical_report = analyze_run(&historical, AnalyzeOptions::default())
        .expect("analyzer options should be valid");
    let mut normalized = historical.clone();
    for snapshot in &mut normalized.runtime_snapshots {
        snapshot.worker_count = Some(4);
    }
    let normalized_report = analyze_run(&normalized, AnalyzeOptions::default())
        .expect("analyzer options should be valid");
    let historical_executor = std::iter::once(&historical_report.primary_suspect)
        .chain(&historical_report.secondary_suspects)
        .find(|s| s.kind == DiagnosisKind::ExecutorPressure)
        .unwrap();
    let normalized_executor = std::iter::once(&normalized_report.primary_suspect)
        .chain(&normalized_report.secondary_suspects)
        .find(|s| s.kind == DiagnosisKind::ExecutorPressure)
        .unwrap();
    assert_eq!(historical_executor.score, 39);
    assert_eq!(normalized_executor.score, 74);
    assert!(normalized_executor
        .evidence
        .iter()
        .any(|e| e.contains("5000 milli-tasks per worker")));
}

fn executor_arithmetic_run(samples: usize, global: u64, local: u64, alive: u64) -> Run {
    let mut run = test_run();
    run.requests = (0..20).map(sample_request).collect();
    run.runtime_snapshots = (0..samples)
        .map(|_| {
            let mut snapshot = runtime_snapshot(Some(global), Some(local), Some(0));
            snapshot.alive_tasks = Some(alive);
            snapshot
        })
        .collect();
    run
}

// TT-TEST: A03 primary
#[test]
fn historical_executor_arithmetic_boundaries_are_exact() {
    let no_signal = executor_arithmetic_run(20, 0, 60, 400);
    let report = analyze_run(&no_signal, AnalyzeOptions::default())
        .expect("analyzer options should be valid");
    assert!(std::iter::once(&report.primary_suspect)
        .chain(&report.secondary_suspects)
        .all(|suspect| suspect.kind != DiagnosisKind::ExecutorPressure));

    for (samples, expected) in [
        (7, 39),
        (8, 39),
        (19, 39),
        (20, 39),
        (39, 39),
        (40, 39),
        (99, 39),
        (100, 39),
    ] {
        let report = analyze_run(
            &executor_arithmetic_run(samples, 20, 0, 0),
            AnalyzeOptions::default(),
        )
        .expect("analyzer options should be valid");
        assert_eq!(
            executor_suspect(&report).score,
            expected,
            "samples={samples}"
        );
    }

    for (local, expected) in [(59, 48), (60, 49)] {
        let report = analyze_run(
            &executor_arithmetic_run(20, 20, local, 0),
            AnalyzeOptions::default(),
        )
        .expect("analyzer options should be valid");
        assert_eq!(executor_suspect(&report).score, expected, "local={local}");
    }
    for (alive, expected) in [(399, 48), (400, 49)] {
        let report = analyze_run(
            &executor_arithmetic_run(20, 20, 0, alive),
            AnalyzeOptions::default(),
        )
        .expect("analyzer options should be valid");
        assert_eq!(executor_suspect(&report).score, expected, "alive={alive}");
    }
}

// TT-TEST: support
#[test]
fn historical_clean_extreme_is_support_invariant_and_absence_has_no_worker_cap() {
    for samples in [1, 29, 30, 64] {
        let mut run = executor_arithmetic_run(samples, 150, 60, 400);
        run.inflight = vec![
            InFlightSnapshot {
                at_unix_ms: 1,
                at_run_us: Some(1),
                gauge: "executor".into(),
                count: 1,
            },
            InFlightSnapshot {
                at_unix_ms: 2,
                at_run_us: Some(2),
                gauge: "executor".into(),
                count: 2,
            },
        ];
        let report =
            analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
        let suspect = executor_suspect(&report);
        assert_eq!(suspect.score, 95, "samples={samples}");
        assert_eq!(suspect.confidence, super::confidence::maturity_cap(samples));
        assert!(suspect
            .confidence_notes
            .iter()
            .all(|note| !note.contains("worker-count")));
    }
}

// TT-TEST: support
#[test]
fn normalized_executor_score_ignores_alive_tasks_and_queue_redistribution() {
    let mut baseline = executor_arithmetic_run(20, 32, 32, 1);
    for snapshot in &mut baseline.runtime_snapshots {
        snapshot.worker_count = Some(4);
    }
    let baseline_score = executor_suspect(
        &analyze_run(&baseline, AnalyzeOptions::default())
            .expect("analyzer options should be valid"),
    )
    .score;

    let mut changed_alive = baseline.clone();
    for snapshot in &mut changed_alive.runtime_snapshots {
        snapshot.alive_tasks = Some(u64::MAX);
    }
    let alive_score = executor_suspect(
        &analyze_run(&changed_alive, AnalyzeOptions::default())
            .expect("analyzer options should be valid"),
    )
    .score;

    let mut redistributed = baseline.clone();
    for snapshot in &mut redistributed.runtime_snapshots {
        snapshot.global_queue_depth = Some(16);
        snapshot.local_queue_depth = Some(48);
    }
    let redistributed_score = executor_suspect(
        &analyze_run(&redistributed, AnalyzeOptions::default())
            .expect("analyzer options should be valid"),
    )
    .score;
    assert_eq!(baseline_score, alive_score);
    assert_eq!(baseline_score, redistributed_score);
}

// TT-TEST: A04 primary
#[test]
fn worker_confidence_limits_compose_with_existing_caps_without_duplicate_notes() {
    let mut ambiguous = executor_arithmetic_run(20, 140, 60, 400);
    ambiguous.runtime_snapshots[0].worker_count = Some(4);
    ambiguous.truncation.limits_hit = true;
    ambiguous.truncation.dropped_runtime_snapshots = 1;
    let report = analyze_run(&ambiguous, AnalyzeOptions::default())
        .expect("analyzer options should be valid");
    let suspect = executor_suspect(&report);
    assert_eq!(suspect.confidence, Confidence::Medium);
    for fragment in [
        "Ambiguous worker-count evidence",
        "Capture truncation caps confidence",
    ] {
        assert_eq!(
            suspect
                .confidence_notes
                .iter()
                .filter(|note| note.contains(fragment))
                .count(),
            1,
            "{fragment}"
        );
    }

    let mut lower_bound = executor_arithmetic_run(20, 64, 0, 0);
    lower_bound.requests.clear();
    for snapshot in &mut lower_bound.runtime_snapshots {
        snapshot.worker_count = Some(4);
        snapshot.local_queue_depth = None;
    }
    let report = analyze_run(&lower_bound, AnalyzeOptions::default())
        .expect("analyzer options should be valid");
    let suspect = executor_suspect(&report);
    assert_eq!(suspect.confidence, Confidence::Low);
    for fragment in ["Missing local queue depth", "Low completed-request count"] {
        assert_eq!(
            suspect
                .confidence_notes
                .iter()
                .filter(|note| note.contains(fragment))
                .count(),
            1,
            "{fragment}"
        );
    }
}

// TT-TEST: A02 primary
#[test]
fn clear_blocking_pressure_selects_blocking_pool() {
    let mut run = test_run();
    run.requests = (0..20).map(sample_request).collect();
    run.runtime_snapshots = vec![runtime_snapshot(Some(0), Some(0), Some(24)); 40];

    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");

    assert_eq!(
        report.primary_suspect.kind,
        DiagnosisKind::BlockingPoolPressure
    );
    assert!(report
        .primary_suspect
        .evidence
        .iter()
        .any(|evidence| evidence.contains("Blocking queue depth")));
}

// TT-TEST: A02 primary
#[test]
fn clear_scheduler_pressure_selects_executor_pressure() {
    let mut run = test_run();
    run.requests = (0..20).map(sample_request).collect();
    run.runtime_snapshots = vec![runtime_snapshot(Some(140), Some(12), Some(0)); 40];

    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");

    assert_eq!(report.primary_suspect.kind, DiagnosisKind::ExecutorPressure);
    assert!(report
        .primary_suspect
        .evidence
        .iter()
        .any(|evidence| evidence.contains("Runtime global queue depth")));
}

// TT-TEST: A02 primary
#[test]
fn downstream_stage_tie_break_is_deterministic() {
    let mut run = test_run();
    run.stages = vec![
        StageEvent {
            request_id: "req-1".to_owned(),
            stage: "stage_a".to_owned(),
            relations: tailtriage_core::StageRelations::default(),
            started_at_unix_ms: 1,
            started_at_run_us: None,
            finished_at_unix_ms: 2,
            finished_at_run_us: None,
            latency_us: 300,
            success: true,
            completed: true,
        },
        StageEvent {
            request_id: "req-2".to_owned(),
            stage: "stage_a".to_owned(),
            relations: tailtriage_core::StageRelations::default(),
            started_at_unix_ms: 2,
            started_at_run_us: None,
            finished_at_unix_ms: 3,
            finished_at_run_us: None,
            latency_us: 300,
            success: true,
            completed: true,
        },
        StageEvent {
            request_id: "req-3".to_owned(),
            stage: "stage_a".to_owned(),
            relations: tailtriage_core::StageRelations::default(),
            started_at_unix_ms: 3,
            started_at_run_us: None,
            finished_at_unix_ms: 4,
            finished_at_run_us: None,
            latency_us: 300,
            success: true,
            completed: true,
        },
        StageEvent {
            request_id: "req-1".to_owned(),
            stage: "stage_b".to_owned(),
            relations: tailtriage_core::StageRelations::default(),
            started_at_unix_ms: 1,
            started_at_run_us: None,
            finished_at_unix_ms: 2,
            finished_at_run_us: None,
            latency_us: 300,
            success: true,
            completed: true,
        },
        StageEvent {
            request_id: "req-2".to_owned(),
            stage: "stage_b".to_owned(),
            relations: tailtriage_core::StageRelations::default(),
            started_at_unix_ms: 2,
            started_at_run_us: None,
            finished_at_unix_ms: 3,
            finished_at_run_us: None,
            latency_us: 300,
            success: true,
            completed: true,
        },
        StageEvent {
            request_id: "req-3".to_owned(),
            stage: "stage_b".to_owned(),
            relations: tailtriage_core::StageRelations::default(),
            started_at_unix_ms: 3,
            started_at_run_us: None,
            finished_at_unix_ms: 4,
            finished_at_run_us: None,
            latency_us: 300,
            success: true,
            completed: true,
        },
    ];

    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    assert_eq!(
        report.primary_suspect.kind,
        DiagnosisKind::DownstreamStageDominance
    );
    assert!(
        report.primary_suspect.evidence[0].contains("stage_a"),
        "expected deterministic stage tie-breaker to choose stage_a, got {:?}",
        report.primary_suspect.evidence
    );
}

// TT-TEST: support
#[test]
fn inflight_trend_is_none_for_empty_series() {
    assert!(super::dominant_inflight_trend(&[]).is_none());
}

// TT-TEST: support
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
    assert_eq!(trend.growth_delta, Some(0));
}

// TT-TEST: support
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
    assert_eq!(trend.growth_delta, Some(5));
    assert_eq!(trend.growth_per_sec_milli, None);
}

fn inflight(
    gauge: &str,
    unix: u64,
    run_us: Option<u64>,
    count: u64,
) -> tailtriage_core::InFlightSnapshot {
    tailtriage_core::InFlightSnapshot {
        gauge: gauge.to_owned(),
        at_unix_ms: unix,
        at_run_us: run_us,
        count,
    }
}

// TT-TEST: support
#[test]
fn latest_active_inflight_episode_outranks_closed_historical_spike() {
    let trend = super::dominant_inflight_trend(&[
        inflight("old", 1, Some(1_000_000), 1),
        inflight("old", 2, Some(2_000_000), 20),
        inflight("old", 3, Some(3_000_000), 0),
        inflight("current", 4, Some(4_000_000), 1),
        inflight("current", 5, Some(5_000_000), 2),
        inflight("current", 6, Some(6_000_000), 4),
    ])
    .unwrap();
    assert_eq!(
        trend,
        InflightTrend {
            gauge: "current".into(),
            sample_count: 3,
            peak_count: 4,
            p95_count: 4,
            growth_delta: Some(3),
            growth_per_sec_milli: Some(1500)
        }
    );
}

// TT-TEST: support
#[test]
fn reused_inflight_label_uses_only_latest_episode() {
    let counts = [1, 9, 0, 2, 3, 5];
    let samples = counts
        .into_iter()
        .enumerate()
        .map(|(i, count)| inflight("reuse", i as u64, Some(i as u64 * 1_000_000), count))
        .collect::<Vec<_>>();
    let trend = super::dominant_inflight_trend(&samples).unwrap();
    assert_eq!(
        (
            trend.sample_count,
            trend.peak_count,
            trend.p95_count,
            trend.growth_delta,
            trend.growth_per_sec_milli
        ),
        (3, 5, 5, Some(3), Some(1500))
    );
}

// TT-TEST: support
#[test]
fn leading_and_consecutive_zero_snapshots_do_not_create_episodes() {
    let samples = [0, 0, 2, 4, 0, 0, 3, 0]
        .into_iter()
        .enumerate()
        .map(|(i, count)| inflight("g", i as u64, Some(i as u64), count))
        .collect::<Vec<_>>();
    let trend = super::dominant_inflight_trend(&samples).unwrap();
    assert_eq!(
        (trend.sample_count, trend.peak_count, trend.growth_delta),
        (2, 3, Some(-3))
    );
}

// TT-TEST: support
#[test]
fn all_zero_inflight_label_has_no_candidate() {
    assert!(super::dominant_inflight_trend(&[
        inflight("zero", 1, Some(1), 0),
        inflight("zero", 2, Some(2), 0)
    ])
    .is_none());
}

// TT-TEST: support
#[test]
fn run_relative_time_orders_inflight_samples_before_wall_clock() {
    let trend = super::dominant_inflight_trend(&[
        inflight("g", 30, Some(1), 2),
        inflight("g", 10, Some(3), 7),
        inflight("g", 20, Some(2), 4),
    ])
    .unwrap();
    assert_eq!(
        (trend.growth_delta, trend.growth_per_sec_milli),
        (Some(5), Some(2_500_000_000))
    );
}

// TT-TEST: support
#[test]
fn inflight_wall_clock_fallback_is_deterministic_without_rate() {
    let trend = super::dominant_inflight_trend(&[
        inflight("g", 30, Some(1), 5),
        inflight("g", 10, None, 2),
    ])
    .unwrap();
    assert_eq!(
        (trend.growth_delta, trend.growth_per_sec_milli),
        (Some(3), None)
    );
}

// TT-TEST: support
#[test]
fn distinct_timestamp_reordering_preserves_inflight_selection() {
    let a = vec![
        inflight("g", 3, Some(3), 5),
        inflight("g", 1, Some(1), 1),
        inflight("g", 2, Some(2), 3),
    ];
    let b = vec![a[1].clone(), a[0].clone(), a[2].clone()];
    assert_eq!(
        super::dominant_inflight_trend(&a),
        super::dominant_inflight_trend(&b)
    );
}

// TT-TEST: support
#[test]
fn equal_timestamp_inflight_order_uses_original_input_index() {
    let forward = super::dominant_inflight_trend(&[
        inflight("g", 1, Some(1), 2),
        inflight("g", 1, Some(1), 5),
    ])
    .unwrap();
    let reverse = super::dominant_inflight_trend(&[
        inflight("g", 1, Some(1), 5),
        inflight("g", 1, Some(1), 2),
    ])
    .unwrap();
    assert_eq!(
        (forward.growth_delta, reverse.growth_delta),
        (Some(3), Some(-3))
    );
    assert_eq!(forward.growth_per_sec_milli, None);
}

// TT-TEST: support
#[test]
fn one_sample_inflight_episode_is_unknown_and_adds_no_bonus() {
    let trend = super::dominant_inflight_trend(&[inflight("single", 1, Some(1), 7)]).unwrap();
    assert_eq!(
        trend,
        InflightTrend {
            gauge: "single".into(),
            sample_count: 1,
            peak_count: 7,
            p95_count: 7,
            growth_delta: None,
            growth_per_sec_milli: None
        }
    );
}

fn assert_dominant_inflight(
    samples: &[tailtriage_core::InFlightSnapshot],
    expected: InflightTrend,
) {
    assert_eq!(super::dominant_inflight_trend(samples), Some(expected));
}

// TT-TEST: support
#[test]
fn inflight_candidate_order_prefers_active_over_closed() {
    assert_dominant_inflight(
        &[
            inflight("closed", 1, Some(1), 1),
            inflight("closed", 2, Some(2), 20),
            inflight("closed", 3, Some(3), 0),
            inflight("active", 4, Some(4), 5),
            inflight("active", 5, Some(5), 5),
        ],
        InflightTrend {
            gauge: "active".into(),
            sample_count: 2,
            peak_count: 5,
            p95_count: 5,
            growth_delta: Some(0),
            growth_per_sec_milli: Some(0),
        },
    );
}

// TT-TEST: support
#[test]
fn inflight_candidate_order_prefers_positive_over_unknown() {
    let samples = [
        inflight("positive", 1, Some(1), 1),
        inflight("positive", 2, Some(1_000_001), 2),
        inflight("unknown", 3, Some(3), 50),
    ];
    assert_dominant_inflight(
        &samples,
        InflightTrend {
            gauge: "positive".into(),
            sample_count: 2,
            peak_count: 2,
            p95_count: 2,
            growth_delta: Some(1),
            growth_per_sec_milli: Some(1000),
        },
    );
}

// TT-TEST: support
#[test]
fn inflight_candidate_order_prefers_positive_over_non_growing() {
    for (label, counts) in [("flat", [20, 20]), ("declining", [20, 10])] {
        let samples = [
            inflight("positive", 1, Some(1), 1),
            inflight("positive", 2, Some(1_000_001), 2),
            inflight(label, 1, Some(1), counts[0]),
            inflight(label, 2, Some(1_000_001), counts[1]),
        ];
        assert_dominant_inflight(
            &samples,
            InflightTrend {
                gauge: "positive".into(),
                sample_count: 2,
                peak_count: 2,
                p95_count: 2,
                growth_delta: Some(1),
                growth_per_sec_milli: Some(1000),
            },
        );
    }
}

// TT-TEST: support
#[test]
fn inflight_candidate_order_prefers_larger_positive_delta() {
    let samples = [
        inflight("larger-delta", 1, Some(1), 1),
        inflight("larger-delta", 2, Some(1_000_001), 6),
        inflight("larger-peak", 1, Some(1), 100),
        inflight("larger-peak", 2, Some(1_000_001), 103),
    ];
    assert_dominant_inflight(
        &samples,
        InflightTrend {
            gauge: "larger-delta".into(),
            sample_count: 2,
            peak_count: 6,
            p95_count: 6,
            growth_delta: Some(5),
            growth_per_sec_milli: Some(5000),
        },
    );
}

// TT-TEST: support
#[test]
fn inflight_candidate_order_prefers_available_positive_rate() {
    assert_dominant_inflight(
        &[
            inflight("no-rate", 1, None, 1),
            inflight("no-rate", 2, None, 4),
            inflight("rate", 1, Some(1), 1),
            inflight("rate", 2, Some(2_000_001), 4),
        ],
        InflightTrend {
            gauge: "rate".into(),
            sample_count: 2,
            peak_count: 4,
            p95_count: 4,
            growth_delta: Some(3),
            growth_per_sec_milli: Some(1500),
        },
    );
}

// TT-TEST: support
#[test]
fn inflight_candidate_order_prefers_larger_positive_rate() {
    let samples = [
        inflight("fast", 1, Some(1), 1),
        inflight("fast", 2, Some(1_000_001), 4),
        inflight("slow-large-peak", 1, Some(1), 100),
        inflight("slow-large-peak", 2, Some(2_000_001), 103),
    ];
    assert_dominant_inflight(
        &samples,
        InflightTrend {
            gauge: "fast".into(),
            sample_count: 2,
            peak_count: 4,
            p95_count: 4,
            growth_delta: Some(3),
            growth_per_sec_milli: Some(3000),
        },
    );
}

// TT-TEST: support
#[test]
fn inflight_candidate_order_prefers_larger_p95() {
    let mut samples = (1..=21)
        .map(|time| inflight("high-p95", time, Some(time), 8))
        .collect::<Vec<_>>();

    samples.push(inflight("low-p95-high-peak", 1, Some(1), 100));
    samples.extend((2..=21).map(|time| inflight("low-p95-high-peak", time, Some(time), 1)));

    assert_dominant_inflight(
        &samples,
        InflightTrend {
            gauge: "high-p95".into(),
            sample_count: 21,
            peak_count: 8,
            p95_count: 8,
            growth_delta: Some(0),
            growth_per_sec_milli: Some(0),
        },
    );
}

// TT-TEST: support
#[test]
fn inflight_candidate_order_prefers_larger_peak() {
    let mut samples = vec![inflight("larger-peak", 1, Some(1), 10)];
    samples.extend((2..=21).map(|time| inflight("larger-peak", time, Some(time), 1)));
    samples.extend((1..=21).map(|time| inflight("smaller-peak", time, Some(time), 1)));
    assert_dominant_inflight(
        &samples,
        InflightTrend {
            gauge: "larger-peak".into(),
            sample_count: 21,
            peak_count: 10,
            p95_count: 1,
            growth_delta: Some(-9),
            growth_per_sec_milli: Some(-450_000_000),
        },
    );
}

// TT-TEST: support
#[test]
fn inflight_candidate_order_uses_lexical_label_last() {
    assert_dominant_inflight(
        &[
            inflight("z", 1, Some(1), 2),
            inflight("z", 2, Some(2), 2),
            inflight("a", 1, Some(1), 2),
            inflight("a", 2, Some(2), 2),
        ],
        InflightTrend {
            gauge: "a".into(),
            sample_count: 2,
            peak_count: 2,
            p95_count: 2,
            growth_delta: Some(0),
            growth_per_sec_milli: Some(0),
        },
    );
}

// TT-TEST: support
#[test]
fn truncated_inflight_history_does_not_infer_missing_zero() {
    let mut run = test_run();
    run.inflight = vec![
        inflight("truncated", 1, Some(1), 2),
        inflight("truncated", 2, Some(2), 3),
    ];
    run.truncation.dropped_inflight_snapshots = 1;
    let trend = analyze_run(&run, AnalyzeOptions::default())
        .expect("analyzer options should be valid")
        .inflight_trend
        .unwrap();
    assert_eq!((trend.sample_count, trend.growth_delta), (2, Some(1)));
}

// TT-TEST: support
#[test]
fn inflight_trend_json_distinguishes_unavailable_from_flat() {
    let value = serde_json::to_value(InflightTrend {
        gauge: "g".into(),
        sample_count: 3,
        peak_count: 4,
        p95_count: 4,
        growth_delta: Some(3),
        growth_per_sec_milli: Some(1500),
    })
    .unwrap();
    assert_eq!(
        value,
        serde_json::json!({
            "gauge": "g",
            "sample_count": 3,
            "peak_count": 4,
            "p95_count": 4,
            "growth_delta": 3,
            "growth_per_sec_milli": 1500
        })
    );
    assert_eq!(
        serde_json::to_value(InflightTrend {
            gauge: "g".into(),
            sample_count: 1,
            peak_count: 4,
            p95_count: 4,
            growth_delta: None,
            growth_per_sec_milli: None,
        })
        .unwrap(),
        serde_json::json!({
            "gauge": "g",
            "sample_count": 1,
            "peak_count": 4,
            "p95_count": 4,
            "growth_delta": null,
            "growth_per_sec_milli": null
        })
    );
    assert_eq!(
        serde_json::to_value(InflightTrend {
            gauge: "flat".into(),
            sample_count: 2,
            peak_count: 4,
            p95_count: 4,
            growth_delta: Some(0),
            growth_per_sec_milli: None,
        })
        .unwrap()["growth_delta"],
        serde_json::json!(0)
    );
}

fn suspect_score(report: &Report, kind: &DiagnosisKind) -> u8 {
    std::iter::once(&report.primary_suspect)
        .chain(&report.secondary_suspects)
        .find(|s| &s.kind == kind)
        .unwrap()
        .score
}

fn queue_bonus_run(inflight_samples: Vec<tailtriage_core::InFlightSnapshot>) -> Run {
    let mut run = option_run_twenty_requests();
    run.queues = (1..=20)
        .map(|i| QueueEvent {
            request_id: format!("req-{i}"),
            queue: "q".into(),
            waited_from_unix_ms: i,
            waited_from_run_us: None,
            waited_until_unix_ms: i + 1,
            waited_until_run_us: None,
            wait_us: 700,
            depth_at_start: Some(1),
            completed: true,
        })
        .collect();
    run.inflight = inflight_samples;
    run
}

fn suspect_evidence<'a>(report: &'a Report, kind: &DiagnosisKind) -> &'a [String] {
    &std::iter::once(&report.primary_suspect)
        .chain(&report.secondary_suspects)
        .find(|suspect| &suspect.kind == kind)
        .expect("expected suspect")
        .evidence
}

// TT-TEST: support
#[test]
fn closed_historical_inflight_spike_adds_no_bonus_when_active_flat_episode_wins() {
    let baseline = analyze_run(&queue_bonus_run(vec![]), AnalyzeOptions::default())
        .expect("analyzer options should be valid");
    let report = analyze_run(
        &queue_bonus_run(vec![
            inflight("closed", 1, Some(1), 1),
            inflight("closed", 2, Some(2), 20),
            inflight("closed", 3, Some(3), 0),
            inflight("active", 4, Some(4), 5),
            inflight("active", 5, Some(5), 5),
        ]),
        AnalyzeOptions::default(),
    )
    .expect("analyzer options should be valid");
    assert_eq!(
        report.inflight_trend,
        Some(InflightTrend {
            gauge: "active".into(),
            sample_count: 2,
            peak_count: 5,
            p95_count: 5,
            growth_delta: Some(0),
            growth_per_sec_milli: Some(0),
        })
    );
    assert_eq!(
        suspect_score(&report, &DiagnosisKind::ApplicationQueuePressure),
        suspect_score(&baseline, &DiagnosisKind::ApplicationQueuePressure)
    );
    assert!(
        !suspect_evidence(&report, &DiagnosisKind::ApplicationQueuePressure)
            .iter()
            .any(|evidence| evidence.contains("grew by"))
    );
}

// TT-TEST: support
#[test]
fn inflight_growth_evidence_states_unix_fallback_without_rate() {
    let baseline = analyze_run(&queue_bonus_run(vec![]), AnalyzeOptions::default())
        .expect("analyzer options should be valid");
    let report = analyze_run(
        &queue_bonus_run(vec![
            inflight("fallback", 1, Some(1), 1),
            inflight("fallback", 2, None, 3),
        ]),
        AnalyzeOptions::default(),
    )
    .expect("analyzer options should be valid");
    assert_eq!(
        report.inflight_trend.as_ref().unwrap().growth_per_sec_milli,
        None
    );
    assert_eq!(
        suspect_score(&report, &DiagnosisKind::ApplicationQueuePressure)
            - suspect_score(&baseline, &DiagnosisKind::ApplicationQueuePressure),
        5
    );
    let evidence = suspect_evidence(&report, &DiagnosisKind::ApplicationQueuePressure).join(" ");
    assert!(evidence.contains("latest active episode grew by 2"));
    assert!(evidence.contains("ordering used Unix-ms fallback"));
    assert!(evidence.contains("no precise growth rate was derived"));
    assert!(!evidence.contains("rate="));
}

// TT-TEST: support
#[test]
fn inflight_growth_evidence_states_unavailable_run_relative_rate() {
    let baseline = analyze_run(&queue_bonus_run(vec![]), AnalyzeOptions::default())
        .expect("analyzer options should be valid");
    let report = analyze_run(
        &queue_bonus_run(vec![
            inflight("zero-elapsed", 1, Some(10), 1),
            inflight("zero-elapsed", 2, Some(10), 3),
        ]),
        AnalyzeOptions::default(),
    )
    .expect("analyzer options should be valid");
    assert_eq!(
        report.inflight_trend.as_ref().unwrap().growth_per_sec_milli,
        None
    );
    assert_eq!(
        suspect_score(&report, &DiagnosisKind::ApplicationQueuePressure)
            - suspect_score(&baseline, &DiagnosisKind::ApplicationQueuePressure),
        5
    );
    let evidence = suspect_evidence(&report, &DiagnosisKind::ApplicationQueuePressure).join(" ");
    assert!(evidence.contains("latest active episode grew by 2"));
    assert!(evidence.contains("precise run-relative growth rate is unavailable"));
    assert!(!evidence.contains("Unix-ms fallback"));
}

// TT-TEST: A02 primary
#[test]
fn queue_inflight_growth_bonus_remains_exactly_five() {
    let baseline = analyze_run(&queue_bonus_run(vec![]), AnalyzeOptions::default())
        .expect("analyzer options should be valid");
    let growing = analyze_run(
        &queue_bonus_run(vec![
            inflight("g", 1, Some(1), 1),
            inflight("g", 2, Some(1_000_001), 3),
        ]),
        AnalyzeOptions::default(),
    )
    .expect("analyzer options should be valid");
    assert_eq!(
        suspect_score(&growing, &DiagnosisKind::ApplicationQueuePressure)
            - suspect_score(&baseline, &DiagnosisKind::ApplicationQueuePressure),
        5
    );
}

// TT-TEST: support
#[test]
fn executor_inflight_growth_bonus_remains_exactly_four() {
    let mut baseline = option_run_twenty_requests();
    baseline.runtime_snapshots = vec![runtime_snapshot(Some(20), Some(0), Some(20)); 20];
    let mut growing = baseline.clone();
    growing.inflight = vec![
        inflight("g", 1, Some(1), 1),
        inflight("g", 2, Some(1_000_001), 3),
    ];
    let without = analyze_run(&baseline, AnalyzeOptions::default())
        .expect("analyzer options should be valid");
    let with =
        analyze_run(&growing, AnalyzeOptions::default()).expect("analyzer options should be valid");
    assert_eq!(
        suspect_score(&with, &DiagnosisKind::ExecutorPressure)
            - suspect_score(&without, &DiagnosisKind::ExecutorPressure),
        4
    );
    assert!(with
        .primary_suspect
        .evidence
        .iter()
        .chain(with.secondary_suspects.iter().flat_map(|s| &s.evidence))
        .any(|e| e.contains("latest active episode") && e.contains("run-relative rate=2000")));
}

// TT-TEST: support
#[test]
fn declining_inflight_episode_adds_no_growth_bonus() {
    let baseline = analyze_run(&queue_bonus_run(vec![]), AnalyzeOptions::default())
        .expect("analyzer options should be valid");
    for samples in [
        vec![inflight("g", 1, Some(1), 7)],
        vec![inflight("g", 1, Some(1), 3), inflight("g", 2, Some(2), 3)],
        vec![inflight("g", 1, Some(1), 3), inflight("g", 2, Some(2), 1)],
    ] {
        let report = analyze_run(&queue_bonus_run(samples), AnalyzeOptions::default())
            .expect("analyzer options should be valid");
        assert_eq!(
            suspect_score(&report, &DiagnosisKind::ApplicationQueuePressure),
            suspect_score(&baseline, &DiagnosisKind::ApplicationQueuePressure)
        );
        assert!(!report
            .primary_suspect
            .evidence
            .iter()
            .any(|e| e.contains(" grew by ")));
    }
}

// TT-TEST: support
#[test]
fn render_text_uses_growth_delta_as_direction_source_of_truth() {
    let mut report = analyze_run(
        &queue_bonus_run(vec![inflight("single", 1, Some(1), 7)]),
        AnalyzeOptions::default(),
    )
    .expect("analyzer options should be valid");
    report.inflight_trend.as_mut().unwrap().sample_count = 2;
    let text = render_text(&report);
    assert!(text.contains("direction unknown") && text.contains("precise rate unavailable"));
    assert!(!text.contains("net growth +0"));
    let json: serde_json::Value = serde_json::from_str(&render_json(&report).unwrap()).unwrap();
    assert_eq!(
        json["inflight_trend"]["growth_delta"],
        serde_json::Value::Null
    );

    let trend = report.inflight_trend.as_mut().unwrap();
    trend.sample_count = 1;
    trend.growth_delta = Some(0);
    let text = render_text(&report);
    assert!(text.contains("net growth +0"));
    assert!(!text.contains("direction unknown"));
    let json: serde_json::Value = serde_json::from_str(&render_json(&report).unwrap()).unwrap();
    assert_eq!(json["inflight_trend"]["growth_delta"], 0);
}

// TT-TEST: S04 primary
#[test]
fn render_text_escapes_artifact_controls_without_changing_report_json() {
    let mut report = analyze_run(
        &queue_bonus_run(vec![inflight("hostile\n\u{1b}[31m", 1, Some(1), 7)]),
        AnalyzeOptions::default(),
    )
    .expect("analyzer options should be valid");
    report.warnings.push("first\nforged\u{7}".to_owned());

    let text = render_text(&report);
    assert!(text.contains("gauge 'hostile\\n\\u{1b}[31m'"));
    assert!(text.contains("- first\\nforged\\u{7}"));
    assert!(!text.lines().any(|line| line == "forged"));

    let json = render_json(&report).expect("report should serialize");
    let decoded: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(decoded["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|warning| warning.as_str() == Some("first\nforged\u{7}")));
    assert_eq!(decoded["inflight_trend"]["gauge"], "hostile\n\u{1b}[31m");
}

// TT-TEST: support
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
            growth_delta: Some(5),
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
            kind: DiagnosisKind::ApplicationQueuePressure,
            score: 90,
            confidence: Confidence::High,
            evidence: vec!["queue wait high".to_owned()],
            next_checks: vec!["check queue policy".to_owned()],
            confidence_notes: Vec::new(),
        },
        secondary_suspects: Vec::new(),
        related_groups: Vec::new(),
        route_breakdowns: Vec::new(),
        temporal_segments: Vec::new(),
        analyzer_config: None,
    };

    let text = render_text(&report);
    assert!(text.contains("Inflight latest activity episode: gauge 'queue_inflight'"));
    assert!(text.contains("samples 4"));
    assert!(text.contains("peak 8"));
    assert!(text.contains("p95 7"));
    assert!(text.contains("net growth +5"));
    assert!(text.contains("run-relative rate 2500 milli-counts/sec"));
    assert!(text.contains("Request time at p95: queue 10.0%, non-queue service 90.0%"));
}

// TT-TEST: support
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
        related_groups: Vec::new(),
        route_breakdowns: Vec::new(),
        temporal_segments: Vec::new(),
        analyzer_config: None,
    };

    let text = render_text(&report);
    assert!(text.contains("Inflight trend: none"));
    assert!(text.contains("Warnings:"));
    assert!(text.contains("- Capture truncated requests."));
}

// TT-TEST: A07 primary
#[test]
fn analyze_run_emits_truncation_warnings() {
    let mut run = test_run();
    run.truncation.dropped_requests = 2;
    run.truncation.dropped_runtime_snapshots = 1;
    run.truncation.limits_hit = true;

    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
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

// TT-TEST: support
#[test]
fn low_request_count_warning_appears() {
    let report = analyze_run(&test_run(), AnalyzeOptions::default())
        .expect("analyzer options should be valid");
    assert!(report
        .warnings
        .iter()
        .any(|w| w.contains("Low completed-request count")));
}

// TT-TEST: support
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
            completed: true,
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
            completed: true,
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
            completed: true,
        },
    ];
    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    assert_eq!(
        report.primary_suspect.kind,
        DiagnosisKind::ApplicationQueuePressure
    );
    assert!(!report
        .warnings
        .iter()
        .any(|w| w.contains("No runtime snapshots captured")));
}

// TT-TEST: support
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
            completed: true,
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
            completed: true,
        },
    ];

    let default_report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    assert_eq!(
        default_report.primary_suspect.kind,
        DiagnosisKind::ApplicationQueuePressure
    );
    assert!(default_report.primary_suspect.score >= 85);
    assert!(default_report.primary_suspect.score < 95);
    assert!(!default_report
        .warnings
        .iter()
        .any(|w| w.contains("No runtime snapshots captured")));
    assert!(default_report.analyzer_config.is_none());

    let strict_options = {
        let mut options = AnalyzeOptions::default();
        {
            let o = &mut options.confidence;
            o.high_score_threshold = 95;
        }
        options
    };
    let strict_report =
        analyze_run(&run, strict_options).expect("analyzer options should be valid");
    assert!(strict_report
        .warnings
        .iter()
        .any(|w| w.contains("No runtime snapshots captured")));
    assert!(strict_report.analyzer_config.is_some());
}

// TT-TEST: support
#[test]
fn runtime_warning_emitted_when_insufficient_evidence() {
    let report = analyze_run(&test_run(), AnalyzeOptions::default())
        .expect("analyzer options should be valid");
    assert!(report
        .warnings
        .iter()
        .any(|w| w.contains("No runtime snapshots captured")));
}

// TT-TEST: support
#[test]
fn typed_stage_relation_metadata_is_analyzer_inert() {
    let mut without_relation = test_run();
    without_relation.stages = without_relation
        .requests
        .iter()
        .map(|request| {
            StageEvent::new(
                request.request_id.clone(),
                "resize_image",
                request.started_at_unix_ms,
                request.finished_at_unix_ms,
                900,
                true,
            )
            .with_run_interval(None, None)
        })
        .collect();
    let mut related = without_relation.clone();
    for stage in &mut related.stages {
        stage.relations = StageRelations::from_relation(StageRelation::BlockingPool);
    }

    let normalized = normalize_run_permissive(&related);
    assert_eq!(normalized.run.stages.len(), related.stages.len());
    assert!(normalized
        .run
        .stages
        .iter()
        .all(|stage| stage.has_relation(StageRelation::BlockingPool)));

    let without_relation = analyze_run(&without_relation, AnalyzeOptions::default()).unwrap();
    let with_relation = analyze_run(&related, AnalyzeOptions::default()).unwrap();
    assert_eq!(with_relation, without_relation);
}

// TT-TEST: support
#[test]
fn typed_blocking_pool_relation_is_independent_of_stage_display_name() {
    let report = |name: &str, typed: bool| {
        let mut run = test_run();
        run.requests = (0..40)
            .map(|i| precise_request(&format!("r{i}"), 4_000))
            .collect();
        run.stages = run
            .requests
            .iter()
            .map(|request| {
                let mut stage =
                    precise_stage(&request.request_id, name, Some(0), Some(3_600), 3_600);
                if typed {
                    stage.relations = StageRelations::from_relation(StageRelation::BlockingPool);
                }
                stage
            })
            .collect();
        run.runtime_snapshots = vec![runtime_snapshot(Some(1), Some(1), Some(16)); 40];
        analyze_run(&run, AnalyzeOptions::default()).unwrap()
    };

    let alpha = report("resize_image", true);
    let beta = report("utterly_neutral_label", true);
    let disposition = |report: &Report| {
        std::iter::once(&report.primary_suspect)
            .chain(&report.secondary_suspects)
            .map(|suspect| {
                (
                    suspect.kind.clone(),
                    suspect.score,
                    suspect.confidence,
                    suspect.confidence_notes.clone(),
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(disposition(&alpha), disposition(&beta));
    assert_eq!(alpha.related_groups.len(), 1);
    assert_eq!(beta.related_groups.len(), 1);
    let mut alpha_group = alpha.related_groups[0].clone();
    let mut beta_group = beta.related_groups[0].clone();
    assert_eq!(alpha_group.relation, beta_group.relation);
    assert_eq!(alpha_group.representative, beta_group.representative);
    for member in &mut alpha_group.members {
        member.stage = None;
    }
    for member in &mut beta_group.members {
        member.stage = None;
    }
    assert_eq!(alpha_group, beta_group);

    let untyped = report("blocking_pool", false);
    assert!(untyped.related_groups.is_empty());
}

// TT-TEST: support
#[test]
fn downstream_beats_weak_blocking() {
    let mut run = test_run();
    run.stages = vec![
        StageEvent {
            request_id: "req-1".into(),
            stage: "db".into(),
            relations: tailtriage_core::StageRelations::default(),
            started_at_unix_ms: 1,
            started_at_run_us: None,
            finished_at_unix_ms: 2,
            finished_at_run_us: None,
            latency_us: 900,
            success: true,
            completed: true,
        },
        StageEvent {
            request_id: "req-2".into(),
            stage: "db".into(),
            relations: tailtriage_core::StageRelations::default(),
            started_at_unix_ms: 2,
            started_at_run_us: None,
            finished_at_unix_ms: 3,
            finished_at_run_us: None,
            latency_us: 900,
            success: true,
            completed: true,
        },
        StageEvent {
            request_id: "req-3".into(),
            stage: "db".into(),
            relations: tailtriage_core::StageRelations::default(),
            started_at_unix_ms: 3,
            started_at_run_us: None,
            finished_at_unix_ms: 4,
            finished_at_run_us: None,
            latency_us: 900,
            success: true,
            completed: true,
        },
    ];
    run.runtime_snapshots = vec![runtime_snapshot(Some(2), Some(1), Some(1)); 5];
    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    assert_eq!(
        report.primary_suspect.kind,
        DiagnosisKind::DownstreamStageDominance
    );
}

// TT-TEST: A01 primary
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
            completed: true,
        })
        .collect();
    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    assert_eq!(
        report.primary_suspect.kind,
        DiagnosisKind::ApplicationQueuePressure
    );
    assert!(report.primary_suspect.score >= 95);
}

// TT-TEST: support
#[test]
fn blocking_like_stage_name_has_no_relation_semantics() {
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
            relations: tailtriage_core::StageRelations::default(),
            started_at_unix_ms: 1,
            started_at_run_us: None,
            finished_at_unix_ms: 2,
            finished_at_run_us: None,
            latency_us: 3_900_000,
            success: true,
            completed: true,
        })
        .collect();
    run.runtime_snapshots = vec![runtime_snapshot(Some(1), Some(1), Some(240)); 80];
    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    assert_eq!(
        report.primary_suspect.kind,
        DiagnosisKind::DownstreamStageDominance
    );
    assert!(report
        .secondary_suspects
        .iter()
        .any(|s| s.kind == DiagnosisKind::BlockingPoolPressure));
    assert!(report.related_groups.is_empty());
}

// TT-TEST: support
#[test]
#[allow(clippy::too_many_lines)]
fn typed_blocking_relation_groups_real_stage_owned_evidence_without_changing_scores() {
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
            latency_us: 4_000,
            outcome: "ok".into(),
        })
        .collect();
    for request in &run.requests {
        for (name, latency) in [("neutral_alpha", 1_900), ("neutral_beta", 1_500)] {
            run.stages.push(StageEvent {
                request_id: request.request_id.clone(),
                stage: name.into(),
                relations: StageRelations::from_relation(StageRelation::BlockingPool),
                started_at_unix_ms: 1,
                started_at_run_us: None,
                finished_at_unix_ms: 2,
                finished_at_run_us: None,
                latency_us: latency,
                success: true,
                completed: true,
            });
        }
    }
    run.runtime_snapshots = vec![runtime_snapshot(Some(1), Some(1), Some(16)); 40];

    let mut untagged = run.clone();
    for stage in &mut untagged.stages {
        stage.relations = StageRelations::default();
    }
    let independent = analyze_run(&untagged, AnalyzeOptions::default()).unwrap();
    let related = analyze_run(&run, AnalyzeOptions::default()).unwrap();
    assert_eq!(related.related_groups.len(), 1);
    let group = &related.related_groups[0];
    assert_eq!(group.relation, StageRelation::BlockingPool);
    assert_eq!(group.members.len(), 3);
    assert_eq!(
        group.members[0].diagnosis,
        DiagnosisKind::BlockingPoolPressure
    );
    assert_eq!(group.members[0].relevant_support, 40);
    assert_eq!(
        group.members[0].measurement,
        RelatedEvidenceMeasurement::BlockingPool {
            usable_snapshots: 40,
            p95_depth: 16,
            peak_depth: 16,
            nonzero_share_permille: 1000,
        }
    );
    assert_eq!(
        group.members[1..]
            .iter()
            .map(|member| member.stage.as_deref().unwrap())
            .collect::<Vec<_>>(),
        vec!["neutral_alpha", "neutral_beta"]
    );
    let scores = |report: &Report| {
        std::iter::once(&report.primary_suspect)
            .chain(&report.secondary_suspects)
            .map(|suspect| (suspect.kind.clone(), suspect.score))
            .collect::<Vec<_>>()
    };
    let independent_scores = scores(&independent);
    let related_scores = scores(&related);
    assert_eq!(
        related_scores
            .iter()
            .find(|(kind, _)| kind == &group.representative)
            .unwrap()
            .1,
        independent_scores
            .iter()
            .find(|(kind, _)| kind == &group.representative)
            .unwrap()
            .1
    );
    assert_eq!(
        group.members[1].relevant_support, 40,
        "member support is stage-owned distinct-request support"
    );
    assert_eq!(
        group.members[2].relevant_support, 40,
        "each related stage remains separately represented"
    );
    assert_eq!(
        group.members[1].measurement,
        RelatedEvidenceMeasurement::DownstreamStage {
            tail_contribution_permille: 475,
            cumulative_contribution_permille: 475,
        }
    );
    assert_eq!(
        group.members[2].measurement,
        RelatedEvidenceMeasurement::DownstreamStage {
            tail_contribution_permille: 375,
            cumulative_contribution_permille: 375,
        }
    );
    assert_eq!(
        related
            .primary_suspect
            .confidence_notes
            .iter()
            .filter(|note| note.contains("ambiguity"))
            .count(),
        0,
        "a related pair cannot create ambiguity with itself"
    );
}

// TT-TEST: support
#[test]
fn blocking_relation_resolution_is_idempotent_and_never_combines_raw_scores() {
    let mut run = test_run();
    run.requests = (0..40)
        .map(|i| precise_request(&format!("r{i}"), 4_000))
        .collect();
    run.stages = run
        .requests
        .iter()
        .map(|request| {
            let mut stage = precise_stage(
                &request.request_id,
                "typed_work",
                Some(0),
                Some(3_600),
                3_600,
            );
            stage.relations = StageRelations::from_relation(StageRelation::BlockingPool);
            stage
        })
        .collect();
    run.queues = run
        .requests
        .iter()
        .map(|request| precise_queue(&request.request_id, 0, 2_000, 2_000))
        .collect();
    run.runtime_snapshots = vec![runtime_snapshot(Some(0), Some(0), Some(16)); 40];
    let options = AnalyzeOptions::default();
    let queue =
        super::scoring::queue_candidate_for_test(&run, &[2_000; 40], true, Some(4_000), &options)
            .unwrap();
    let blocking = super::scoring::blocking_pressure_suspect(&run, &options).unwrap();
    let downstream = super::scoring::downstream_stage_suspect(&run, &options).unwrap();
    let member_scores = [blocking.suspect.score, downstream.suspect.score];
    let unrelated_score = queue.suspect.score;
    let mut candidates = vec![queue, blocking, downstream];

    let first = super::relation::resolve_blocking_pool_group(&mut candidates, &run, &options);
    assert_eq!(first.len(), 1);
    assert_eq!(candidates.len(), 2);
    assert!(candidates
        .iter()
        .any(|candidate| candidate.suspect.kind == DiagnosisKind::ApplicationQueuePressure));
    let representative = candidates
        .iter()
        .find(|candidate| candidate.suspect.kind == first[0].representative)
        .unwrap();
    assert!(member_scores.contains(&representative.suspect.score));
    assert_ne!(
        representative.suspect.score,
        member_scores[0].saturating_add(member_scores[1])
    );
    assert_eq!(
        candidates
            .iter()
            .find(|candidate| candidate.suspect.kind == DiagnosisKind::ApplicationQueuePressure)
            .unwrap()
            .suspect
            .score,
        unrelated_score
    );

    let surviving = candidates
        .iter()
        .map(|candidate| (candidate.suspect.kind.clone(), candidate.suspect.score))
        .collect::<Vec<_>>();
    let second = super::relation::resolve_blocking_pool_group(&mut candidates, &run, &options);
    assert!(second.is_empty());
    assert_eq!(
        candidates
            .iter()
            .map(|candidate| (candidate.suspect.kind.clone(), candidate.suspect.score))
            .collect::<Vec<_>>(),
        surviving
    );
}

// TT-TEST: A06 primary
#[test]
#[allow(clippy::too_many_lines)]
fn related_representative_and_unrelated_candidate_are_ambiguity_peers() {
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
            latency_us: 4_000,
            outcome: "ok".into(),
        })
        .collect();
    for request in &run.requests {
        run.stages.push(StageEvent {
            request_id: request.request_id.clone(),
            stage: "typed_blocking_work".into(),
            relations: StageRelations::from_relation(StageRelation::BlockingPool),
            started_at_unix_ms: 1,
            started_at_run_us: None,
            finished_at_unix_ms: 2,
            finished_at_run_us: None,
            latency_us: 3_600,
            success: true,
            completed: true,
        });
        run.queues.push(QueueEvent {
            request_id: request.request_id.clone(),
            queue: "admission".into(),
            waited_from_unix_ms: 1,
            waited_from_run_us: None,
            waited_until_unix_ms: 2,
            waited_until_run_us: None,
            wait_us: 3_600,
            depth_at_start: Some(20),
            completed: true,
        });
    }
    run.runtime_snapshots = vec![runtime_snapshot(Some(0), Some(0), Some(16)); 40];

    let mut unrelated = run.clone();
    for stage in &mut unrelated.stages {
        stage.relations = StageRelations::default();
    }
    let independent = analyze_run(&unrelated, AnalyzeOptions::default()).unwrap();
    let report = analyze_run(&run, AnalyzeOptions::default()).unwrap();
    let group = report.related_groups.first().expect("one related group");
    assert_eq!(
        group.representative,
        DiagnosisKind::DownstreamStageDominance
    );
    assert_eq!(group.members.len(), 2);

    let suspects = std::iter::once(&report.primary_suspect)
        .chain(&report.secondary_suspects)
        .collect::<Vec<_>>();
    assert_eq!(
        suspects
            .iter()
            .map(|suspect| &suspect.kind)
            .collect::<Vec<_>>(),
        vec![
            &DiagnosisKind::ApplicationQueuePressure,
            &DiagnosisKind::DownstreamStageDominance,
        ],
        "the related non-representative is absent while the unrelated family remains"
    );
    let options = AnalyzeOptions::default();
    assert!(suspects.iter().all(|suspect| {
        suspect.score >= options.confidence.ambiguity_min_score
            && suspect.score.abs_diff(suspects[0].score) <= options.confidence.ambiguity_score_gap
    }));
    assert!(suspects
        .iter()
        .all(|suspect| suspect.score >= options.confidence.medium_score_threshold));

    let ambiguity_note = "Top suspects are close in score; confidence is capped by ambiguity.";
    let ambiguity_members = suspects
        .iter()
        .filter(|suspect| {
            suspect
                .confidence_notes
                .iter()
                .any(|note| note == ambiguity_note)
        })
        .count();
    assert_eq!(ambiguity_members, 2);
    assert!(suspects
        .iter()
        .all(|suspect| suspect.confidence == Confidence::Medium));
    let has_warning = report
        .warnings
        .iter()
        .any(|warning| warning.contains("ranking as ambiguous"));
    assert_eq!(has_warning, ambiguity_members >= 2);

    let score = |report: &Report, kind: DiagnosisKind| {
        std::iter::once(&report.primary_suspect)
            .chain(&report.secondary_suspects)
            .find(|suspect| suspect.kind == kind)
            .map(|suspect| suspect.score)
            .expect("family remains independently visible in the ungrouped control")
    };
    for kind in [
        DiagnosisKind::ApplicationQueuePressure,
        DiagnosisKind::BlockingPoolPressure,
        DiagnosisKind::DownstreamStageDominance,
    ] {
        let expected = score(&independent, kind.clone());
        if kind == DiagnosisKind::BlockingPoolPressure {
            let blocking = group
                .members
                .iter()
                .find(|member| member.diagnosis == kind)
                .expect("blocking member remains structured evidence");
            assert_eq!(blocking.relevant_support, 40);
        } else {
            assert_eq!(score(&report, kind), expected);
        }
    }
}

// TT-TEST: support
#[test]
fn unknown_relation_round_trips_but_is_analyzer_inert() {
    let mut run = test_run();
    run.requests = (0..40).map(|i| sample_request(i + 1)).collect();
    run.stages = run
        .requests
        .iter()
        .map(|request| StageEvent {
            request_id: request.request_id.clone(),
            stage: "neutral".into(),
            relations: serde_json::from_value(serde_json::json!(["future_relation"]))
                .expect("unknown relation remains supported wire data"),
            started_at_unix_ms: 1,
            started_at_run_us: None,
            finished_at_unix_ms: 2,
            finished_at_run_us: None,
            latency_us: 900,
            success: true,
            completed: true,
        })
        .collect();
    run.runtime_snapshots = vec![runtime_snapshot(Some(1), Some(1), Some(16)); 40];
    let encoded = serde_json::to_value(&run).unwrap();
    assert!(encoded["stages"][0]["relations"]
        .as_array()
        .unwrap()
        .iter()
        .any(|value| value == "future_relation"));
    let report = analyze_run(&run, AnalyzeOptions::default()).unwrap();
    assert!(report.related_groups.is_empty());
}

// TT-TEST: support
#[test]
fn one_tagged_event_does_not_promote_same_named_untagged_evidence() {
    let mut run = test_run();
    run.requests = (0..40).map(|i| sample_request(i + 1)).collect();
    run.stages = run
        .requests
        .iter()
        .enumerate()
        .map(|(index, request)| StageEvent {
            request_id: request.request_id.clone(),
            stage: "same_name".into(),
            relations: if index == 0 {
                StageRelations::from_relation(StageRelation::BlockingPool)
            } else {
                StageRelations::default()
            },
            started_at_unix_ms: 1,
            started_at_run_us: None,
            finished_at_unix_ms: 2,
            finished_at_run_us: None,
            latency_us: 900,
            success: true,
            completed: true,
        })
        .collect();
    run.runtime_snapshots = vec![runtime_snapshot(Some(1), Some(1), Some(16)); 40];
    let report = analyze_run(&run, AnalyzeOptions::default()).unwrap();
    assert!(report.related_groups.is_empty());
    assert!(std::iter::once(&report.primary_suspect)
        .chain(&report.secondary_suspects)
        .any(|suspect| suspect.kind == DiagnosisKind::DownstreamStageDominance));
}

// TT-TEST: support
#[test]
fn truncation_warnings_remain_additive() {
    let mut run = test_run();
    run.truncation.dropped_requests = 1;
    run.truncation.dropped_stages = 1;
    run.truncation.dropped_runtime_snapshots = 1;
    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
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

// TT-TEST: support
#[test]
fn evidence_quality_weak_for_low_requests() {
    let report = analyze_run(&test_run(), AnalyzeOptions::default())
        .expect("analyzer options should be valid");
    assert_eq!(report.evidence_quality.quality, EvidenceQualityLevel::Weak);
    assert_eq!(
        report.evidence_quality.requests,
        SignalCoverageStatus::Partial
    );
}

// TT-TEST: support
#[test]
fn evidence_quality_requests_missing_when_zero_requests_even_if_dropped() {
    let mut run = test_run();
    run.requests.clear();
    run.truncation.dropped_requests = 3;
    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    assert_eq!(
        report.evidence_quality.requests,
        SignalCoverageStatus::Missing
    );
}

// TT-TEST: A07 primary
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
            completed: true,
        })
        .collect();
    run.runtime_snapshots = vec![runtime_snapshot(Some(1), None, Some(1)); 10];
    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    assert_eq!(
        report.evidence_quality.runtime_snapshots,
        SignalCoverageStatus::Partial
    );
    assert_eq!(
        report.evidence_quality.quality,
        EvidenceQualityLevel::Partial
    );
}

// TT-TEST: support
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
            completed: true,
        })
        .collect();
    run.stages = run
        .requests
        .iter()
        .map(|r| StageEvent {
            request_id: r.request_id.clone(),
            stage: "db".into(),
            relations: tailtriage_core::StageRelations::default(),
            started_at_unix_ms: 1,
            started_at_run_us: None,
            finished_at_unix_ms: 2,
            finished_at_run_us: None,
            latency_us: 400,
            success: true,
            completed: true,
        })
        .collect();
    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    assert_eq!(
        report.evidence_quality.quality,
        EvidenceQualityLevel::Strong
    );
}

// TT-TEST: A07 primary
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
            completed: true,
        })
        .collect();
    run.stages = run
        .requests
        .iter()
        .map(|r| StageEvent {
            request_id: r.request_id.clone(),
            stage: "db".into(),
            relations: tailtriage_core::StageRelations::default(),
            started_at_unix_ms: 1,
            started_at_run_us: None,
            finished_at_unix_ms: 2,
            finished_at_run_us: None,
            latency_us: 400,
            success: true,
            completed: true,
        })
        .collect();
    run.truncation.dropped_queues = 2;

    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    assert_eq!(
        report.evidence_quality.queues,
        SignalCoverageStatus::Truncated
    );
    assert_ne!(
        report.evidence_quality.quality,
        EvidenceQualityLevel::Strong
    );
}

// TT-TEST: support
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
            completed: true,
        })
        .collect();
    run.stages = run
        .requests
        .iter()
        .map(|r| StageEvent {
            request_id: r.request_id.clone(),
            stage: "db".into(),
            relations: tailtriage_core::StageRelations::default(),
            started_at_unix_ms: 1,
            started_at_run_us: None,
            finished_at_unix_ms: 2,
            finished_at_run_us: None,
            latency_us: 800,
            success: true,
            completed: true,
        })
        .collect();
    run.truncation.dropped_requests = 1;
    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    let mut scores = vec![report.primary_suspect.score];
    scores.extend(report.secondary_suspects.iter().map(|s| s.score));
    assert!(scores.windows(2).all(|w| w[0] >= w[1]));
}

// TT-TEST: A04 primary
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
            completed: true,
        })
        .collect();
    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    assert_eq!(report.primary_suspect.confidence, Confidence::Medium);
    assert!(report
        .primary_suspect
        .confidence_notes
        .iter()
        .any(|n| n == "Low completed-request count caps confidence."));
}

// TT-TEST: support
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
            completed: true,
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
    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    assert_eq!(
        report.primary_suspect.kind,
        DiagnosisKind::ApplicationQueuePressure
    );
    assert_eq!(report.primary_suspect.confidence, Confidence::High);
    assert!(report.primary_suspect.confidence_notes.is_empty());
}

// TT-TEST: support
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
            completed: true,
        })
        .collect();
    run.truncation.dropped_queues = 1;
    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
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

// TT-TEST: support
#[test]
fn missing_queue_instrumentation_uses_missing_queue_note() {
    let mut run = test_run();
    run.requests = vec![sample_request(1)];
    run.queues.clear();
    let eq = evidence::evidence_quality(&run, &AnalyzeOptions::default());
    let mut suspects = vec![Suspect::new(
        DiagnosisKind::ApplicationQueuePressure,
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

// TT-TEST: support
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
            relations: tailtriage_core::StageRelations::default(),
            started_at_unix_ms: 1,
            started_at_run_us: None,
            finished_at_unix_ms: 10,
            finished_at_run_us: None,
            latency_us: 4_800,
            success: true,
            completed: true,
        })
        .collect();
    run.truncation.dropped_stages = 1;
    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
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

// TT-TEST: support
#[test]
fn missing_stage_instrumentation_uses_missing_stage_note() {
    let mut run = test_run();
    run.requests = vec![sample_request(1)];
    run.stages.clear();
    let eq = evidence::evidence_quality(&run, &AnalyzeOptions::default());
    let mut suspects = vec![Suspect::new(
        DiagnosisKind::DownstreamStageDominance,
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

// TT-TEST: A04 primary
#[test]
fn runtime_partial_fields_cap_executor_or_blocking_confidence() {
    let mut run = test_run();
    run.requests = vec![sample_request(1)];
    run.runtime_snapshots = (0..10)
        .map(|i| RuntimeSnapshot {
            at_unix_ms: i,
            at_run_us: None,
            alive_tasks: Some(1),
            worker_count: None,
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

// TT-TEST: support
#[test]
fn missing_runtime_snapshots_use_missing_runtime_note() {
    let mut run = test_run();
    run.requests = vec![sample_request(1)];
    run.runtime_snapshots.clear();
    let eq = evidence::evidence_quality(&run, &AnalyzeOptions::default());
    let mut suspects = vec![Suspect::new(
        DiagnosisKind::ExecutorPressure,
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

// TT-TEST: A04 primary
#[test]
fn ambiguity_cap_adds_note_to_close_top_suspects() {
    let mut suspects = vec![
        Suspect::new(DiagnosisKind::ApplicationQueuePressure, 100, vec![], vec![]),
        Suspect::new(DiagnosisKind::DownstreamStageDominance, 97, vec![], vec![]),
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

// TT-TEST: A06 primary
#[test]
fn ambiguity_capping_preserves_order_and_scores() {
    let mut suspects = vec![
        Suspect::new(DiagnosisKind::ApplicationQueuePressure, 100, vec![], vec![]),
        Suspect::new(DiagnosisKind::DownstreamStageDominance, 100, vec![], vec![]),
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
    assert_eq!(suspects[0].kind, DiagnosisKind::ApplicationQueuePressure);
    assert_eq!(suspects[1].kind, DiagnosisKind::DownstreamStageDominance);
    assert!(suspects[0]
        .confidence_notes
        .iter()
        .any(|n| n == "Top suspects are close in score; confidence is capped by ambiguity."));
    assert!(suspects[1]
        .confidence_notes
        .iter()
        .any(|n| n == "Top suspects are close in score; confidence is capped by ambiguity."));
}

// TT-TEST: support
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
            completed: true,
        })
        .collect();
    let mut suspects = vec![
        Suspect::new(DiagnosisKind::ApplicationQueuePressure, 100, vec![], vec![]),
        Suspect::new(DiagnosisKind::DownstreamStageDominance, 10, vec![], vec![]),
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

// TT-TEST: A08 primary
#[test]
fn route_breakdowns_empty_for_single_route() {
    let report = analyze_run(&test_run(), AnalyzeOptions::default())
        .expect("analyzer options should be valid");
    assert!(report.route_breakdowns.is_empty());
    assert!(report
        .warnings
        .iter()
        .all(|warning| warning != ROUTE_DIVERGENCE_WARNING));
}

// TT-TEST: support
#[test]
fn single_route_executor_signals_do_not_emit_route_breakdowns_or_divergence_warning() {
    let mut run = test_run();
    run.runtime_snapshots = vec![runtime_snapshot(Some(150), Some(120), Some(2))];
    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    assert!(report.route_breakdowns.is_empty());
    assert!(report
        .warnings
        .iter()
        .all(|warning| warning != ROUTE_DIVERGENCE_WARNING));
}

// TT-TEST: A08 primary
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
            completed: true,
        });
    }
    for req_id in ["req-5", "req-6", "req-7"] {
        run.stages.push(StageEvent {
            request_id: req_id.to_owned(),
            stage: "db".into(),
            relations: tailtriage_core::StageRelations::default(),
            started_at_unix_ms: 1,
            started_at_run_us: None,
            finished_at_unix_ms: 2,
            finished_at_run_us: None,
            latency_us: 1_900,
            success: true,
            completed: true,
        });
    }
    run.runtime_snapshots = vec![runtime_snapshot(Some(200), Some(140), Some(180))];
    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    assert_eq!(report.route_breakdowns.len(), 2);
    assert_eq!(report.route_breakdowns[0].route, "/a");
    assert_eq!(report.route_breakdowns[1].route, "/b");
    assert_eq!(
        report.route_breakdowns[0].primary_suspect.kind,
        DiagnosisKind::ApplicationQueuePressure
    );
    assert_eq!(
        report.route_breakdowns[1].primary_suspect.kind,
        DiagnosisKind::DownstreamStageDominance
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
        .all(|s| s.kind != DiagnosisKind::ExecutorPressure
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

// TT-TEST: support
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
            completed: true,
        });
    }
    for req_id in ["req-5", "req-6", "req-7"] {
        run.stages.push(StageEvent {
            request_id: req_id.to_owned(),
            stage: "db".into(),
            relations: tailtriage_core::StageRelations::default(),
            started_at_unix_ms: 1,
            started_at_run_us: None,
            finished_at_unix_ms: 2,
            finished_at_run_us: None,
            latency_us: 1_900,
            success: true,
            completed: true,
        });
    }

    let default_report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    assert_eq!(default_report.route_breakdowns.len(), 2);
    assert!(default_report
        .warnings
        .iter()
        .any(|warning| warning == ROUTE_DIVERGENCE_WARNING));

    let mut options = AnalyzeOptions::default();
    options.route.emit_on_divergent_suspects = false;
    let toggled_report = analyze_run(&run, options).expect("analyzer options should be valid");
    assert_eq!(toggled_report.route_breakdowns.len(), 2);
    assert!(toggled_report
        .warnings
        .iter()
        .all(|warning| warning != ROUTE_DIVERGENCE_WARNING));
}

// TT-TEST: support
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
            completed: true,
        });
    }
    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    assert!(report.route_breakdowns.is_empty());
    assert!(report
        .warnings
        .iter()
        .all(|warning| warning != ROUTE_DIVERGENCE_WARNING));
}

// TT-TEST: A08 primary
#[test]
fn route_breakdowns_do_not_change_global_primary_suspect() {
    let mut run = test_run();
    run.runtime_snapshots = vec![runtime_snapshot(Some(300), Some(250), Some(200))];
    let global = analyze_run_internal(
        &run,
        crate::scoring::classify_worker_evidence(&run),
        &AnalyzeOptions::default(),
    );
    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    assert_eq!(report.primary_suspect, global.primary_suspect);
    assert_eq!(report.secondary_suspects, global.secondary_suspects);
    assert_eq!(report.related_groups, global.related_groups);
}

// TT-TEST: A09 primary
#[test]
fn temporal_segments_present_and_empty_below_threshold() {
    let report = analyze_run(&test_run(), AnalyzeOptions::default())
        .expect("analyzer options should be valid");
    let value = serde_json::to_value(&report).expect("serialize");
    assert!(value.get("temporal_segments").is_some());
    assert!(report.temporal_segments.is_empty());
}

// TT-TEST: support
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
            completed: true,
        });
    }
    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    assert_eq!(report.temporal_segments.len(), 2);
    let early = report
        .temporal_segments
        .iter()
        .find(|s| s.name == "early")
        .expect("early temporal segment should be emitted");
    assert_eq!(early.finished_at_unix_ms, Some(1000));
}

// TT-TEST: support
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
            completed: true,
        });
    }
    for id in 1..=10 {
        run.stages.push(StageEvent {
            request_id: format!("req-{id}"),
            stage: "db".into(),
            relations: tailtriage_core::StageRelations::default(),
            started_at_unix_ms: 100,
            started_at_run_us: None,
            finished_at_unix_ms: 101,
            finished_at_run_us: None,
            latency_us: 5_000,
            success: true,
            completed: true,
        });
    }

    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");

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
        DiagnosisKind::ApplicationQueuePressure
    );
    assert_eq!(
        late.primary_suspect.kind,
        DiagnosisKind::DownstreamStageDominance
    );
}

// TT-TEST: A09 primary
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
            worker_count: None,
            blocking_queue_depth: Some(0),
            remote_schedule_count: None,
        },
        RuntimeSnapshot {
            at_unix_ms: 1,
            at_run_us: Some(11_200),
            global_queue_depth: Some(1),
            local_queue_depth: Some(1),
            alive_tasks: Some(100),
            worker_count: None,
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

    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");

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

// TT-TEST: support
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
            worker_count: None,
            blocking_queue_depth: Some(0),
            remote_schedule_count: None,
        },
        RuntimeSnapshot {
            at_unix_ms: 15,
            at_run_us: None,
            global_queue_depth: Some(1),
            local_queue_depth: Some(1),
            alive_tasks: Some(100),
            worker_count: None,
            blocking_queue_depth: Some(0),
            remote_schedule_count: None,
        },
        RuntimeSnapshot {
            at_unix_ms: 5,
            at_run_us: Some(150_000),
            global_queue_depth: Some(1),
            local_queue_depth: Some(1),
            alive_tasks: Some(100),
            worker_count: None,
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

    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");

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

// TT-TEST: support
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
            worker_count: None,
            blocking_queue_depth: Some(0),
            remote_schedule_count: None,
        },
        RuntimeSnapshot {
            at_unix_ms: 12,
            at_run_us: None,
            global_queue_depth: Some(1),
            local_queue_depth: Some(1),
            alive_tasks: Some(100),
            worker_count: None,
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

    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");

    assert_eq!(report.request_count, 20);
    assert_eq!(report.temporal_segments.len(), 2);
    for segment in &report.temporal_segments {
        assert!(segment.warnings.iter().any(|warning| warning
            == "Temporal segment used wall-clock timestamp fallback; attribution is approximate for artifacts without complete run-relative timing."));
    }
}

// TT-TEST: support
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

    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");

    assert_eq!(report.temporal_segments.len(), 2);
    for segment in &report.temporal_segments {
        assert!(!segment.warnings.iter().any(|warning| warning
            == "Temporal segment used wall-clock timestamp fallback; attribution is approximate for artifacts without complete run-relative timing."));
    }
}

// TT-TEST: support
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

    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");

    assert_eq!(report.temporal_segments.len(), 2);
    assert_eq!(report.temporal_segments[0].name, "early");
    assert_eq!(report.temporal_segments[1].name, "late");
    assert_eq!(report.temporal_segments[0].p95_latency_us, Some(1_000));
    assert_eq!(report.temporal_segments[1].p95_latency_us, Some(6_000));
}

// TT-TEST: support
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

    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");

    assert_eq!(report.temporal_segments.len(), 2);
    assert_eq!(report.temporal_segments[0].name, "early");
    assert_eq!(report.temporal_segments[1].name, "late");
    assert_eq!(report.temporal_segments[0].p95_latency_us, Some(1_000));
    assert_eq!(report.temporal_segments[1].p95_latency_us, Some(6_000));
}

// TT-TEST: support
#[test]
fn temporal_segments_not_emitted_when_no_meaningful_difference() {
    let mut run = test_run();
    run.requests = (0..20).map(|i| sample_request(i + 1)).collect();
    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    assert!(report.temporal_segments.is_empty());
    assert!(!report
        .warnings
        .iter()
        .any(|w| w == TEMPORAL_SUSPECT_SHIFT_WARNING));
}

// TT-TEST: support
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
            completed: true,
        });
    }
    for i in 11..=20 {
        run.stages.push(StageEvent {
            request_id: format!("req-{i}"),
            stage: "db".into(),
            relations: tailtriage_core::StageRelations::default(),
            started_at_unix_ms: i,
            started_at_run_us: None,
            finished_at_unix_ms: i + 1,
            finished_at_run_us: None,
            latency_us: 5_000,
            success: true,
            completed: true,
        });
    }
    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
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

// TT-TEST: support
#[test]
fn temporal_p95_shift_emits_segments_and_ignores_missing_or_zero_lower_p95() {
    let mut run = test_run();
    run.requests = (0..20).map(|i| sample_request(i + 1)).collect();
    for i in 10usize..20 {
        if let Some(req) = run.requests.get_mut(i) {
            req.latency_us = 5_000;
        }
    }
    let shifted =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
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

// TT-TEST: A09 primary
#[test]
fn temporal_segments_do_not_change_global_candidate_semantics() {
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
            completed: true,
        });
    }
    let global = analyze_run_internal(
        &run,
        crate::scoring::classify_worker_evidence(&run),
        &AnalyzeOptions::default(),
    );
    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    assert_eq!(report.primary_suspect, global.primary_suspect);
    assert_eq!(report.secondary_suspects, global.secondary_suspects);
    assert_eq!(report.related_groups, global.related_groups);
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
            completed: true,
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
            relations: tailtriage_core::StageRelations::default(),
            started_at_unix_ms: i_u64,
            started_at_run_us: Some(i_u64 * 10_000),
            finished_at_unix_ms: i_u64 + 1,
            finished_at_run_us: Some(i_u64 * 10_000 + 7_000),
            latency_us: 7_000,
            success: true,
            completed: true,
        });
    }
    run.runtime_snapshots = vec![
        RuntimeSnapshot {
            at_unix_ms: 5,
            at_run_us: Some(50_000),
            global_queue_depth: Some(1),
            local_queue_depth: Some(1),
            alive_tasks: Some(20),
            worker_count: None,
            blocking_queue_depth: Some(0),
            remote_schedule_count: None,
        },
        RuntimeSnapshot {
            at_unix_ms: 15,
            at_run_us: Some(150_000),
            global_queue_depth: Some(1),
            local_queue_depth: Some(1),
            alive_tasks: Some(20),
            worker_count: None,
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

// TT-TEST: support
#[test]
fn temporal_segments_warn_only_when_run_relative_timing_is_incomplete() {
    let complete_report = analyze_run(
        &run_with_temporal_shift_and_run_relative_offsets(),
        AnalyzeOptions::default(),
    )
    .expect("analyzer options should be valid");
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
    let incomplete_report = analyze_run(&incomplete_run, AnalyzeOptions::default())
        .expect("analyzer options should be valid");
    assert_eq!(incomplete_report.temporal_segments.len(), 2);
    for segment in &incomplete_report.temporal_segments {
        assert!(segment
            .warnings
            .iter()
            .any(|w| w == TEMPORAL_WALL_CLOCK_FALLBACK_WARNING));
    }
}

// TT-TEST: support
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
        worker_count: None,
        blocking_queue_depth: Some(0),
        remote_schedule_count: None,
    }];
    run.inflight = vec![tailtriage_core::InFlightSnapshot {
        at_unix_ms: 1,
        at_run_us: None,
        gauge: "http.server.requests".into(),
        count: 1,
    }];
    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    assert!(report.temporal_segments.is_empty());
}

// TT-TEST: support
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
            completed: true,
        });
    }
    for i in 11..=20 {
        run.stages.push(StageEvent {
            request_id: format!("req-{i}"),
            stage: "db".into(),
            relations: tailtriage_core::StageRelations::default(),
            started_at_unix_ms: i,
            started_at_run_us: None,
            finished_at_unix_ms: i + 1,
            finished_at_run_us: None,
            latency_us: 9_000,
            success: true,
            completed: true,
        });
    }
    run.runtime_snapshots = vec![runtime_snapshot(Some(1), Some(1), Some(1))];
    run.inflight = vec![tailtriage_core::InFlightSnapshot {
        at_unix_ms: 1,
        at_run_us: None,
        gauge: "http.server.requests".into(),
        count: 1,
    }];

    let global = analyze_run_internal(
        &run,
        crate::scoring::classify_worker_evidence(&run),
        &AnalyzeOptions::default(),
    );
    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");

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

// TT-TEST: support
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
            completed: true,
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
            relations: tailtriage_core::StageRelations::default(),
            started_at_unix_ms: i_u64,
            started_at_run_us: None,
            finished_at_unix_ms: i_u64 + 1,
            finished_at_run_us: None,
            latency_us: 9_000,
            success: true,
            completed: true,
        });
    }
    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    assert!(report
        .warnings
        .iter()
        .any(|w| w == TEMPORAL_SUSPECT_SHIFT_WARNING));
    assert!(report
        .warnings
        .iter()
        .any(|w| w == TEMPORAL_P95_SHIFT_WARNING));
}

// TT-TEST: support
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
    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    assert_eq!(report.temporal_segments.len(), 2);
    for segment in &report.temporal_segments {
        assert!(segment
            .warnings
            .iter()
            .any(|w| w == TEMPORAL_OVERLAP_ATTRIBUTION_WARNING));
    }
}

// TT-TEST: support
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
    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    assert_eq!(report.temporal_segments.len(), 2);
    for segment in &report.temporal_segments {
        assert!(!segment
            .warnings
            .iter()
            .any(|w| w == TEMPORAL_OVERLAP_ATTRIBUTION_WARNING));
    }
}

// TT-TEST: support
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
    let mut report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
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

// TT-TEST: A12 secondary
#[test]
fn public_api_supports_report_text_and_json_contract_fields() {
    let run = test_run();
    let report: Report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    let text = render_text(&report);
    assert!(!text.is_empty(), "rendered text should not be empty");

    let report_json =
        serde_json::to_string_pretty(&report).expect("report should serialize to json");
    assert!(report_json.contains("\"evidence_quality\""));
    assert!(report_json.contains("\"confidence_notes\""));
    assert!(report_json.contains("\"route_breakdowns\""));
    assert!(report_json.contains("\"temporal_segments\""));
}

// TT-TEST: A12 secondary
#[test]
fn diagnosis_kind_machine_labels_match_serde_for_all_variants() {
    let cases = [
        (
            DiagnosisKind::ApplicationQueuePressure,
            "application_queue_pressure",
        ),
        (
            DiagnosisKind::BlockingPoolPressure,
            "blocking_pool_pressure",
        ),
        (DiagnosisKind::ExecutorPressure, "executor_pressure"),
        (
            DiagnosisKind::DownstreamStageDominance,
            "downstream_stage_dominance",
        ),
        (DiagnosisKind::InsufficientEvidence, "insufficient_evidence"),
    ];

    for (kind, expected) in cases {
        assert_eq!(kind.as_str(), expected);
        assert_eq!(
            serde_json::to_string(&kind).unwrap(),
            format!("\"{expected}\"")
        );
    }
}

// TT-TEST: A12 primary
#[test]
fn render_json_pretty_matches_serde_json_pretty() {
    let report = analyze_run(&test_run(), AnalyzeOptions::default())
        .expect("analyzer options should be valid");
    let actual = render_json_pretty(&report).expect("report json should render");
    let expected = serde_json::to_string_pretty(&report).expect("report json should render");
    assert_eq!(actual, expected);
}

// TT-TEST: A12 primary
#[test]
fn render_json_matches_serde_json_compact() {
    let report = analyze_run(&test_run(), AnalyzeOptions::default())
        .expect("analyzer options should be valid");
    let actual = render_json(&report).expect("report json should render");
    let expected = serde_json::to_string(&report).expect("report json should render");
    assert_eq!(actual, expected);
}

// TT-TEST: A12 primary
#[test]
fn compact_and_pretty_report_json_are_value_equivalent() {
    let report = analyze_run(&test_run(), AnalyzeOptions::default())
        .expect("analyzer options should be valid");
    let compact = render_json(&report).expect("compact report json should render");
    let pretty = render_json_pretty(&report).expect("pretty report json should render");
    let compact_value: serde_json::Value =
        serde_json::from_str(&compact).expect("compact report json should parse");
    let pretty_value: serde_json::Value =
        serde_json::from_str(&pretty).expect("pretty report json should parse");
    assert_eq!(compact_value, pretty_value);
}

// TT-TEST: A13 primary
#[test]
fn analyze_options_defaults_match_v1_surface() {
    let options = AnalyzeOptions::default();
    assert_eq!(options.queueing.trigger_permille, 300);
    assert_eq!(options.blocking.min_nonzero_samples_for_signal, 2);
    assert_eq!(options.executor.min_global_queue_p95_for_signal, 1);
    assert_eq!(options.downstream.min_stage_samples, 3);
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

// TT-TEST: support
#[test]
fn analyze_options_default_validates() {
    assert!(AnalyzeOptions::default().validate().is_ok());
}

// TT-TEST: Q01 primary
#[test]
#[allow(clippy::too_many_lines)]
fn analyze_options_validate_rejects_invalid_classes() {
    assert!({
        let mut options = AnalyzeOptions::default();
        {
            let o = &mut options.queueing;
            o.trigger_permille = 1001;
        }
        options
    }
    .validate()
    .is_err());
    assert!({
        let mut options = AnalyzeOptions::default();
        {
            let o = &mut options.confidence;
            o.medium_score_threshold = 90;
            o.high_score_threshold = 80;
        }
        options
    }
    .validate()
    .is_err());
    assert!({
        let mut options = AnalyzeOptions::default();
        {
            let o = &mut options.confidence;
            o.high_score_threshold = 101;
        }
        options
    }
    .validate()
    .is_err());
    assert!({
        let mut options = AnalyzeOptions::default();
        {
            let o = &mut options.confidence;
            o.ambiguity_min_score = 101;
        }
        options
    }
    .validate()
    .is_err());
    assert!({
        let mut options = AnalyzeOptions::default();
        {
            let o = &mut options.confidence;
            o.ambiguity_score_gap = 101;
        }
        options
    }
    .validate()
    .is_err());
    assert!({
        let mut options = AnalyzeOptions::default();
        {
            let o = &mut options.route;
            o.breakdown_limit = 0;
        }
        options
    }
    .validate()
    .is_err());
    assert!({
        let mut options = AnalyzeOptions::default();
        {
            let o = &mut options.route;
            o.slowest_to_fastest_p95_ratio_numerator = 0;
        }
        options
    }
    .validate()
    .is_err());
    assert!({
        let mut options = AnalyzeOptions::default();
        {
            let o = &mut options.route;
            o.slowest_to_fastest_p95_ratio_numerator = 1;
        }
        {
            let o = &mut options.route;
            o.slowest_to_fastest_p95_ratio_denominator = 2;
        }
        options
    }
    .validate()
    .is_err());
    assert!({
        let mut options = AnalyzeOptions::default();
        {
            let o = &mut options.temporal;
            o.min_segment_request_count = 0;
        }
        options
    }
    .validate()
    .is_err());
    assert!({
        let mut options = AnalyzeOptions::default();
        {
            let o = &mut options.temporal;
            o.min_segment_request_count = 11;
        }
        options
    }
    .validate()
    .is_err());
    assert!({
        let mut options = AnalyzeOptions::default();
        {
            let o = &mut options.temporal;
            o.share_shift_permille = 1001;
        }
        options
    }
    .validate()
    .is_err());
    assert!({
        let mut options = AnalyzeOptions::default();
        {
            let o = &mut options.temporal;
            o.p95_shift_ratio_numerator = 0;
        }
        options
    }
    .validate()
    .is_err());
    assert!({
        let mut options = AnalyzeOptions::default();
        {
            let o = &mut options.temporal;
            o.p95_shift_ratio_numerator = 1;
            o.p95_shift_ratio_denominator = 2;
        }
        options
    }
    .validate()
    .is_err());
}

// TT-TEST: Q01 primary
#[test]
fn validate_ratio_zero_denominators_report_exact_paths() {
    let err = {
        let mut options = AnalyzeOptions::default();
        {
            let o = &mut options.route;
            o.slowest_to_fastest_p95_ratio_denominator = 0;
        }
        options
    }
    .validate()
    .expect_err("fastest ratio denominator zero should fail");
    assert!(matches!(
        err,
        AnalyzeConfigError::InvalidConfigValue {
            path: "route.slowest_to_fastest_p95_ratio_denominator",
            ..
        }
    ));

    let err = {
        let mut options = AnalyzeOptions::default();
        {
            let o = &mut options.route;
            o.slowest_to_global_p95_ratio_denominator = 0;
        }
        options
    }
    .validate()
    .expect_err("global ratio denominator zero should fail");
    assert!(matches!(
        err,
        AnalyzeConfigError::InvalidConfigValue {
            path: "route.slowest_to_global_p95_ratio_denominator",
            ..
        }
    ));

    let err = {
        let mut options = AnalyzeOptions::default();
        {
            let o = &mut options.temporal;
            o.p95_shift_ratio_denominator = 0;
        }
        options
    }
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

// TT-TEST: A11 primary
#[test]
fn analyze_run_rejects_invalid_options() {
    let run = test_run();
    let options = {
        let mut options = AnalyzeOptions::default();
        {
            let o = &mut options.queueing;
            o.trigger_permille = 1001;
        }
        options
    };
    assert!(matches!(
        analyze_run(&run, options),
        Err(AnalyzeConfigError::InvalidConfigValue {
            path: "queueing.trigger_permille",
            ..
        })
    ));
}

// TT-TEST: A11 primary
#[test]
fn analyze_run_still_works_with_default_options() {
    let run = test_run();
    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    assert_eq!(report.request_count, 3);
}

// TT-TEST: support
#[test]
fn queueing_trigger_descriptor_direction_text_is_correct() {
    let descriptor = crate::analyze_option_descriptors()
        .iter()
        .find(|d| d.path() == "queueing.trigger_permille")
        .expect("queueing.trigger_permille descriptor exists");
    assert!(descriptor
        .increasing()
        .expect("increasing text")
        .contains("harder"));
    assert!(descriptor
        .decreasing()
        .expect("decreasing text")
        .contains("easier"));
}

// TT-TEST: A13 primary
// TT-TEST: Q01 primary
#[test]
fn descriptors_have_unique_and_exact_v1_paths() {
    let descriptors = crate::analyze_option_descriptors();
    let paths = descriptors
        .iter()
        .map(crate::AnalyzeOptionDescriptor::path)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(paths.len(), descriptors.len());
    let expected = [
        "queueing.trigger_permille",
        "blocking.min_nonzero_samples_for_signal",
        "executor.min_global_queue_p95_for_signal",
        "executor.min_runnable_queue_per_worker_p95_milli_for_signal",
        "downstream.min_stage_samples",
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

// TT-TEST: A13 secondary
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
            "executor.min_global_queue_p95_for_signal",
            opts.executor.min_global_queue_p95_for_signal.to_string(),
        ),
        (
            "executor.min_runnable_queue_per_worker_p95_milli_for_signal",
            opts.executor
                .min_runnable_queue_per_worker_p95_milli_for_signal
                .to_string(),
        ),
        (
            "downstream.min_stage_samples",
            opts.downstream.min_stage_samples.to_string(),
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
            Some(&descriptor.default_value().to_string()),
            expected.get(descriptor.path())
        );
    }
}

fn assert_default_report_has_no_analyzer_config(report: &Report) {
    assert!(report.analyzer_config.is_none());
}

// TT-TEST: support
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
            completed: true,
        })
        .collect();
    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    assert_eq!(
        report.primary_suspect.kind,
        DiagnosisKind::ApplicationQueuePressure
    );
    assert_default_report_has_no_analyzer_config(&report);
}

// TT-TEST: support
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
            relations: tailtriage_core::StageRelations::default(),
            started_at_unix_ms: 1,
            started_at_run_us: None,
            finished_at_unix_ms: 2,
            finished_at_run_us: None,
            latency_us: 3_900_000,
            success: true,
            completed: true,
        })
        .collect();
    run.runtime_snapshots = vec![runtime_snapshot(Some(1), Some(1), Some(240)); 80];
    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    assert_eq!(
        report.primary_suspect.kind,
        DiagnosisKind::DownstreamStageDominance
    );
    assert_default_report_has_no_analyzer_config(&report);
}

// TT-TEST: support
#[test]
fn default_options_compat_insufficient_and_weak_evidence_case() {
    let report = analyze_run(&test_run(), AnalyzeOptions::default())
        .expect("analyzer options should be valid");
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

// TT-TEST: support
#[test]
fn default_options_compat_downstream_stage_dominance_case() {
    let mut run = test_run();
    run.stages = run
        .requests
        .iter()
        .map(|r| StageEvent {
            request_id: r.request_id.clone(),
            stage: "db".into(),
            relations: tailtriage_core::StageRelations::default(),
            started_at_unix_ms: 1,
            started_at_run_us: None,
            finished_at_unix_ms: 2,
            finished_at_run_us: None,
            latency_us: 900,
            success: true,
            completed: true,
        })
        .collect();
    run.runtime_snapshots = vec![runtime_snapshot(Some(2), Some(1), Some(1)); 5];
    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    assert_eq!(
        report.primary_suspect.kind,
        DiagnosisKind::DownstreamStageDominance
    );
    assert_default_report_has_no_analyzer_config(&report);
}

// TT-TEST: support
#[test]
fn default_options_compat_truncated_evidence_case() {
    let mut run = test_run();
    run.truncation.dropped_requests = 2;
    run.truncation.limits_hit = true;
    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    assert!(report
        .warnings
        .iter()
        .any(|w| w.contains("dropped evidence can reduce diagnosis completeness and confidence")));
    assert!(report.evidence_quality.truncated);
    assert_default_report_has_no_analyzer_config(&report);
}

// TT-TEST: support
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
            completed: true,
        });
    }
    for req_id in ["req-5", "req-6", "req-7"] {
        run.stages.push(StageEvent {
            request_id: req_id.to_owned(),
            stage: "db".into(),
            relations: tailtriage_core::StageRelations::default(),
            started_at_unix_ms: 1,
            started_at_run_us: None,
            finished_at_unix_ms: 2,
            finished_at_run_us: None,
            latency_us: 1_900,
            success: true,
            completed: true,
        });
    }
    run.runtime_snapshots = vec![runtime_snapshot(Some(200), Some(140), Some(180))];
    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    assert!(!report.route_breakdowns.is_empty());
    assert!(report
        .warnings
        .iter()
        .any(|w| w == ROUTE_DIVERGENCE_WARNING));
    assert_default_report_has_no_analyzer_config(&report);
}

// TT-TEST: support
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
            completed: true,
        })
        .collect();
    run.stages = run
        .requests
        .iter()
        .enumerate()
        .map(|(i, r)| StageEvent {
            request_id: r.request_id.clone(),
            stage: "db".into(),
            relations: tailtriage_core::StageRelations::default(),
            started_at_unix_ms: 1,
            started_at_run_us: None,
            finished_at_unix_ms: 2,
            finished_at_run_us: None,
            latency_us: if i < 20 { 200 } else { 4_400 },
            success: true,
            completed: true,
        })
        .collect();
    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    assert!(!report.temporal_segments.is_empty());
    assert_default_report_has_no_analyzer_config(&report);
}

// TT-TEST: A13 primary
#[test]
fn analyzer_config_transparency_default_report_omits_config() {
    let run = test_run();
    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");

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

// TT-TEST: A13 primary
#[test]
fn analyzer_config_transparency_non_default_report_includes_config() {
    let run = test_run();
    let mut options = AnalyzeOptions::default();
    options.queueing.trigger_permille = 400;
    options.temporal.min_request_count = 30;

    let report = analyze_run(&run, options).expect("analyzer options should be valid");
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

// TT-TEST: support
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
            completed: true,
        });
    }
    let default_report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    assert_eq!(
        default_report.primary_suspect.kind,
        DiagnosisKind::ApplicationQueuePressure
    );
    let strict = {
        let mut options = AnalyzeOptions::default();
        {
            let o = &mut options.queueing;
            o.trigger_permille = 600;
        }
        options
    };
    let strict_report = analyze_run(&run, strict).expect("analyzer options should be valid");
    assert_ne!(
        strict_report.primary_suspect.kind,
        DiagnosisKind::ApplicationQueuePressure
    );
}

// TT-TEST: support
#[test]
fn option_blocking_min_nonzero_samples_changes_signal_emission() {
    let mut run = option_run_twenty_requests();
    run.runtime_snapshots = vec![runtime_snapshot(Some(0), Some(0), Some(0)); 100];
    run.runtime_snapshots[0].blocking_queue_depth = Some(1);
    let default_report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    assert_ne!(
        default_report.primary_suspect.kind,
        DiagnosisKind::BlockingPoolPressure
    );
    let relaxed = {
        let mut options = AnalyzeOptions::default();
        {
            let o = &mut options.blocking;
            o.min_nonzero_samples_for_signal = 1;
        }
        options
    };
    let relaxed_report = analyze_run(&run, relaxed).expect("analyzer options should be valid");
    assert_eq!(
        relaxed_report.primary_suspect.kind,
        DiagnosisKind::BlockingPoolPressure
    );
}

// TT-TEST: support
#[test]
fn option_executor_min_global_queue_p95_changes_signal_emission() {
    let mut run = option_run_twenty_requests();
    run.runtime_snapshots = vec![runtime_snapshot(Some(1), Some(0), Some(0)); 20];
    let default_report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    assert_eq!(
        default_report.primary_suspect.kind,
        DiagnosisKind::ExecutorPressure
    );
    let strict = {
        let mut options = AnalyzeOptions::default();
        {
            let o = &mut options.executor;
            o.min_global_queue_p95_for_signal = 2;
        }
        options
    };
    let strict_report = analyze_run(&run, strict).expect("analyzer options should be valid");
    assert_ne!(
        strict_report.primary_suspect.kind,
        DiagnosisKind::ExecutorPressure
    );
}

// TT-TEST: support
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
            completed: true,
        });
    }

    let default_report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    assert_eq!(
        default_report.primary_suspect.kind,
        DiagnosisKind::ApplicationQueuePressure
    );
    assert_eq!(default_report.primary_suspect.score, 87);
    assert_eq!(default_report.primary_suspect.confidence, Confidence::High);

    let strict = {
        let mut options = AnalyzeOptions::default();
        {
            let o = &mut options.confidence;
            o.high_score_threshold = 91;
        }
        options
    };
    let strict_report = analyze_run(&run, strict).expect("analyzer options should be valid");
    assert_eq!(
        strict_report.primary_suspect.kind,
        DiagnosisKind::ApplicationQueuePressure
    );
    assert_eq!(strict_report.primary_suspect.score, 87);
    assert_eq!(strict_report.primary_suspect.confidence, Confidence::Medium);
}

// TT-TEST: A13 primary
#[test]
fn analyzer_toml_full_parses() {
    let input = include_str!("../../examples/analyzer-config.toml");
    let options = AnalyzeOptions::from_toml_str(input).expect("parse full analyzer toml");
    assert_eq!(options.queueing.trigger_permille, 400);
}

// TT-TEST: support
#[test]
fn analyzer_toml_sparse_preserves_defaults() {
    let input = "[analyzer]\nschema_version=1\n[analyzer.queueing]\ntrigger_permille=450\n";
    let options = AnalyzeOptions::from_toml_str(input).expect("parse sparse toml");
    assert_eq!(options.queueing.trigger_permille, 450);
    assert_eq!(options.blocking, AnalyzeOptions::default().blocking);
}

// TT-TEST: support
#[test]
fn analyzer_toml_merge_sparse_preserves_unrelated_non_default_base_values() {
    let base = {
        let mut options = AnalyzeOptions::default();
        {
            let o = &mut options.executor;
            o.min_global_queue_p95_for_signal = 99;
        }
        options
    };
    let merged = base
        .merge_toml_str("[analyzer]\nschema_version=1\n[analyzer.queueing]\ntrigger_permille=410\n")
        .expect("merge");
    assert_eq!(merged.queueing.trigger_permille, 410);
    assert_eq!(merged.executor.min_global_queue_p95_for_signal, 99);
}

// TT-TEST: support
#[test]
fn analyzer_toml_missing_analyzer_fails() {
    assert!(matches!(
        AnalyzeOptions::from_toml_str("[other]\na=1\n"),
        Err(AnalyzeConfigError::MissingAnalyzerTable)
    ));
}
// TT-TEST: support
#[test]
fn analyzer_toml_root_level_queueing_group_is_rejected() {
    assert!(matches!(
        AnalyzeOptions::from_toml_str("[queueing]\ntrigger_permille=400\n"),
        Err(AnalyzeConfigError::MissingAnalyzerTable)
    ));
}

// TT-TEST: support
#[test]
fn analyzer_toml_missing_schema_fails() {
    assert!(matches!(
        AnalyzeOptions::from_toml_str("[analyzer]\n"),
        Err(AnalyzeConfigError::MissingSchemaVersion)
    ));
}
// TT-TEST: support
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
// TT-TEST: support
#[test]
fn analyzer_toml_unknown_top_level_sibling_ignored() {
    let input = "[controller]\nmode='light'\n[analyzer]\nschema_version=1\n";
    assert!(AnalyzeOptions::from_toml_str(input).is_ok());
}
// TT-TEST: support
#[test]
fn analyzer_toml_unknown_field_under_analyzer_fails() {
    assert!(matches!(
        AnalyzeOptions::from_toml_str("[analyzer]\nschema_version=1\nfoo=1\n"),
        Err(AnalyzeConfigError::InvalidToml { .. })
    ));
}
// TT-TEST: support
#[test]
fn analyzer_toml_unknown_subgroup_fails() {
    assert!(matches!(
        AnalyzeOptions::from_toml_str("[analyzer]\nschema_version=1\n[analyzer.unknown]\na=1\n"),
        Err(AnalyzeConfigError::InvalidToml { .. })
    ));
}
// TT-TEST: support
#[test]
fn analyzer_toml_unknown_field_in_known_subgroup_fails() {
    assert!(matches!(
        AnalyzeOptions::from_toml_str(
            "[analyzer]\nschema_version=1\n[analyzer.queueing]\nnope=1\n"
        ),
        Err(AnalyzeConfigError::InvalidToml { .. })
    ));
}
// TT-TEST: support
#[test]
fn analyzer_toml_invalid_type_fails() {
    assert!(matches!(
        AnalyzeOptions::from_toml_str(
            "[analyzer]\nschema_version=1\n[analyzer.queueing]\ntrigger_permille='bad'\n"
        ),
        Err(AnalyzeConfigError::InvalidToml { .. })
    ));
}
// TT-TEST: support
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
// TT-TEST: support
#[test]
fn analyzer_toml_canonical_example_path_parses() {
    let _ = AnalyzeOptions::from_toml_str(include_str!("../../examples/analyzer-config.toml"))
        .expect("canonical repo-root example parse");
}

// TT-TEST: support
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
// TT-TEST: A10 secondary
#[test]
fn prompt09_partial_events_are_now_visible_without_contaminating_completed_percentiles() {
    let completed = partial_policy_run(false, false);
    let mut partial = completed.clone();
    partial.queues[0].completed = false;
    partial.stages[0].completed = false;
    partial.stages[0].success = false;
    let completed_report = analyze_run(&completed, AnalyzeOptions::default())
        .expect("analyzer options should be valid");
    let partial_report =
        analyze_run(&partial, AnalyzeOptions::default()).expect("analyzer options should be valid");
    assert_eq!(
        completed_report.p95_queue_share_permille,
        partial_report.p95_queue_share_permille
    );
    assert!(partial_report.evidence_quality.limitations[0].contains("Partial evidence captured"));
    assert!(partial_report
        .warnings
        .iter()
        .any(|w| w == super::partial_evidence::PARTIAL_WARNING));
}

// TT-TEST: A10 primary
#[test]
fn partial_queue_events_do_not_enter_completed_queue_percentiles() {
    let completed = partial_policy_run(false, false);
    let mut partial = completed.clone();
    let mut q = precise_queue("r0", 0, 1_000, 1_000);
    q.completed = false;
    q.depth_at_start = Some(99);
    partial.queues.push(q);
    let a = analyze_run(&completed, AnalyzeOptions::default())
        .expect("analyzer options should be valid");
    let b =
        analyze_run(&partial, AnalyzeOptions::default()).expect("analyzer options should be valid");
    assert_eq!(a.p95_queue_share_permille, Some(900));
    assert_eq!(b.p95_queue_share_permille, Some(900));
    assert_eq!(a.p95_service_share_permille, Some(100));
    assert_eq!(b.p95_service_share_permille, Some(100));
    assert_eq!(b.evidence_quality.queue_event_count, 46);
    assert_eq!(b.evidence_quality.queues, SignalCoverageStatus::Partial);
    assert_eq!(
        b.warnings
            .iter()
            .filter(|w| w.as_str() == super::partial_evidence::PARTIAL_WARNING)
            .count(),
        1
    );
}

// TT-TEST: A10 primary
#[test]
fn partial_only_queue_evidence_can_rank_queue_suspect_but_caps_confidence() {
    let mut run = partial_policy_run(true, false);
    run.stages.clear();
    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    assert_eq!(report.p95_queue_share_permille, Some(0));
    assert_eq!(
        report.primary_suspect.kind,
        DiagnosisKind::ApplicationQueuePressure
    );
    assert_eq!(report.primary_suspect.score, 95);
    assert_ne!(report.primary_suspect.confidence, Confidence::High);
    assert!(report.primary_suspect.evidence.iter().any(|e| e
        .contains("Observed queue-wait lower bound at p95 is 90.0%")
        && e.contains("45 partial queue event(s)")));
    assert!(report
        .primary_suspect
        .confidence_notes
        .contains(&super::partial_evidence::PARTIAL_QUEUE_CONFIDENCE_NOTE.to_string()));
}

// TT-TEST: A10 primary
#[test]
fn partial_stage_events_do_not_enter_completed_stage_percentiles() {
    let completed = partial_policy_run(false, false);
    let mut partial = completed.clone();
    let mut s = precise_stage("r0", "db", Some(0), Some(1_000), 1_000);
    s.completed = false;
    s.success = false;
    partial.stages.push(s);
    let a = analyze_run(&completed, AnalyzeOptions::default())
        .expect("analyzer options should be valid");
    let b =
        analyze_run(&partial, AnalyzeOptions::default()).expect("analyzer options should be valid");
    let asus = downstream_suspect(&a);
    assert_eq!(asus.score, 95);
    assert_eq!(
        asus.evidence[0],
        "Stage 'db' has p95 latency 900 us across 45 samples."
    );
    let bsus = downstream_suspect(&b);
    assert_eq!(bsus.evidence[0], asus.evidence[0]);
}

// TT-TEST: A10 primary
#[test]
fn partial_only_stage_evidence_can_rank_downstream_suspect_but_caps_confidence() {
    let mut run = partial_policy_run(false, true);
    run.queues.clear();
    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    assert_eq!(
        report.primary_suspect.kind,
        DiagnosisKind::DownstreamStageDominance
    );
    assert_ne!(report.primary_suspect.confidence, Confidence::High);
    assert!(report
        .primary_suspect
        .evidence
        .iter()
        .all(|e| !e.contains("failure")));
    assert!(report
        .primary_suspect
        .evidence
        .iter()
        .any(|e| e.contains("observed lower-bound") && e.contains("45 partial stage event(s)")));
    assert!(report
        .primary_suspect
        .confidence_notes
        .contains(&super::partial_evidence::PARTIAL_STAGE_CONFIDENCE_NOTE.to_string()));
}

// TT-TEST: support
#[test]
fn completed_only_report_json_text_scores_and_rankings_remain_exact() {
    let run = partial_policy_run(false, false);
    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    let expected_json = r#"{"request_count":45,"p50_latency_us":1000,"p95_latency_us":1000,"p99_latency_us":1000,"p95_queue_share_permille":900,"p95_service_share_permille":100,"inflight_trend":null,"warnings":["Top suspects are close in score; treat ranking as ambiguous and validate both with next checks."],"evidence_quality":{"request_count":45,"queue_event_count":45,"stage_event_count":45,"runtime_snapshot_count":0,"inflight_snapshot_count":0,"requests":"present","queues":"present","stages":"present","runtime_snapshots":"missing","inflight_snapshots":"missing","truncated":false,"dropped_requests":0,"dropped_stages":0,"dropped_queues":0,"dropped_inflight_snapshots":0,"dropped_runtime_snapshots":0,"quality":"strong","limitations":["Runtime snapshots are missing, limiting executor and blocking-pressure interpretation."]},"primary_suspect":{"kind":"application_queue_pressure","score":95,"confidence":"medium","evidence":["Queue wait at p95 consumes 90.0% of request time.","Observed queue depth sample up to 20."],"next_checks":["Inspect queue admission limits and producer burst patterns.","Compare queue wait distribution before and after increasing worker parallelism."],"confidence_notes":["Top suspects are close in score; confidence is capped by ambiguity."]},"secondary_suspects":[{"kind":"downstream_stage_dominance","score":95,"confidence":"medium","evidence":["Stage 'db' has p95 latency 900 us across 45 samples.","Stage 'db' cumulative latency is 40500 us (900 permille of request latency).","Stage 'db' contributes 900 permille of tail request latency."],"next_checks":["Inspect downstream dependency behind stage 'db'.","Collect downstream service timings and retry behavior during tail windows.","Review downstream SLO/error budget and align retry budget/backoff with it."],"confidence_notes":["Top suspects are close in score; confidence is capped by ambiguity."]}],"route_breakdowns":[],"temporal_segments":[]}"#;
    assert_eq!(render_json(&report).unwrap(), expected_json);
    let text = render_text(&report);
    let expected_text = "tailtriage diagnosis\nRequests analyzed: 45\nLatency (us): p50 1000, p95 1000, p99 1000\nRequest time at p95: queue 90.0%, non-queue service 10.0%\nInflight trend: none\nPrimary suspect: application queue pressure (medium confidence, score 95)\nEvidence quality: strong (Runtime snapshots are missing, limiting executor and blocking-pressure interpretation.)\nWarnings:\n- Top suspects are close in score; treat ranking as ambiguous and validate both with next checks.\nEvidence:\n- Queue wait at p95 consumes 90.0% of request time.\n- Observed queue depth sample up to 20.\nNext checks:\n- Inspect queue admission limits and producer burst patterns.\n- Compare queue wait distribution before and after increasing worker parallelism.\nSecondary suspects:\n- downstream stage dominance (medium confidence, score 95)";
    assert_eq!(text, expected_text);
    assert_eq!(
        report.primary_suspect.kind,
        DiagnosisKind::ApplicationQueuePressure
    );
    assert_eq!(report.primary_suspect.score, 95);
    assert_eq!(report.primary_suspect.confidence, Confidence::Medium);
    assert_eq!(
        report.primary_suspect.evidence,
        vec![
            "Queue wait at p95 consumes 90.0% of request time.".to_string(),
            "Observed queue depth sample up to 20.".to_string()
        ]
    );
    assert_eq!(
        report.primary_suspect.confidence_notes,
        vec!["Top suspects are close in score; confidence is capped by ambiguity.".to_string()]
    );
    assert_eq!(
        report
            .secondary_suspects
            .iter()
            .map(|s| s.kind.clone())
            .collect::<Vec<_>>(),
        vec![DiagnosisKind::DownstreamStageDominance]
    );
    assert_eq!(report.warnings, vec!["Top suspects are close in score; treat ranking as ambiguous and validate both with next checks.".to_string()]);
    assert_eq!(report.evidence_quality.limitations, vec!["Runtime snapshots are missing, limiting executor and blocking-pressure interpretation.".to_string()]);
    assert_eq!(report.p95_queue_share_permille, Some(900));
    assert_eq!(report.p95_service_share_permille, Some(100));
}

// TT-TEST: support
#[test]
fn completed_only_warning_and_limitation_order_remains_stable() {
    let mut run = partial_policy_run(false, false);
    run.truncation.dropped_requests = 1;
    run.truncation.dropped_queues = 1;
    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    assert_eq!(report.warnings, vec![
        "Capture limits were hit during this run; dropped evidence can reduce diagnosis completeness and confidence.".to_string(),
        "Capture truncated requests: dropped 1 request events after reaching the configured max_requests limit. This dropped evidence can reduce diagnosis completeness and confidence.".to_string(),
        "Capture truncated queues: dropped 1 queue events after reaching the configured max_queues limit. This dropped evidence can reduce diagnosis completeness and confidence.".to_string(),
        "Top suspects are close in score; treat ranking as ambiguous and validate both with next checks.".to_string(),
    ]);
    assert_eq!(
        report.evidence_quality.limitations,
        vec![
            "Runtime snapshots are missing, limiting executor and blocking-pressure interpretation.".to_string(),
            "Capture truncation dropped evidence and can reduce diagnosis completeness.".to_string(),
        ]
    );
}
// TT-TEST: support
#[test]
fn mixed_queue_evidence_uses_higher_basis_and_labels_material_partial_reliance() {
    let mut run = partial_policy_run(false, false);
    for i in 20..45 {
        let q = &mut run.queues[i];
        q.completed = false;
        q.wait_us = 900;
        q.waited_from_run_us = Some(0);
        q.waited_until_run_us = Some(900);
    }
    for i in 0..20 {
        let q = &mut run.queues[i];
        q.wait_us = 300;
        q.waited_from_run_us = Some(0);
        q.waited_until_run_us = Some(300);
    }

    let profile = super::partial_evidence::PartialEvidenceProfile::from_run(&run);
    assert_eq!(profile.queues.completed, 20);
    assert_eq!(profile.queues.partial, 25);

    let shares = super::request_time_shares(&run);
    let completed_p95 = super::percentile(&shares.completed_queue, 95, 100).unwrap();
    let observed_p95 = super::percentile(&shares.observed_queue, 95, 100).unwrap();
    assert_eq!(completed_p95, 300);
    assert_eq!(observed_p95, 900);

    let completed = super::scoring::queue_candidate_for_test(
        &run,
        &shares.completed_queue,
        true,
        Some(completed_p95),
        &AnalyzeOptions::default(),
    )
    .expect("completed queue candidate");
    let observed = super::scoring::queue_candidate_for_test(
        &run,
        &shares.observed_queue,
        false,
        Some(completed_p95),
        &AnalyzeOptions::default(),
    )
    .expect("observed queue candidate");
    assert_eq!(completed.suspect.score, 56);
    assert_eq!(observed.suspect.score, 95);
    assert!(observed.suspect.score > completed.suspect.score);

    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    assert_eq!(
        report.primary_suspect.kind,
        DiagnosisKind::ApplicationQueuePressure
    );
    assert_eq!(report.primary_suspect.score, observed.suspect.score);
    assert_eq!(report.p95_queue_share_permille, Some(300));
    assert_eq!(report.p95_service_share_permille, Some(1000));
    assert_eq!(
        report.primary_suspect.evidence,
        vec![
            "Completed-only queue wait at p95 is 30.0% of request time.".to_string(),
            "Observed queue-wait lower bound at p95 is 90.0% of request time and includes 25 partial queue event(s).".to_string(),
            "Observed queue depth sample up to 20.".to_string(),
        ]
    );
    assert_eq!(report.primary_suspect.confidence, Confidence::Medium);
    assert_eq!(
        report.primary_suspect.confidence_notes,
        vec![
            super::partial_evidence::PARTIAL_QUEUE_CONFIDENCE_NOTE.to_string(),
            "Top suspects are close in score; confidence is capped by ambiguity.".to_string(),
        ]
    );
}

// TT-TEST: support
#[test]
fn fully_overlapped_partial_queue_does_not_cap_completed_candidate() {
    let mut run = partial_policy_run(false, false);
    let mut q = precise_queue("r0", 0, 900, 900);
    q.completed = false;
    q.depth_at_start = Some(20);
    run.queues.push(q);
    let profile = super::partial_evidence::PartialEvidenceProfile::from_run(&run);
    assert_eq!(profile.queues.completed, 45);
    assert_eq!(profile.queues.partial, 1);
    let shares = super::request_time_shares(&run);
    let completed_p95 = super::percentile(&shares.completed_queue, 95, 100).unwrap();
    let completed = super::scoring::queue_candidate_for_test(
        &run,
        &shares.completed_queue,
        true,
        Some(completed_p95),
        &AnalyzeOptions::default(),
    )
    .expect("completed queue candidate");
    let observed = super::scoring::queue_candidate_for_test(
        &run,
        &shares.observed_queue,
        false,
        Some(completed_p95),
        &AnalyzeOptions::default(),
    )
    .expect("observed queue candidate");
    assert_eq!(completed.suspect.score, observed.suspect.score);

    let r = analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    assert_eq!(
        r.primary_suspect.kind,
        DiagnosisKind::ApplicationQueuePressure
    );
    assert_eq!(r.primary_suspect.score, completed.suspect.score);
    assert_eq!(r.primary_suspect.evidence, completed.suspect.evidence);
    assert!(r
        .primary_suspect
        .evidence
        .iter()
        .any(|e| e.starts_with("Queue wait at p95 consumes")));
    assert!(r
        .primary_suspect
        .evidence
        .iter()
        .all(|e| !e.contains("Observed queue-wait lower bound")));
    assert!(!r
        .primary_suspect
        .confidence_notes
        .contains(&super::partial_evidence::PARTIAL_QUEUE_CONFIDENCE_NOTE.to_string()));
    let completed_only_confidence =
        analyze_run(&partial_policy_run(false, false), AnalyzeOptions::default())
            .expect("analyzer options should be valid")
            .primary_suspect
            .confidence;
    assert_eq!(r.primary_suspect.confidence, completed_only_confidence);
    assert_eq!(r.evidence_quality.queues, SignalCoverageStatus::Partial);
    assert_eq!(r.evidence_quality.queue_event_count, 46);
    assert_eq!(r.evidence_quality.limitations[0], "Partial evidence captured: queues 45 completed/1 partial; stages 45 completed/0 partial. Partial durations are observed lower bounds.");
}

// TT-TEST: support
#[test]
fn mixed_stage_evidence_uses_higher_basis_and_labels_material_partial_reliance() {
    let mut run = partial_policy_run(false, false);
    for i in 0..20 {
        let s = &mut run.stages[i];
        s.latency_us = 300;
        s.started_at_run_us = Some(0);
        s.finished_at_run_us = Some(300);
    }
    for i in 20..45 {
        let s = &mut run.stages[i];
        s.completed = false;
        s.success = false;
        s.latency_us = 900;
        s.started_at_run_us = Some(0);
        s.finished_at_run_us = Some(900);
    }
    let profile = super::partial_evidence::PartialEvidenceProfile::from_run(&run);
    assert_eq!(profile.stages.completed, 20);
    assert_eq!(profile.stages.partial, 25);
    let candidates = super::scoring::downstream_stage_candidates_for_test(
        &run,
        1_000,
        &AnalyzeOptions::default(),
    );
    let completed = candidates
        .iter()
        .find(|c| c.0 == super::partial_evidence::EvidenceBasis::Completed);
    let observed = candidates
        .iter()
        .find(|c| c.0 == super::partial_evidence::EvidenceBasis::ObservedLowerBound)
        .expect("observed stage candidate");
    assert!(completed.is_none());
    assert_eq!(observed.2, 45);
    assert_eq!(observed.3, 900);
    assert_eq!(observed.4, 28_500);
    assert_eq!(observed.5, 633);
    assert_eq!(observed.6, 633);
    assert_eq!(observed.7, 95);

    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    let suspect = downstream_suspect(&report);
    assert_eq!(suspect.score, observed.7);
    assert_eq!(suspect.evidence, vec![
        "Stage 'db' observed lower-bound p95 latency is 900 us across 45 samples and includes 25 partial stage event(s).".to_string(),
        "Stage 'db' observed lower-bound cumulative latency is 28500 us (633 permille of request latency).".to_string(),
        "Stage 'db' observed lower-bound contribution is 633 permille of tail request latency.".to_string(),
    ]);
    assert_eq!(suspect.confidence, Confidence::Medium);
    assert_eq!(
        suspect.confidence_notes,
        vec![
            super::partial_evidence::PARTIAL_STAGE_CONFIDENCE_NOTE.to_string(),
            "Top suspects are close in score; confidence is capped by ambiguity.".to_string(),
        ]
    );
}

// TT-TEST: support
#[test]
fn fully_overlapped_partial_stage_does_not_cap_completed_candidate() {
    let mut run = partial_policy_run(false, false);
    let mut s = precise_stage("r0", "db", Some(0), Some(900), 900);
    s.completed = false;
    s.success = false;
    run.stages.push(s);
    let profile = super::partial_evidence::PartialEvidenceProfile::from_run(&run);
    assert_eq!(profile.stages.completed, 45);
    assert_eq!(profile.stages.partial, 1);
    let candidates = super::scoring::downstream_stage_candidates_for_test(
        &run,
        1_000,
        &AnalyzeOptions::default(),
    );
    let completed = candidates
        .iter()
        .find(|c| c.0 == super::partial_evidence::EvidenceBasis::Completed)
        .expect("completed stage candidate");
    let observed = candidates
        .iter()
        .find(|c| c.0 == super::partial_evidence::EvidenceBasis::ObservedLowerBound)
        .expect("observed stage candidate");
    assert_eq!(completed.7, observed.7);

    let r = analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    let d = downstream_suspect(&r);
    assert_eq!(d.score, completed.7);
    assert!(d
        .evidence
        .iter()
        .any(|e| e == "Stage 'db' has p95 latency 900 us across 45 samples."));
    assert!(d
        .evidence
        .iter()
        .all(|e| !e.contains("observed lower-bound")));
    assert!(!d
        .confidence_notes
        .contains(&super::partial_evidence::PARTIAL_STAGE_CONFIDENCE_NOTE.to_string()));
    let completed_only_confidence = downstream_suspect(
        &analyze_run(&partial_policy_run(false, false), AnalyzeOptions::default())
            .expect("analyzer options should be valid"),
    )
    .confidence;
    assert_eq!(d.confidence, completed_only_confidence);
    assert_eq!(r.evidence_quality.stages, SignalCoverageStatus::Partial);
    assert_eq!(r.evidence_quality.stage_event_count, 46);
    assert_eq!(r.evidence_quality.limitations[0], "Partial evidence captured: queues 45 completed/0 partial; stages 45 completed/1 partial. Partial durations are observed lower bounds.");
}
// TT-TEST: support
#[test]
fn partial_counts_and_signal_statuses_are_deterministic() {
    let run = partial_policy_run(true, true);
    let r = analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    assert_eq!(r.evidence_quality.queue_event_count, 45);
    assert_eq!(r.evidence_quality.stage_event_count, 45);
    assert_eq!(r.evidence_quality.queues, SignalCoverageStatus::Partial);
    assert_eq!(r.evidence_quality.stages, SignalCoverageStatus::Partial);
    assert_eq!(r.evidence_quality.quality, EvidenceQualityLevel::Partial);
    assert_eq!(r.evidence_quality.limitations[0],"Partial evidence captured: queues 0 completed/45 partial; stages 0 completed/45 partial. Partial durations are observed lower bounds.");
}
// TT-TEST: A07 primary
#[test]
fn partial_and_truncated_family_reports_truncated_status() {
    let mut run = partial_policy_run(true, true);
    run.truncation.dropped_queues = 1;
    let r = analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    assert_eq!(r.evidence_quality.queues, SignalCoverageStatus::Truncated);
}
fn scoped_partial_acceptance_run() -> Run {
    let mut run = test_run();
    run.requests.clear();
    run.queues.clear();
    run.stages.clear();
    for i in 0_u64..40 {
        let partial = i >= 20;
        let route = if partial { "/partial" } else { "/completed" };
        let id = format!("r{i}");
        let start = i * 2_000;
        let mut req = precise_request(&id, 1_000);
        req.route = route.into();
        req.started_at_unix_ms = 10 + i;
        req.finished_at_unix_ms = 11 + i;
        req.started_at_run_us = Some(start);
        req.finished_at_run_us = Some(start + 1_000);
        run.requests.push(req);
        let mut q = precise_queue(
            &id,
            start,
            start + if partial { 900 } else { 300 },
            if partial { 900 } else { 300 },
        );
        q.waited_from_unix_ms = 10 + i;
        q.waited_until_unix_ms = 10 + i;
        q.depth_at_start = Some(20);
        q.completed = !partial;
        run.queues.push(q);
        let mut st = precise_stage(
            &id,
            "db",
            Some(start),
            Some(start + if partial { 900 } else { 300 }),
            if partial { 900 } else { 300 },
        );
        st.started_at_unix_ms = 10 + i;
        st.finished_at_unix_ms = 10 + i;
        st.completed = !partial;
        st.success = !partial;
        run.stages.push(st);
    }
    run
}

fn assert_no_duplicate_warnings(values: &[String]) {
    let set: std::collections::BTreeSet<_> = values.iter().collect();
    assert_eq!(values.len(), set.len(), "duplicate warnings: {values:?}");
}

// TT-TEST: support
#[test]
fn global_partial_warning_is_emitted_once_and_not_copied_to_nested_warnings() {
    let run = scoped_partial_acceptance_run();
    let r = analyze_run(&run, {
        let mut options = AnalyzeOptions::default();
        {
            let o = &mut options.route;
            o.min_request_count = 10;
        }
        {
            let o = &mut options.temporal;
            o.min_request_count = 20;
            o.min_segment_request_count = 10;
        }
        options
    })
    .expect("analyzer options should be valid");
    assert_eq!(r.route_breakdowns.len(), 2);
    assert_eq!(r.temporal_segments.len(), 2);
    assert_eq!(r.warnings, vec![
        "Top suspects are close in score; treat ranking as ambiguous and validate both with next checks.".to_string(),
        super::partial_evidence::PARTIAL_WARNING.to_string(),
        "Different routes show different primary suspects; inspect route_breakdowns before acting on the global suspect.".to_string(),
        "Temporal segments show different primary suspects; inspect temporal_segments before acting on the global suspect.".to_string(),
    ]);
    assert_eq!(
        r.warnings
            .iter()
            .filter(|w| w.as_str() == super::partial_evidence::PARTIAL_WARNING)
            .count(),
        1
    );
    assert_no_duplicate_warnings(&r.warnings);
    for rb in &r.route_breakdowns {
        assert_no_duplicate_warnings(&rb.warnings);
        assert_eq!(
            rb.warnings
                .iter()
                .filter(|w| w.as_str() == super::partial_evidence::PARTIAL_WARNING)
                .count(),
            0
        );
    }
    for ts in &r.temporal_segments {
        assert_no_duplicate_warnings(&ts.warnings);
        assert_eq!(
            ts.warnings
                .iter()
                .filter(|w| w.as_str() == super::partial_evidence::PARTIAL_WARNING)
                .count(),
            0
        );
    }
}

fn assert_completed_scoped_projection(report: &Report, name: &str, route_warning: bool) {
    assert_eq!(report.request_count, 20, "{name}");
    assert_eq!(report.evidence_quality.queue_event_count, 20);
    assert_eq!(report.evidence_quality.stage_event_count, 20);
    assert_eq!(
        report.evidence_quality.queues,
        SignalCoverageStatus::Present
    );
    assert_eq!(
        report.evidence_quality.stages,
        SignalCoverageStatus::Present
    );
    assert_eq!(
        report.evidence_quality.quality,
        EvidenceQualityLevel::Strong
    );
    assert_eq!(report.evidence_quality.limitations, vec!["Runtime snapshots are missing, limiting executor and blocking-pressure interpretation.".to_string()]);
    assert_eq!(report.p95_queue_share_permille, Some(300));
    assert_eq!(report.p95_service_share_permille, Some(700));
    assert_eq!(
        report.primary_suspect.kind,
        DiagnosisKind::DownstreamStageDominance
    );
    assert_eq!(report.primary_suspect.score, 59);
    assert_eq!(report.primary_suspect.confidence, Confidence::Low);
    assert_eq!(
        report.primary_suspect.evidence,
        vec![
            "Stage 'db' has p95 latency 300 us across 20 samples.".to_string(),
            "Stage 'db' cumulative latency is 6000 us (300 permille of request latency)."
                .to_string(),
            "Stage 'db' contributes 300 permille of tail request latency.".to_string(),
        ]
    );
    assert!(report.primary_suspect.confidence_notes.is_empty());
    assert!(report
        .primary_suspect
        .evidence
        .iter()
        .all(|e| !e.contains("Observed queue-wait lower bound")
            && !e.contains("observed lower-bound")));
    assert!(report
        .primary_suspect
        .confidence_notes
        .iter()
        .all(|n| !n.contains("Partial queue evidence") && !n.contains("Partial stage evidence")));
    let mut expected = vec![
        "No runtime snapshots captured; executor and blocking-pressure interpretation is limited."
            .to_string(),
    ];
    if route_warning {
        expected.push(
            "Runtime and in-flight signals are global and are not attributed to this route."
                .to_string(),
        );
    }
    assert_eq!(report.warnings, expected);
    assert!(report
        .warnings
        .iter()
        .all(|w| w != super::partial_evidence::PARTIAL_WARNING));
}

fn assert_partial_scoped_projection(report: &Report, name: &str, route_warning: bool) {
    assert_eq!(report.request_count, 20, "{name}");
    assert_eq!(report.evidence_quality.queue_event_count, 20);
    assert_eq!(report.evidence_quality.stage_event_count, 20);
    let profile =
        super::partial_evidence::PartialEvidenceProfile::from_run(&scoped_partial_acceptance_run());
    assert_eq!(profile.queues.completed, 20);
    assert_eq!(profile.queues.partial, 20);
    assert_eq!(
        report.evidence_quality.queues,
        SignalCoverageStatus::Partial
    );
    assert_eq!(
        report.evidence_quality.stages,
        SignalCoverageStatus::Partial
    );
    assert_eq!(
        report.evidence_quality.quality,
        EvidenceQualityLevel::Partial
    );
    assert_eq!(report.evidence_quality.limitations[0], "Partial evidence captured: queues 0 completed/20 partial; stages 0 completed/20 partial. Partial durations are observed lower bounds.");
    assert_eq!(report.p95_queue_share_permille, Some(0));
    assert_eq!(report.p95_service_share_permille, Some(1000));
    assert_eq!(
        report.primary_suspect.kind,
        DiagnosisKind::ApplicationQueuePressure
    );
    assert_eq!(report.primary_suspect.score, 95);
    assert_ne!(report.primary_suspect.confidence, Confidence::High);
    assert_eq!(report.primary_suspect.evidence, vec![
        "Completed-only queue wait at p95 is 0.0% of request time.".to_string(),
        "Observed queue-wait lower bound at p95 is 90.0% of request time and includes 20 partial queue event(s).".to_string(),
        "Observed queue depth sample up to 20.".to_string(),
    ]);
    assert!(report
        .primary_suspect
        .evidence
        .iter()
        .any(|e| e.contains("Observed queue-wait lower bound")));
    assert_eq!(
        report.primary_suspect.confidence_notes,
        vec![
            super::partial_evidence::PARTIAL_QUEUE_CONFIDENCE_NOTE.to_string(),
            "Top suspects are close in score; confidence is capped by ambiguity.".to_string(),
        ]
    );
    let mut expected = vec!["Top suspects are close in score; treat ranking as ambiguous and validate both with next checks.".to_string()];
    if route_warning {
        expected.push(
            "Runtime and in-flight signals are global and are not attributed to this route."
                .to_string(),
        );
    }
    assert_eq!(report.warnings, expected);
    assert!(report
        .warnings
        .iter()
        .all(|w| w != super::partial_evidence::PARTIAL_WARNING));
}

// TT-TEST: A10 secondary
#[test]
fn route_breakdowns_apply_completed_distribution_and_partial_confidence_policy() {
    let run = scoped_partial_acceptance_run();
    assert_eq!(
        run.requests
            .iter()
            .filter(|r| r.route == "/completed")
            .count(),
        20
    );
    assert_eq!(
        run.requests
            .iter()
            .filter(|r| r.route == "/partial")
            .count(),
        20
    );
    assert!(run
        .stages
        .iter()
        .filter(|s| s.request_id == "r0")
        .all(|s| s.completed && s.success));
    assert!(run
        .stages
        .iter()
        .filter(|s| s.request_id == "r20")
        .all(|s| !s.completed && !s.success));
    let report = analyze_run(&run, {
        let mut options = AnalyzeOptions::default();
        {
            let o = &mut options.route;
            o.min_request_count = 10;
        }
        options
    })
    .expect("analyzer options should be valid");
    assert_eq!(report.route_breakdowns.len(), 2);
    let completed = report
        .route_breakdowns
        .iter()
        .find(|r| r.route == "/completed")
        .unwrap();
    assert_completed_scoped_projection(
        &Report {
            request_count: completed.request_count,
            p50_latency_us: completed.p50_latency_us,
            p95_latency_us: completed.p95_latency_us,
            p99_latency_us: completed.p99_latency_us,
            p95_queue_share_permille: completed.p95_queue_share_permille,
            p95_service_share_permille: completed.p95_service_share_permille,
            inflight_trend: None,
            warnings: completed.warnings.clone(),
            evidence_quality: completed.evidence_quality.clone(),
            primary_suspect: completed.primary_suspect.clone(),
            secondary_suspects: completed.secondary_suspects.clone(),
            related_groups: vec![],
            route_breakdowns: vec![],
            temporal_segments: vec![],
            analyzer_config: None,
        },
        "/completed",
        true,
    );
    assert_eq!(completed.route, "/completed");
    assert_eq!(
        completed.secondary_suspects[0].kind,
        DiagnosisKind::ApplicationQueuePressure
    );
    let partial = report
        .route_breakdowns
        .iter()
        .find(|r| r.route == "/partial")
        .unwrap();
    assert_eq!(partial.route, "/partial");
    assert_partial_scoped_projection(
        &Report {
            request_count: partial.request_count,
            p50_latency_us: partial.p50_latency_us,
            p95_latency_us: partial.p95_latency_us,
            p99_latency_us: partial.p99_latency_us,
            p95_queue_share_permille: partial.p95_queue_share_permille,
            p95_service_share_permille: partial.p95_service_share_permille,
            inflight_trend: None,
            warnings: partial.warnings.clone(),
            evidence_quality: partial.evidence_quality.clone(),
            primary_suspect: partial.primary_suspect.clone(),
            secondary_suspects: partial.secondary_suspects.clone(),
            related_groups: vec![],
            route_breakdowns: vec![],
            temporal_segments: vec![],
            analyzer_config: None,
        },
        "/partial",
        true,
    );
}

// TT-TEST: A10 secondary
#[test]
fn temporal_segments_apply_completed_distribution_and_partial_confidence_policy() {
    let run = scoped_partial_acceptance_run();
    let report = analyze_run(&run, {
        let mut options = AnalyzeOptions::default();
        {
            let o = &mut options.temporal;
            o.min_request_count = 20;
            o.min_segment_request_count = 10;
        }
        options
    })
    .expect("analyzer options should be valid");
    assert_eq!(report.temporal_segments.len(), 2);
    assert!(report.warnings.iter().any(|w| w == "Temporal segments show different primary suspects; inspect temporal_segments before acting on the global suspect."));
    let early = report
        .temporal_segments
        .iter()
        .find(|s| s.name == "early")
        .unwrap();
    assert_eq!(early.name, "early");
    assert_completed_scoped_projection(
        &Report {
            request_count: early.request_count,
            p50_latency_us: early.p50_latency_us,
            p95_latency_us: early.p95_latency_us,
            p99_latency_us: early.p99_latency_us,
            p95_queue_share_permille: early.p95_queue_share_permille,
            p95_service_share_permille: early.p95_service_share_permille,
            inflight_trend: None,
            warnings: early.warnings.clone(),
            evidence_quality: early.evidence_quality.clone(),
            primary_suspect: early.primary_suspect.clone(),
            secondary_suspects: early.secondary_suspects.clone(),
            related_groups: vec![],
            route_breakdowns: vec![],
            temporal_segments: vec![],
            analyzer_config: None,
        },
        "early",
        false,
    );
    let late = report
        .temporal_segments
        .iter()
        .find(|s| s.name == "late")
        .unwrap();
    assert_eq!(late.name, "late");
    assert_partial_scoped_projection(
        &Report {
            request_count: late.request_count,
            p50_latency_us: late.p50_latency_us,
            p95_latency_us: late.p95_latency_us,
            p99_latency_us: late.p99_latency_us,
            p95_queue_share_permille: late.p95_queue_share_permille,
            p95_service_share_permille: late.p95_service_share_permille,
            inflight_trend: None,
            warnings: late.warnings.clone(),
            evidence_quality: late.evidence_quality.clone(),
            primary_suspect: late.primary_suspect.clone(),
            secondary_suspects: late.secondary_suspects.clone(),
            related_groups: vec![],
            route_breakdowns: vec![],
            temporal_segments: vec![],
            analyzer_config: None,
        },
        "late",
        false,
    );
}

// TT-TEST: support
#[test]
fn cancelled_requests_with_partial_children_are_qualified_without_fabricated_failure() {
    let mut run = partial_policy_run(false, true);
    run.queues.clear();
    run.requests[7].outcome = "cancelled".into();
    let r7: Vec<_> = run
        .requests
        .iter()
        .filter(|r| r.request_id == "r7")
        .collect();
    assert_eq!(r7.len(), 1);
    assert_eq!(r7[0].outcome, "cancelled");
    let child = run.stages.iter().find(|s| s.request_id == "r7").unwrap();
    assert_eq!(child.request_id, "r7");
    assert!(!child.completed);
    assert!(!child.success);
    let r = analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    assert_eq!(r.request_count, 45);
    assert_eq!(
        run.requests
            .iter()
            .filter(|req| req.request_id == "r7" && req.outcome == "cancelled")
            .count(),
        1
    );
    assert_eq!(r.evidence_quality.stage_event_count, 45);
    assert_eq!(r.evidence_quality.stages, SignalCoverageStatus::Partial);
    assert_eq!(r.evidence_quality.quality, EvidenceQualityLevel::Partial);
    assert_eq!(r.evidence_quality.limitations[0], "Partial evidence captured: queues 0 completed/0 partial; stages 0 completed/45 partial. Partial durations are observed lower bounds.");
    assert_eq!(
        r.warnings
            .iter()
            .filter(|w| w.as_str() == super::partial_evidence::PARTIAL_WARNING)
            .count(),
        1
    );
    assert_eq!(
        r.primary_suspect.kind,
        DiagnosisKind::DownstreamStageDominance
    );
    assert_eq!(r.primary_suspect.score, 95);
    assert_ne!(r.primary_suspect.confidence, Confidence::High);
    assert_eq!(
        r.primary_suspect.confidence_notes,
        vec![super::partial_evidence::PARTIAL_STAGE_CONFIDENCE_NOTE.to_string()]
    );
    assert_eq!(r.primary_suspect.evidence, vec![
        "Stage 'db' observed lower-bound p95 latency is 900 us across 45 samples and includes 45 partial stage event(s).".to_string(),
        "Stage 'db' observed lower-bound cumulative latency is 40500 us (900 permille of request latency).".to_string(),
        "Stage 'db' observed lower-bound contribution is 900 permille of tail request latency.".to_string(),
    ]);
    let text = r
        .primary_suspect
        .evidence
        .iter()
        .chain(r.primary_suspect.confidence_notes.iter())
        .cloned()
        .collect::<Vec<_>>()
        .join("\n")
        .to_lowercase();
    for forbidden in [
        "failed stage",
        "stage failure",
        "operation failed",
        "downstream failure",
        "completed failure",
        "error result",
    ] {
        assert!(
            !text.contains(forbidden),
            "fabricated failure wording {forbidden}: {text}"
        );
    }
}

// TT-TEST: support
#[test]
fn validation_corpus_completed_defaults_and_partial_flags_deserialize() {
    let paths = [
        "validation/diagnostics/corpus/partial-evidence-completed-only.json",
        "validation/diagnostics/corpus/partial-evidence-mixed.json",
        "validation/diagnostics/corpus/partial-evidence-queue-only.json",
        "validation/diagnostics/corpus/partial-evidence-stage-only.json",
        "validation/diagnostics/corpus/cancelled-request-with-partial-child.json",
    ];
    let mut saw_omitted_completed_default = false;
    let mut saw_explicit_partial = false;
    for path in paths {
        let raw = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .join(path),
        )
        .unwrap();
        let run: Run = serde_json::from_str(&raw).unwrap();
        let value: serde_json::Value = serde_json::from_str(&raw).unwrap();
        if let Some(raw_events) = value.get("queues").and_then(|v| v.as_array()) {
            for (idx, raw_event) in raw_events.iter().enumerate() {
                if raw_event.get("completed").is_none() {
                    assert!(run.queues[idx].completed);
                    saw_omitted_completed_default = true;
                } else if raw_event.get("completed") == Some(&serde_json::Value::Bool(false)) {
                    assert!(!run.queues[idx].completed);
                    saw_explicit_partial = true;
                }
            }
        }
        if let Some(raw_events) = value.get("stages").and_then(|v| v.as_array()) {
            for (idx, raw_event) in raw_events.iter().enumerate() {
                if raw_event.get("completed").is_none() {
                    assert!(run.stages[idx].completed);
                    saw_omitted_completed_default = true;
                } else if raw_event.get("completed") == Some(&serde_json::Value::Bool(false)) {
                    assert!(!run.stages[idx].completed);
                    saw_explicit_partial = true;
                }
            }
        }
        let report =
            analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
        assert_eq!(report.request_count, run.requests.len());
    }
    assert!(saw_omitted_completed_default);
    assert!(saw_explicit_partial);
}

// TT-TEST: A02 secondary
#[test]
fn percentiles_use_nearest_rank_selected_values() {
    assert_eq!(super::percentile(&[4, 1, 3, 2], 50, 100), Some(2));
    assert_eq!(
        super::percentile(&(1..=20).collect::<Vec<_>>(), 95, 100),
        Some(19)
    );
    assert_eq!(
        super::percentile(&(1..=100).collect::<Vec<_>>(), 99, 100),
        Some(99)
    );

    let mut one_extreme_outlier = vec![1; 19];
    one_extreme_outlier.push(1_000);
    assert_eq!(super::percentile(&one_extreme_outlier, 95, 100), Some(1));
}

// TT-TEST: A02 secondary
#[test]
fn p95_nearest_rank_differs_at_reviewed_small_sample_boundaries() {
    let differing_lengths = (1_usize..=128)
        .filter(|&len| {
            let samples = (0..u64::try_from(len).unwrap()).collect::<Vec<_>>();
            let nearest_rank = super::percentile(&samples, 95, 100).unwrap();
            let previous_index = (len - 1).saturating_mul(95).div_ceil(100);
            nearest_rank != samples[previous_index]
        })
        .collect::<Vec<_>>();

    assert_eq!(differing_lengths, [20, 40, 60, 80, 100, 120]);
}

// TT-TEST: support
#[test]
fn percentile_absence_edges_remain_none() {
    assert_eq!(super::percentile(&[], 95, 100), None);
    assert_eq!(super::percentile(&[1, 2, 3], 95, 0), None);
}

fn partial_policy_run(queue_partial: bool, stage_partial: bool) -> Run {
    let mut run = test_run();
    run.requests = (0..45)
        .map(|i| precise_request(&format!("r{i}"), 1_000))
        .collect();
    for i in 0..45 {
        let id = format!("r{i}");
        let mut q = precise_queue(&id, 0, 900, 900);
        q.depth_at_start = Some(20);
        q.completed = !queue_partial;
        run.queues.push(q);
        let mut s = precise_stage(&id, "db", Some(0), Some(900), 900);
        s.completed = !stage_partial;
        if stage_partial {
            s.success = false;
        }
        run.stages.push(s);
    }
    run
}

// TT-TEST: A04 primary
#[test]
fn provisional_maturity_boundaries_are_exact() {
    assert_eq!(super::confidence::maturity_cap(7), Confidence::Low);
    assert_eq!(super::confidence::maturity_cap(8), Confidence::Medium);
    assert_eq!(super::confidence::maturity_cap(19), Confidence::Medium);
    assert_eq!(super::confidence::maturity_cap(20), Confidence::High);
}

// TT-TEST: A01 primary
#[test]
fn downstream_materiality_boundary_and_fallback_are_exact() {
    let report_at = |stage_us| {
        let mut run = test_run();
        run.requests = (0..20)
            .map(|i| precise_request(&format!("r{i}"), 1_000))
            .collect();
        run.stages = (0..20)
            .map(|i| precise_stage(&format!("r{i}"), "db", Some(0), Some(stage_us), stage_us))
            .collect();
        analyze_run(&run, AnalyzeOptions::default()).expect("valid default options")
    };

    let below = report_at(299);
    assert_eq!(
        below.primary_suspect.kind,
        DiagnosisKind::InsufficientEvidence
    );
    assert_eq!(below.primary_suspect.score, 50);
    assert!(below.secondary_suspects.is_empty());

    let boundary = report_at(300);
    assert_eq!(
        boundary.primary_suspect.kind,
        DiagnosisKind::DownstreamStageDominance
    );
}

// TT-TEST: A01 primary
#[test]
fn every_isolated_below_boundary_path_has_exact_insufficient_fallback() {
    let assert_fallback = |run: &Run, options: AnalyzeOptions| {
        let report = analyze_run(run, options).expect("locally valid options");
        assert_eq!(
            report.primary_suspect.kind,
            DiagnosisKind::InsufficientEvidence
        );
        assert_eq!(report.primary_suspect.score, 50);
        assert!(report.secondary_suspects.is_empty());
    };

    let requests = || {
        (0..20)
            .map(|i| precise_request(&format!("r{i}"), 1_000))
            .collect::<Vec<_>>()
    };

    let mut queue = test_run();
    queue.requests = requests();
    queue.queues = (0..20)
        .map(|i| precise_queue(&format!("r{i}"), 0, 299, 299))
        .collect();
    assert_fallback(&queue, AnalyzeOptions::default());

    let mut blocking = test_run();
    blocking.requests = requests();
    blocking.runtime_snapshots = vec![runtime_snapshot(Some(0), Some(0), Some(0)); 20];
    assert_fallback(&blocking, AnalyzeOptions::default());

    let mut legacy = blocking.clone();
    legacy.runtime_snapshots = vec![runtime_snapshot(Some(1), Some(0), Some(0)); 20];
    let mut legacy_options = AnalyzeOptions::default();
    legacy_options.executor.min_global_queue_p95_for_signal = 2;
    assert_fallback(&legacy, legacy_options);

    let mut normalized = legacy;
    for snapshot in &mut normalized.runtime_snapshots {
        snapshot.worker_count = Some(4);
    }
    let mut normalized_options = AnalyzeOptions::default();
    normalized_options
        .executor
        .min_runnable_queue_per_worker_p95_milli_for_signal = 500;
    assert_fallback(&normalized, normalized_options);

    let mut downstream = test_run();
    downstream.requests = requests();
    downstream.stages = (0..20)
        .map(|i| precise_stage(&format!("r{i}"), "db", Some(0), Some(299), 299))
        .collect();
    assert_fallback(&downstream, AnalyzeOptions::default());

    downstream.stages.truncate(2);
    let mut sample_options = AnalyzeOptions::default();
    sample_options.downstream.min_stage_samples = 3;
    assert_fallback(&downstream, sample_options);
}

// TT-TEST: A02 primary
#[test]
fn relevant_support_units_are_family_specific_and_distinct_from_event_counts() {
    let mut run = test_run();
    run.requests = vec![
        precise_request("q1", 1_000),
        precise_request("q2", 1_000),
        precise_request("none", 1_000),
    ];
    run.queues = vec![
        precise_queue("q1", 0, 400, 400),
        precise_queue("q1", 400, 800, 400),
        precise_queue("q2", 0, 400, 400),
    ];
    let completed = super::scoring::queue_candidate_for_test(
        &run,
        &[800, 400, 0],
        true,
        Some(800),
        &AnalyzeOptions::default(),
    )
    .unwrap();
    assert_eq!(
        completed.relevant_support, 2,
        "distinct contributing requests, not three events or all requests"
    );
    run.queues.push({
        let mut q = precise_queue("none", 0, 500, 500);
        q.completed = false;
        q
    });
    let observed = super::scoring::queue_candidate_for_test(
        &run,
        &[800, 400, 500],
        false,
        Some(800),
        &AnalyzeOptions::default(),
    )
    .unwrap();
    assert_eq!(
        observed.relevant_support, 3,
        "partial-only queue evidence contributes to observed support"
    );

    run.runtime_snapshots = vec![
        runtime_snapshot(Some(4), Some(2), Some(0)),
        runtime_snapshot(Some(4), Some(2), Some(7)),
        runtime_snapshot(None, None, None),
    ];
    assert_eq!(
        super::scoring::blocking_measurement_for_test(&run)
            .unwrap()
            .0,
        2,
        "present zero counts; missing does not"
    );
    let normalized = super::scoring::executor_pressure_suspect(
        &run,
        Some(super::scoring::WorkerEvidenceStatus::Complete {
            worker_count: 2,
            local_complete: true,
        }),
        None,
        &AnalyzeOptions::default(),
    )
    .unwrap();
    assert_eq!(normalized.2, 2);
    let legacy = super::scoring::executor_pressure_suspect(
        &run,
        Some(super::scoring::WorkerEvidenceStatus::HistoricalAbsent),
        None,
        &AnalyzeOptions::default(),
    )
    .unwrap();
    assert_eq!(legacy.2, 2);

    run.stages = vec![
        precise_stage("q1", "db", Some(0), Some(300), 300),
        precise_stage("q1", "db", Some(300), Some(600), 300),
        precise_stage("q2", "db", Some(0), Some(600), 600),
    ];
    let mut options = AnalyzeOptions::default();
    options.downstream.min_stage_samples = 2;
    assert_eq!(
        super::scoring::downstream_stage_suspect(&run, &options)
            .unwrap()
            .relevant_support,
        2
    );
}

// TT-TEST: A02 primary
#[test]
fn support_does_not_add_raw_magnitude_on_any_scoring_path() {
    fn run_with(count: usize) -> Run {
        let mut run = test_run();
        run.requests = (0..count)
            .map(|i| precise_request(&format!("r{i}"), 1_000))
            .collect();
        run.queues = (0..count)
            .map(|i| precise_queue(&format!("r{i}"), 0, 500, 500))
            .collect();
        run.stages = (0..count)
            .map(|i| precise_stage(&format!("r{i}"), "db", Some(0), Some(500), 500))
            .collect();
        run
    }
    let options = AnalyzeOptions::default();
    let (small, large) = (run_with(3), run_with(19));
    let queue_score = |run: &Run| {
        super::scoring::queue_candidate_for_test(
            run,
            &vec![500; run.requests.len()],
            true,
            Some(500),
            &options,
        )
        .unwrap()
        .suspect
        .score
    };
    assert_eq!(queue_score(&small), queue_score(&large));
    let downstream_score = |run: &Run| {
        super::scoring::downstream_stage_suspect(run, &options)
            .unwrap()
            .suspect
            .score
    };
    assert_eq!(downstream_score(&small), downstream_score(&large));

    let runtime_run = |count: usize, workers: Option<u32>| {
        let mut run = run_with(20);
        run.runtime_snapshots = (0..count)
            .map(|_| {
                let mut s = runtime_snapshot(Some(8), Some(4), Some(5));
                s.worker_count = workers;
                s
            })
            .collect();
        run
    };
    let b3 = runtime_run(3, None);
    let b19 = runtime_run(19, None);
    assert_eq!(
        super::scoring::blocking_pressure_suspect(&b3, &options)
            .unwrap()
            .suspect
            .score,
        super::scoring::blocking_pressure_suspect(&b19, &options)
            .unwrap()
            .suspect
            .score
    );
    let score = |run: &Run, status| {
        super::scoring::executor_pressure_suspect(run, Some(status), None, &options)
            .unwrap()
            .0
            .score
    };
    assert_eq!(
        score(&b3, super::scoring::WorkerEvidenceStatus::HistoricalAbsent),
        score(&b19, super::scoring::WorkerEvidenceStatus::HistoricalAbsent)
    );
    let n3 = runtime_run(3, Some(4));
    let n19 = runtime_run(19, Some(4));
    assert_eq!(
        score(
            &n3,
            super::scoring::WorkerEvidenceStatus::Complete {
                worker_count: 4,
                local_complete: true
            }
        ),
        score(
            &n19,
            super::scoring::WorkerEvidenceStatus::Complete {
                worker_count: 4,
                local_complete: true
            }
        )
    );
}

// TT-TEST: A02 primary
#[test]
fn unrelated_family_evidence_does_not_rewrite_raw_magnitude() {
    let options = AnalyzeOptions::default();
    let mut base = test_run();
    base.requests = (0..20)
        .map(|i| precise_request(&format!("r{i}"), 1_000))
        .collect();
    base.queues = (0..20)
        .map(|i| precise_queue(&format!("r{i}"), 0, 600, 600))
        .collect();
    base.stages = (0..20)
        .map(|i| precise_stage(&format!("r{i}"), "db", Some(0), Some(700), 700))
        .collect();
    base.runtime_snapshots = vec![runtime_snapshot(Some(8), Some(4), Some(8)); 20];

    let queue_score = |run: &Run| {
        super::scoring::queue_candidate_for_test(run, &[600; 20], true, Some(600), &options)
            .unwrap()
            .suspect
            .score
    };
    let blocking_score = |run: &Run| {
        super::scoring::blocking_pressure_suspect(run, &options)
            .unwrap()
            .suspect
            .score
    };
    let executor_score = |run: &Run, status| {
        super::scoring::executor_pressure_suspect(run, Some(status), None, &options)
            .unwrap()
            .0
            .score
    };
    let downstream_score = |run: &Run| {
        super::scoring::downstream_stage_suspect(run, &options)
            .unwrap()
            .suspect
            .score
    };

    let mut without_runtime = base.clone();
    without_runtime.runtime_snapshots.clear();
    assert_eq!(queue_score(&base), queue_score(&without_runtime));
    assert_eq!(downstream_score(&base), downstream_score(&without_runtime));

    let mut without_request_families = base.clone();
    without_request_families.queues.clear();
    without_request_families.stages.clear();
    assert_eq!(
        blocking_score(&base),
        blocking_score(&without_request_families)
    );
    assert_eq!(
        executor_score(
            &base,
            super::scoring::WorkerEvidenceStatus::HistoricalAbsent
        ),
        executor_score(
            &without_request_families,
            super::scoring::WorkerEvidenceStatus::HistoricalAbsent
        )
    );
    for snapshot in &mut base.runtime_snapshots {
        snapshot.worker_count = Some(4);
    }
    for snapshot in &mut without_request_families.runtime_snapshots {
        snapshot.worker_count = Some(4);
    }
    let complete = super::scoring::WorkerEvidenceStatus::Complete {
        worker_count: 4,
        local_complete: true,
    };
    assert_eq!(
        executor_score(&base, complete),
        executor_score(&without_request_families, complete)
    );
}

// TT-TEST: support
#[test]
fn tuning_thresholds_are_directional_at_valid_local_values() {
    let has = |report: &Report, kind: DiagnosisKind| {
        std::iter::once(&report.primary_suspect)
            .chain(&report.secondary_suspects)
            .any(|suspect| suspect.kind == kind)
    };
    let requests = || {
        (0..20)
            .map(|i| precise_request(&format!("r{i}"), 1_000))
            .collect::<Vec<_>>()
    };

    let mut queue = test_run();
    queue.requests = requests();
    queue.queues = (0..20)
        .map(|i| precise_queue(&format!("r{i}"), 0, 500, 500))
        .collect();
    let low = analyze_run(&queue, AnalyzeOptions::default()).unwrap();
    let mut raised = AnalyzeOptions::default();
    raised.queueing.trigger_permille = 501;
    let high = analyze_run(&queue, raised).unwrap();
    assert!(has(&low, DiagnosisKind::ApplicationQueuePressure));
    assert!(!has(&high, DiagnosisKind::ApplicationQueuePressure));

    let mut runtime = test_run();
    runtime.requests = requests();
    runtime.runtime_snapshots = vec![runtime_snapshot(Some(1), Some(1), Some(1)); 20];
    let mut sparse_blocking = runtime.clone();
    for snapshot in sparse_blocking.runtime_snapshots.iter_mut().take(19) {
        snapshot.blocking_queue_depth = Some(0);
    }
    let mut permissive = AnalyzeOptions::default();
    permissive.blocking.min_nonzero_samples_for_signal = 1;
    let low_blocking = analyze_run(&sparse_blocking, permissive).unwrap();
    let mut raised = AnalyzeOptions::default();
    raised.blocking.min_nonzero_samples_for_signal = 2;
    let high = analyze_run(&sparse_blocking, raised).unwrap();
    assert!(has(&low_blocking, DiagnosisKind::BlockingPoolPressure));
    assert!(!has(&high, DiagnosisKind::BlockingPoolPressure));

    let low = analyze_run(&runtime, AnalyzeOptions::default()).unwrap();
    let mut raised = AnalyzeOptions::default();
    raised.executor.min_global_queue_p95_for_signal = 2;
    assert!(has(&low, DiagnosisKind::ExecutorPressure));
    assert!(!has(
        &analyze_run(&runtime, raised).unwrap(),
        DiagnosisKind::ExecutorPressure
    ));

    for snapshot in &mut runtime.runtime_snapshots {
        snapshot.worker_count = Some(2);
        snapshot.global_queue_depth = Some(1);
        snapshot.local_queue_depth = Some(0);
    }
    let low = analyze_run(&runtime, AnalyzeOptions::default()).unwrap();
    let mut raised = AnalyzeOptions::default();
    raised
        .executor
        .min_runnable_queue_per_worker_p95_milli_for_signal = 501;
    assert!(has(&low, DiagnosisKind::ExecutorPressure));
    assert!(!has(
        &analyze_run(&runtime, raised).unwrap(),
        DiagnosisKind::ExecutorPressure
    ));

    let mut stage = test_run();
    stage.requests = requests();
    stage.stages = (0..20)
        .map(|i| precise_stage(&format!("r{i}"), "db", Some(0), Some(700), 700))
        .collect();
    let low = analyze_run(&stage, AnalyzeOptions::default()).unwrap();
    let mut raised = AnalyzeOptions::default();
    raised.downstream.min_stage_samples = 21;
    assert!(has(&low, DiagnosisKind::DownstreamStageDominance));
    assert!(!has(
        &analyze_run(&stage, raised).unwrap(),
        DiagnosisKind::DownstreamStageDominance
    ));
}

// TT-TEST: A02 primary
#[test]
fn clean_extreme_magnitude_is_invariant_across_former_support_cliffs() {
    let extreme_run = |count: usize| {
        let mut run = test_run();
        run.requests = (0..count)
            .map(|i| precise_request(&format!("r{i}"), 1_000))
            .collect();
        run.queues = (0..count)
            .map(|i| {
                let mut queue = precise_queue(&format!("r{i}"), 0, 990, 990);
                queue.depth_at_start = Some(12);
                queue
            })
            .collect();
        run.stages = (0..count)
            .map(|i| precise_stage(&format!("r{i}"), "db", Some(0), Some(980), 980))
            .collect();
        run.inflight = vec![
            inflight("requests", 1, Some(0), 1),
            inflight("requests", 2, Some(1_000_000), 3),
        ];
        run
    };
    let real_score = |report: &Report, kind: DiagnosisKind| {
        std::iter::once(&report.primary_suspect)
            .chain(report.secondary_suspects.iter())
            .find(|suspect| suspect.kind == kind)
            .unwrap()
            .score
    };
    for (below, above, kind) in [
        (19, 20, DiagnosisKind::ApplicationQueuePressure),
        (19, 20, DiagnosisKind::DownstreamStageDominance),
    ] {
        let left = analyze_run(&extreme_run(below), AnalyzeOptions::default()).unwrap();
        let right = analyze_run(&extreme_run(above), AnalyzeOptions::default()).unwrap();
        assert_eq!(real_score(&left, kind.clone()), real_score(&right, kind));
    }

    let legacy_extreme = |count: usize| {
        let mut run = test_run();
        run.requests = (0..20).map(sample_request).collect();
        run.runtime_snapshots = (0..count)
            .map(|_| runtime_snapshot(Some(140), Some(30), Some(200)))
            .collect();
        run
    };
    let legacy_score = |run: &Run| {
        super::scoring::executor_pressure_suspect(
            run,
            Some(super::scoring::WorkerEvidenceStatus::HistoricalAbsent),
            None,
            &AnalyzeOptions::default(),
        )
        .unwrap()
        .0
        .score
    };
    assert_eq!(
        legacy_score(&legacy_extreme(29)),
        legacy_score(&legacy_extreme(30))
    );
}

// TT-TEST: A02 primary
#[test]
fn family_magnitude_is_monotone_bounded_and_normalized_executor_is_scale_invariant() {
    let options = AnalyzeOptions::default();
    let mut prior = 0;
    for share in [300, 500, 700, 985, 1_000] {
        let mut run = test_run();
        run.requests = (0..20).map(sample_request).collect();
        run.queues = (0..20)
            .map(|i| {
                let mut q = precise_queue(&format!("req-{i}"), 0, share, share);
                q.depth_at_start = Some(20);
                q
            })
            .collect();
        let candidate = super::scoring::queue_candidate_for_test(
            &run,
            &[share; 20],
            true,
            Some(share),
            &options,
        )
        .unwrap();
        assert!((prior..=100).contains(&candidate.suspect.score));
        prior = candidate.suspect.score;
    }

    let mut prior = 0;
    for depths in [[1, 1], [4, 8], [16, 24], [24, 40]] {
        let mut run = test_run();
        run.runtime_snapshots = (0..20)
            .map(|i| runtime_snapshot(Some(1), Some(1), Some(depths[i % 2])))
            .collect();
        let score = super::scoring::blocking_pressure_suspect(&run, &options)
            .unwrap()
            .suspect
            .score;
        assert!((prior..=100).contains(&score));
        prior = score;
    }

    let normalized_score = |workers, global, local| {
        let mut run = test_run();
        run.runtime_snapshots = (0..20)
            .map(|_| {
                let mut snapshot = runtime_snapshot(Some(global), Some(local), Some(0));
                snapshot.worker_count = Some(workers);
                snapshot
            })
            .collect();
        super::scoring::executor_pressure_suspect(
            &run,
            Some(super::scoring::WorkerEvidenceStatus::Complete {
                worker_count: workers,
                local_complete: true,
            }),
            None,
            &options,
        )
        .unwrap()
        .0
        .score
    };
    for (a, b) in [((2, 1, 1), (8, 4, 4)), ((2, 8, 8), (4, 16, 16))] {
        let left = normalized_score(a.0, a.1, a.2);
        let right = normalized_score(b.0, b.1, b.2);
        assert_eq!(left, right);
        assert!(left <= 100);
    }

    let downstream = |latency| {
        let mut run = test_run();
        run.requests = (0..20)
            .map(|i| precise_request(&format!("r{i}"), 1_000))
            .collect();
        run.stages = (0..20)
            .map(|i| precise_stage(&format!("r{i}"), "db", Some(0), Some(latency), latency))
            .collect();
        super::scoring::downstream_stage_suspect(&run, &options)
            .unwrap()
            .suspect
            .score
    };
    let scores = [300, 500, 700, 960, 1_000].map(downstream);
    assert!(scores.windows(2).all(|pair| pair[0] <= pair[1]));
    assert!(scores.into_iter().all(|score| score <= 100));
}

// TT-TEST: A04 primary
// TT-TEST: A02 secondary
#[test]
fn relevant_support_can_only_raise_maturity_without_changing_magnitude() {
    let mut last_confidence = Confidence::Low;
    let mut expected_score = None;
    for support in 1..=32 {
        let mut run = test_run();
        run.requests = (0..support)
            .map(|i| precise_request(&format!("r{i}"), 1_000))
            .collect();
        run.queues = (0..support)
            .map(|i| precise_queue(&format!("r{i}"), 0, 700, 700))
            .collect();
        let report = analyze_run(&run, AnalyzeOptions::default()).unwrap();
        let suspect = &report.primary_suspect;
        assert_eq!(suspect.kind, DiagnosisKind::ApplicationQueuePressure);
        assert_eq!(*expected_score.get_or_insert(suspect.score), suspect.score);
        let rank = |confidence| match confidence {
            Confidence::Low => 0,
            Confidence::Medium => 1,
            Confidence::High => 2,
        };
        assert!(rank(suspect.confidence) >= rank(last_confidence));
        last_confidence = suspect.confidence;
    }
}

// TT-TEST: A04 primary
#[test]
fn maturity_caps_real_candidates_and_only_emits_a_material_note() {
    let report = |count| {
        let mut run = test_run();
        run.requests = (0..count)
            .map(|i| precise_request(&format!("r{i}"), 1_000))
            .collect();
        run.queues = (0..count)
            .map(|i| {
                let mut q = precise_queue(&format!("r{i}"), 0, 900, 900);
                q.depth_at_start = Some(20);
                q
            })
            .collect();
        analyze_run(&run, AnalyzeOptions::default()).unwrap()
    };
    let sparse = report(7);
    assert_eq!(sparse.primary_suspect.confidence, Confidence::Low);
    assert!(sparse
        .primary_suspect
        .confidence_notes
        .iter()
        .any(|n| n.contains("provisional maturity")));
    let mature = report(20);
    assert_eq!(mature.primary_suspect.confidence, Confidence::High);
    assert!(!mature
        .primary_suspect
        .confidence_notes
        .iter()
        .any(|n| n.contains("provisional maturity")));
}

// TT-TEST: A10 primary
#[test]
fn queue_representation_resolution_uses_pre_ambiguity_limitations_and_stable_ties() {
    let mut run = test_run();
    run.requests = (0..20).map(sample_request).collect();
    run.queues = (0..20)
        .map(|i| precise_queue(&format!("req-{i}"), 0, 500, 500))
        .collect();
    let candidate = |score, support, basis| {
        let mut c = literal_scored(
            DiagnosisKind::ApplicationQueuePressure,
            score,
            Confidence::High,
        );
        c.relevant_support = support;
        c.basis = basis;
        c
    };
    let selected = super::scoring::select_queue_representation_for_test(
        Some(candidate(
            90,
            20,
            super::partial_evidence::EvidenceBasis::Completed,
        )),
        Some(candidate(
            99,
            30,
            super::partial_evidence::EvidenceBasis::ObservedLowerBound,
        )),
        &run,
        &AnalyzeOptions::default(),
    )
    .unwrap();
    assert_eq!(
        selected.basis,
        super::partial_evidence::EvidenceBasis::Completed,
        "the partial limitation must apply before selection"
    );
    let lower = super::scoring::select_queue_representation_for_test(
        Some(candidate(
            70,
            8,
            super::partial_evidence::EvidenceBasis::Completed,
        )),
        Some(candidate(
            70,
            19,
            super::partial_evidence::EvidenceBasis::ObservedLowerBound,
        )),
        &run,
        &AnalyzeOptions::default(),
    )
    .unwrap();
    assert_eq!(
        lower.basis,
        super::partial_evidence::EvidenceBasis::ObservedLowerBound,
        "legitimately greater support wins within the same capped confidence"
    );
    let tie = super::scoring::select_queue_representation_for_test(
        Some(candidate(
            70,
            20,
            super::partial_evidence::EvidenceBasis::Completed,
        )),
        Some(candidate(
            70,
            20,
            super::partial_evidence::EvidenceBasis::ObservedLowerBound,
        )),
        &run,
        &AnalyzeOptions::default(),
    )
    .unwrap();
    assert_eq!(tie.basis, super::partial_evidence::EvidenceBasis::Completed);
}

// TT-TEST: A10 primary
#[test]
fn downstream_representation_resolution_uses_pre_ambiguity_confidence_and_support() {
    use super::partial_evidence::EvidenceBasis::{Completed, ObservedLowerBound};

    let run = partial_policy_run(false, false);
    let select = |representations: &[super::scoring::DownstreamRepresentationForTest]| {
        super::scoring::select_downstream_representation_for_test(
            representations,
            &run,
            &AnalyzeOptions::default(),
        )
        .expect("a downstream representation should be selected")
    };

    assert_eq!(
        select(&[
            (Completed, "completed", 20, 500, 500, 90),
            (ObservedLowerBound, "partial", 30, 900, 900, 99),
        ])
        .0,
        Completed,
        "the partial-evidence limitation must precede selection, so raw magnitude alone cannot displace completed evidence"
    );
    assert_eq!(
        select(&[
            (Completed, "completed", 8, 500, 500, 70),
            (ObservedLowerBound, "partial", 19, 500, 500, 70),
        ])
        .0,
        ObservedLowerBound,
        "within equal pre-ambiguity confidence, legitimately greater support wins"
    );
    assert_eq!(
        select(&[
            (Completed, "completed", 20, 500, 500, 70),
            (ObservedLowerBound, "partial", 20, 500, 500, 70),
        ])
        .0,
        Completed,
        "an otherwise exact representation tie prefers completed evidence"
    );
}

// TT-TEST: A10 primary
#[test]
fn downstream_representation_residual_order_is_tail_then_cumulative_then_stage() {
    use super::partial_evidence::EvidenceBasis::Completed;

    let run = partial_policy_run(false, false);
    let select = |representations: &[super::scoring::DownstreamRepresentationForTest]| {
        super::scoring::select_downstream_representation_for_test(
            representations,
            &run,
            &AnalyzeOptions::default(),
        )
        .expect("a downstream representation should be selected")
    };

    assert_eq!(
        select(&[
            (Completed, "lower_tail", 20, 600, 900, 70),
            (Completed, "higher_tail", 20, 700, 100, 70),
        ])
        .1,
        "higher_tail"
    );
    assert_eq!(
        select(&[
            (Completed, "lower_cumulative", 20, 700, 600, 70),
            (Completed, "higher_cumulative", 20, 700, 800, 70),
        ])
        .1,
        "higher_cumulative"
    );
    assert_eq!(
        select(&[
            (Completed, "stage_b", 20, 700, 800, 70),
            (Completed, "stage_a", 20, 700, 800, 70),
        ])
        .1,
        "stage_a"
    );
}

// TT-TEST: A10 primary
// TT-TEST: A06 secondary
#[test]
fn downstream_same_family_resolution_precedes_cross_family_ambiguity() {
    let mut run = partial_policy_run(false, false);
    let partial = run
        .stages
        .iter()
        .cloned()
        .map(|mut stage| {
            stage.completed = false;
            stage.success = false;
            stage
        })
        .collect::<Vec<_>>();
    run.stages.extend(partial);

    let candidates = super::scoring::downstream_stage_candidates_for_test(
        &run,
        1_000,
        &AnalyzeOptions::default(),
    );
    assert_eq!(candidates.len(), 2, "both representations are eligible");
    assert!(candidates
        .iter()
        .any(|candidate| candidate.0 == super::partial_evidence::EvidenceBasis::Completed));
    assert!(candidates.iter().any(|candidate| {
        candidate.0 == super::partial_evidence::EvidenceBasis::ObservedLowerBound
    }));

    run.queues.clear();
    let downstream_only = analyze_run(&run, AnalyzeOptions::default()).unwrap();
    assert_eq!(downstream_only.secondary_suspects.len(), 0);
    assert!(downstream_only
        .primary_suspect
        .confidence_notes
        .iter()
        .all(|note| !note.contains("ambiguity")));

    run.queues = (0..45)
        .map(|i| {
            let mut queue = precise_queue(&format!("r{i}"), 0, 900, 900);
            queue.depth_at_start = Some(20);
            queue
        })
        .collect();
    let cross_family = analyze_run(&run, AnalyzeOptions::default()).unwrap();
    let surviving = std::iter::once(&cross_family.primary_suspect)
        .chain(cross_family.secondary_suspects.iter())
        .collect::<Vec<_>>();
    assert_eq!(
        surviving
            .iter()
            .filter(|suspect| suspect.kind == DiagnosisKind::DownstreamStageDominance)
            .count(),
        1,
        "only the selected downstream representation reaches cross-family work"
    );
    assert_eq!(cross_family.secondary_suspects.len(), 1);
    assert!(surviving.iter().all(|suspect| suspect
        .confidence_notes
        .iter()
        .any(|note| note.contains("ambiguity"))));
}

// TT-TEST: A04 primary
#[test]
fn runtime_partial_note_is_emitted_only_when_it_lowers_confidence() {
    let mut run = test_run();
    run.requests = (0..20).map(sample_request).collect();
    run.runtime_snapshots = (0..20)
        .map(|_| runtime_snapshot(Some(5), Some(2), None))
        .collect();
    let quality = evidence::evidence_quality(&run, &AnalyzeOptions::default());
    for (score, expected_note) in [(90, true), (70, false)] {
        let mut candidates = vec![literal_scored(
            DiagnosisKind::BlockingPoolPressure,
            score,
            Confidence::from_score_with_options(score, &AnalyzeOptions::default()),
        )];
        super::confidence::apply_pre_ambiguity_confidence_caps(
            &mut candidates,
            &run,
            &quality,
            &AnalyzeOptions::default(),
        );
        assert_eq!(
            candidates[0]
                .suspect
                .confidence_notes
                .iter()
                .any(|n| n.contains("Runtime snapshots are partial")),
            expected_note
        );
    }
}
