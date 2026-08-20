#![allow(clippy::semicolon_if_nothing_returned)]
#[path = "support/equivalence_harness.rs"]
mod equivalence_harness;
use equivalence_harness::*;
use tailtriage_core::{inspect_run, CaptureLimits, InFlightSnapshot, RuntimeSnapshot};

fn pair(name: &str) -> (tailtriage_core::Run, tailtriage_core::Run) {
    (native_case(name), import_case(name, None))
}
fn pair_array(name: &str) -> [tailtriage_core::Run; 2] {
    let (a, b) = pair(name);
    [a, b]
}
fn case_pair_array(name: &str, limits: Option<CaptureLimits>) -> [tailtriage_core::Run; 2] {
    let (a, b) = case_pair(name, limits);
    [a, b]
}
fn assert_both(name: &str) {
    let (n, t) = pair(name);
    let e = expected_run(name);
    assert_eq!(project_run(&n).unwrap(), e);
    assert_eq!(project_run(&t).unwrap(), e);
    assert_eq!(project_run(&n), project_run(&t));
}
fn assert_reports(name: &str) {
    let (n, t) = pair(name);
    let expected = expected_report(name);
    assert_eq!(report(&n), expected);
    assert_eq!(report(&t), expected);
}
fn limits() -> CaptureLimits {
    CaptureLimits {
        max_requests: 2,
        max_stages: 2,
        max_queues: 2,
        max_inflight_snapshots: 200_000,
        max_runtime_snapshots: 100_000,
    }
}
fn case_pair(
    name: &str,
    limits: Option<CaptureLimits>,
) -> (tailtriage_core::Run, tailtriage_core::Run) {
    let native = limits.map_or_else(
        || native_case(name),
        |l| build_native_case_with_limits(name, l),
    );
    (native, import_case(name, limits))
}
fn assert_case_matches_independent_oracles(name: &str, limits: Option<CaptureLimits>) {
    let (native, tracing) = case_pair(name, limits);
    let expected_run = expected_run(name);
    let expected_report = expected_report(name);
    assert_eq!(project_run(&native).unwrap(), expected_run);
    assert_eq!(project_run(&tracing).unwrap(), expected_run);
    assert_eq!(report(&native), expected_report);
    assert_eq!(report(&tracing), expected_report);
}
// TT-TEST: F03 primary
#[test]
fn precise_native_and_tracing_runs_match_independent_representable_projection() {
    assert_both("precise_route_divergent")
}
// TT-TEST: F03 primary
#[test]
fn precise_native_and_tracing_reports_match_independent_expected_projection() {
    assert_reports("precise_route_divergent")
}
// TT-TEST: F03 primary
#[test]
fn route_breakdowns_match_for_equivalent_native_and_tracing_evidence() {
    assert_case_matches_independent_oracles("precise_route_divergent", None);
    for run in pair_array("precise_route_divergent") {
        let r = typed_report(&run);
        assert_eq!(
            r.route_breakdowns
                .iter()
                .map(|x| x.route.as_str())
                .collect::<Vec<_>>(),
            ["/downstream", "/queued"]
        );
        assert_eq!(
            r.route_breakdowns
                .iter()
                .map(|x| x.request_count)
                .collect::<Vec<_>>(),
            [4, 4]
        );
        assert_eq!(
            r.route_breakdowns
                .iter()
                .map(|x| format!("{:?}", x.primary_suspect.kind))
                .collect::<Vec<_>>(),
            ["DownstreamStageDominates", "ApplicationQueueSaturation"]
        );
        assert_eq!(
            r.route_breakdowns
                .iter()
                .map(|x| x.warnings.len())
                .collect::<Vec<_>>(),
            [2, 3]
        );
        assert_eq!(r.warnings.last().unwrap(), "Different routes show different primary suspects; inspect route_breakdowns before acting on the global suspect.");
    }
}
// TT-TEST: F03 primary
#[test]
fn temporal_segments_match_for_equivalent_native_and_tracing_evidence() {
    assert_case_matches_independent_oracles("precise_temporal_movement", None);
    for run in pair_array("precise_temporal_movement") {
        let r = typed_report(&run);
        let x = &r.temporal_segments;
        assert_eq!(
            x.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(),
            ["early", "late"]
        );
        assert_eq!(
            x.iter().map(|x| x.request_count).collect::<Vec<_>>(),
            [12, 12]
        );
        assert_eq!(
            x.iter().map(|x| x.p95_latency_us).collect::<Vec<_>>(),
            [Some(100_000), Some(120_000)]
        );
        assert_eq!(
            x.iter()
                .map(|x| x.p95_queue_share_permille)
                .collect::<Vec<_>>(),
            [Some(700), Some(41)]
        );
        assert_eq!(
            x.iter()
                .map(|x| x.p95_service_share_permille)
                .collect::<Vec<_>>(),
            [Some(300), Some(958)]
        );
        assert_eq!(
            x.iter()
                .map(|x| format!("{:?}", x.primary_suspect.kind))
                .collect::<Vec<_>>(),
            ["ApplicationQueueSaturation", "DownstreamStageDominates"]
        );
        assert_eq!(
            x.iter().map(|x| x.warnings.len()).collect::<Vec<_>>(),
            [2, 1]
        );
        assert!(r
            .warnings
            .iter()
            .any(|w| w == "Temporal segments show different primary suspects; inspect temporal_segments before acting on the global suspect."));
    }
}
// TT-TEST: support
#[test]
fn duration_only_native_and_tracing_cases_share_core_warning_and_report_semantics() {
    assert_case_matches_independent_oracles("duration_only_legacy", None);
    let expected = [
        ("Requests", 0),
        ("Requests", 1),
        ("Requests", 2),
        ("Requests", 3),
        ("Requests", 4),
        ("Requests", 5),
        ("Stages", 0),
        ("Stages", 1),
        ("Stages", 2),
        ("Stages", 3),
        ("Stages", 4),
        ("Stages", 5),
        ("Queues", 0),
        ("Queues", 1),
        ("Queues", 2),
        ("Queues", 3),
        ("Queues", 4),
        ("Queues", 5),
    ]
    .into_iter()
    .map(|(section, index)| {
        (
            "precise_interval_validation_unavailable",
            "Warning".to_string(),
            section.to_string(),
            Some(index),
            None,
            "run-relative interval is unavailable".to_string(),
        )
    })
    .collect::<Vec<_>>();
    for run in pair_array("duration_only_legacy") {
        let actual = inspect_run(&run)
            .issues
            .into_iter()
            .map(|x| {
                (
                    x.code.as_str(),
                    format!("{:?}", x.severity),
                    format!("{:?}", x.location.section),
                    x.location.index,
                    x.location.field,
                    x.message,
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(actual, expected);
    }
}
// TT-TEST: support
#[test]
#[allow(clippy::too_many_lines)]
fn semantic_limits_retain_the_same_evidence_and_drop_counts() {
    let source = native_case("semantic_retention_limits");
    assert_eq!(
        (
            source.requests.len(),
            source.stages.len(),
            source.queues.len()
        ),
        (5, 5, 5)
    );
    assert!(!source.truncation.limits_hit);
    assert_eq!(
        (
            source.truncation.dropped_requests,
            source.truncation.dropped_stages,
            source.truncation.dropped_queues
        ),
        (0, 0, 0)
    );
    let request_ids = source
        .requests
        .iter()
        .map(|event| event.request_id.as_str())
        .collect::<Vec<_>>();
    let stage_names = source
        .stages
        .iter()
        .map(|event| event.stage.as_str())
        .collect::<Vec<_>>();
    let queue_names = source
        .queues
        .iter()
        .map(|event| event.queue.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        request_ids,
        ["limit-1", "limit-2", "limit-00", "limit-01", "limit-02"]
    );
    assert_eq!(
        stage_names,
        ["stage-1", "stage-2", "stage-00", "stage-01", "stage-02"]
    );
    assert_eq!(
        queue_names,
        ["queue-1", "queue-2", "queue-00", "queue-01", "queue-02"]
    );
    for identities in [&request_ids, &stage_names, &queue_names] {
        let mut lexical = identities.clone();
        lexical.sort_unstable();
        assert_ne!(&lexical[..2], &identities[..2]);
    }
    let expected_tracing_order = [
        ("request", "limit-1", None),
        ("stage", "limit-1", Some("stage-1")),
        ("queue", "limit-1", Some("queue-1")),
        ("request", "limit-2", None),
        ("stage", "limit-2", Some("stage-2")),
        ("queue", "limit-2", Some("queue-2")),
        ("request", "limit-00", None),
        ("stage", "limit-00", Some("stage-00")),
        ("queue", "limit-00", Some("queue-00")),
        ("request", "limit-01", None),
        ("stage", "limit-01", Some("stage-01")),
        ("queue", "limit-01", Some("queue-01")),
        ("request", "limit-02", None),
        ("stage", "limit-02", Some("stage-02")),
        ("queue", "limit-02", Some("queue-02")),
    ];
    let tracing_order = tracing_fixture_semantic_order("semantic_retention_limits");
    assert_eq!(
        tracing_order,
        expected_tracing_order.map(|(kind, request, identity)| (
            kind.to_owned(),
            request.to_owned(),
            identity.map(str::to_owned)
        ))
    );
    assert_case_matches_independent_oracles("semantic_retention_limits", Some(limits()));
    for run in case_pair_array("semantic_retention_limits", Some(limits())) {
        assert_eq!(
            run.requests
                .iter()
                .map(|x| x.request_id.as_str())
                .collect::<Vec<_>>(),
            ["limit-1", "limit-2"]
        );
        assert_eq!(
            run.stages
                .iter()
                .map(|x| x.stage.as_str())
                .collect::<Vec<_>>(),
            ["stage-1", "stage-2"]
        );
        assert_eq!(
            run.queues
                .iter()
                .map(|x| x.queue.as_str())
                .collect::<Vec<_>>(),
            ["queue-1", "queue-2"]
        );
        assert!(run.truncation.limits_hit);
        assert_eq!(
            (
                run.truncation.dropped_requests,
                run.truncation.dropped_stages,
                run.truncation.dropped_queues
            ),
            (3, 3, 3)
        );
        assert_eq!(
            run.metadata.effective_core_config.unwrap().capture_limits,
            limits()
        );
        assert!(run.inflight.is_empty() && run.runtime_snapshots.is_empty());
    }
}
// TT-TEST: support
#[test]
fn completed_span_jsonl_import_never_fabricates_runtime_or_inflight_evidence() {
    let t = import_case("precise_route_divergent", None);
    assert!(t.runtime_snapshots.is_empty());
    assert!(t.inflight.is_empty());
    assert_eq!(t.metadata.effective_tokio_sampler_config, None);
}
// TT-TEST: support
#[test]
fn completed_span_equivalence_rejects_native_partial_stage_and_queue_evidence() {
    let mut r = native_case("precise_route_divergent");
    r.stages[0].completed = false;
    assert_eq!(
        project_run(&r),
        Err(UnsupportedParityEvidence::PartialStage)
    );
    r.stages[0].completed = true;
    r.queues[0].completed = false;
    assert_eq!(
        project_run(&r),
        Err(UnsupportedParityEvidence::PartialQueue)
    );
}
// TT-TEST: support
#[test]
fn completed_span_equivalence_rejects_run_only_runtime_and_inflight_evidence() {
    let mut r = native_case("precise_route_divergent");
    r.inflight.push(InFlightSnapshot {
        at_unix_ms: 1,
        at_run_us: None,
        gauge: "x".into(),
        count: 1,
    });
    assert_eq!(project_run(&r), Err(UnsupportedParityEvidence::Inflight));
    r.inflight.clear();
    r.runtime_snapshots.push(RuntimeSnapshot {
        at_unix_ms: 1,
        at_run_us: None,
        alive_tasks: None,
        worker_count: None,
        global_queue_depth: None,
        local_queue_depth: None,
        blocking_queue_depth: None,
        remote_schedule_count: None,
    });
    assert_eq!(
        project_run(&r),
        Err(UnsupportedParityEvidence::RuntimeSnapshots)
    );
}
// TT-TEST: support
#[test]
fn run_only_metadata_differences_do_not_change_representable_projection() {
    let a = native_case("precise_route_divergent");
    let mut b = a.clone();
    b.metadata.run_id = "other".into();
    b.metadata.host = Some("host".into());
    b.metadata.pid = Some(99);
    b.metadata.finalized_at_unix_ms = Some(9);
    assert_ne!(a, b);
    assert_eq!(project_run(&a), project_run(&b));
}

// TT-TEST: support
#[test]
#[allow(clippy::too_many_lines)]
fn equivalence_projections_detect_every_contract_field_mutation() {
    let run = native_case("precise_route_divergent");
    let base = project_run(&run).unwrap();
    macro_rules! run_mut {
        ($source:expr, $expected:expr) => {{
            let mut source = run.clone();
            $source(&mut source);
            let actual = project_run(&source).unwrap();
            let mut expected = base.clone();
            $expected(&mut expected);
            assert_eq!(actual, expected);
        }};
    }
    run_mut!(
        |x: &mut tailtriage_core::Run| x.metadata.service_name = "mutated-service".into(),
        |x: &mut RepresentableRunProjection| x.service_name = "mutated-service".into()
    );
    run_mut!(
        |x: &mut tailtriage_core::Run| x.metadata.mode =
            tailtriage_core::CaptureMode::Investigation,
        |x: &mut RepresentableRunProjection| x.mode = tailtriage_core::CaptureMode::Investigation
    );
    run_mut!(
        |x: &mut tailtriage_core::Run| x.metadata.effective_core_config = None,
        |x: &mut RepresentableRunProjection| x.effective_core_config = None
    );
    run_mut!(
        |x: &mut tailtriage_core::Run| x.requests[0].request_id = "mutated-request".into(),
        |x: &mut RepresentableRunProjection| x.requests[0].request_id = "mutated-request".into()
    );
    run_mut!(
        |x: &mut tailtriage_core::Run| x.requests[0].route = "/mutated-route".into(),
        |x: &mut RepresentableRunProjection| x.requests[0].route = "/mutated-route".into()
    );
    run_mut!(
        |x: &mut tailtriage_core::Run| x.requests[0].kind = Some("mutated-kind".into()),
        |x: &mut RepresentableRunProjection| x.requests[0].kind = Some("mutated-kind".into())
    );
    run_mut!(
        |x: &mut tailtriage_core::Run| x.requests[0].outcome = "mutated-outcome".into(),
        |x: &mut RepresentableRunProjection| x.requests[0].outcome = "mutated-outcome".into()
    );
    run_mut!(
        |x: &mut tailtriage_core::Run| x.requests[0].latency_us = 111_111,
        |x: &mut RepresentableRunProjection| x.requests[0].latency_us = 111_111
    );
    run_mut!(
        |x: &mut tailtriage_core::Run| x.requests[0].started_at_run_us = Some(222_222),
        |x: &mut RepresentableRunProjection| x.requests[0].started_at_run_us = Some(222_222)
    );
    run_mut!(
        |x: &mut tailtriage_core::Run| x.requests[0].finished_at_run_us = Some(333_333),
        |x: &mut RepresentableRunProjection| x.requests[0].finished_at_run_us = Some(333_333)
    );
    run_mut!(
        |x: &mut tailtriage_core::Run| x.stages[0].request_id = "mutated-stage-request".into(),
        |x: &mut RepresentableRunProjection| x.stages[0].request_id =
            "mutated-stage-request".into()
    );
    run_mut!(
        |x: &mut tailtriage_core::Run| x.stages[0].stage = "mutated-stage".into(),
        |x: &mut RepresentableRunProjection| x.stages[0].stage = "mutated-stage".into()
    );
    run_mut!(
        |x: &mut tailtriage_core::Run| x.stages[0].success = !x.stages[0].success,
        |x: &mut RepresentableRunProjection| x.stages[0].success = !x.stages[0].success
    );
    run_mut!(
        |x: &mut tailtriage_core::Run| x.stages[0].latency_us = 111_112,
        |x: &mut RepresentableRunProjection| x.stages[0].latency_us = 111_112
    );
    run_mut!(
        |x: &mut tailtriage_core::Run| x.stages[0].started_at_run_us = Some(222_223),
        |x: &mut RepresentableRunProjection| x.stages[0].started_at_run_us = Some(222_223)
    );
    run_mut!(
        |x: &mut tailtriage_core::Run| x.stages[0].finished_at_run_us = Some(333_334),
        |x: &mut RepresentableRunProjection| x.stages[0].finished_at_run_us = Some(333_334)
    );
    let mut partial = run.clone();
    partial.stages[0].completed = false;
    assert_eq!(
        project_run(&partial),
        Err(UnsupportedParityEvidence::PartialStage)
    );
    run_mut!(
        |x: &mut tailtriage_core::Run| x.queues[0].request_id = "mutated-queue-request".into(),
        |x: &mut RepresentableRunProjection| x.queues[0].request_id =
            "mutated-queue-request".into()
    );
    run_mut!(
        |x: &mut tailtriage_core::Run| x.queues[0].queue = "mutated-queue".into(),
        |x: &mut RepresentableRunProjection| x.queues[0].queue = "mutated-queue".into()
    );
    run_mut!(
        |x: &mut tailtriage_core::Run| x.queues[0].depth_at_start = Some(77_777),
        |x: &mut RepresentableRunProjection| x.queues[0].depth_at_start = Some(77_777)
    );
    run_mut!(
        |x: &mut tailtriage_core::Run| x.queues[0].wait_us = 111_113,
        |x: &mut RepresentableRunProjection| x.queues[0].wait_us = 111_113
    );
    run_mut!(
        |x: &mut tailtriage_core::Run| x.queues[0].waited_from_run_us = Some(222_224),
        |x: &mut RepresentableRunProjection| x.queues[0].waited_from_run_us = Some(222_224)
    );
    run_mut!(
        |x: &mut tailtriage_core::Run| x.queues[0].waited_until_run_us = Some(333_335),
        |x: &mut RepresentableRunProjection| x.queues[0].waited_until_run_us = Some(333_335)
    );
    let mut partial = run.clone();
    partial.queues[0].completed = false;
    assert_eq!(
        project_run(&partial),
        Err(UnsupportedParityEvidence::PartialQueue)
    );
    run_mut!(
        |x: &mut tailtriage_core::Run| x.truncation.limits_hit = true,
        |x: &mut RepresentableRunProjection| x.semantic_truncation.limits_hit = true
    );
    run_mut!(
        |x: &mut tailtriage_core::Run| x.truncation.dropped_requests = 11,
        |x: &mut RepresentableRunProjection| x.semantic_truncation.dropped_requests = 11
    );
    run_mut!(
        |x: &mut tailtriage_core::Run| x.truncation.dropped_stages = 22,
        |x: &mut RepresentableRunProjection| x.semantic_truncation.dropped_stages = 22
    );
    run_mut!(
        |x: &mut tailtriage_core::Run| x.truncation.dropped_queues = 33,
        |x: &mut RepresentableRunProjection| x.semantic_truncation.dropped_queues = 33
    );

    let typed = typed_report(&run);
    let projected = project_report(&typed);
    macro_rules! report_field {
        ($source:expr, $expected:expr) => {{
            let mut source = typed.clone();
            $source(&mut source);
            let actual = project_report(&source);
            let mut expected = projected.clone();
            $expected(&mut expected);
            assert_eq!(actual, expected);
        }};
    }
    report_field!(
        |x: &mut tailtriage_analyzer::Report| x.request_count = 987,
        |x: &mut ComparableReportProjection| x.request_count = 987
    );
    report_field!(
        |x: &mut tailtriage_analyzer::Report| x.p50_latency_us = Some(111_111),
        |x: &mut ComparableReportProjection| x.p50_latency_us = Some(111_111)
    );
    report_field!(
        |x: &mut tailtriage_analyzer::Report| x.p95_latency_us = Some(222_222),
        |x: &mut ComparableReportProjection| x.p95_latency_us = Some(222_222)
    );
    report_field!(
        |x: &mut tailtriage_analyzer::Report| x.p99_latency_us = Some(333_333),
        |x: &mut ComparableReportProjection| x.p99_latency_us = Some(333_333)
    );
    report_field!(
        |x: &mut tailtriage_analyzer::Report| x.p95_queue_share_permille = Some(444),
        |x: &mut ComparableReportProjection| x.p95_queue_share_permille = Some(444)
    );
    report_field!(
        |x: &mut tailtriage_analyzer::Report| x.p95_service_share_permille = Some(555),
        |x: &mut ComparableReportProjection| x.p95_service_share_permille = Some(555)
    );
    report_field!(
        |x: &mut tailtriage_analyzer::Report| x.inflight_trend =
            Some(tailtriage_analyzer::InflightTrend {
                gauge: "mutated-gauge".into(),
                sample_count: 7,
                peak_count: 8,
                p95_count: 9,
                growth_delta: 10,
                growth_per_sec_milli: Some(11)
            }),
        |x: &mut ComparableReportProjection| x.inflight_trend = serde_json::json!({"gauge":"mutated-gauge","sample_count":7,"peak_count":8,"p95_count":9,"growth_delta":10,"growth_per_sec_milli":11})
    );
    report_field!(
        |x: &mut tailtriage_analyzer::Report| x.evidence_quality.limitations =
            vec!["mutated-quality".into()],
        |x: &mut ComparableReportProjection| x.evidence_quality["limitations"] =
            serde_json::json!(["mutated-quality"])
    );
    report_field!(
        |x: &mut tailtriage_analyzer::Report| x.warnings =
            vec!["warning-b".into(), "warning-a".into()],
        |x: &mut ComparableReportProjection| x.warnings =
            vec!["warning-b".into(), "warning-a".into()]
    );

    macro_rules! nested_report {
        ($field:ident, $source:expr, $pointer:literal, $value:expr) => {{
            let mut source = typed.clone();
            $source(&mut source);
            let actual = project_report(&source);
            assert_eq!(actual.$field.pointer($pointer), Some(&$value));
            let mut expected = projected.clone();
            *expected.$field.pointer_mut($pointer).unwrap() = $value;
            assert_eq!(actual, expected);
        }};
    }
    nested_report!(
        primary_suspect,
        |x: &mut tailtriage_analyzer::Report| x.primary_suspect.kind =
            x.secondary_suspects[0].kind.clone(),
        "/kind",
        serde_json::to_value(&typed.secondary_suspects[0].kind).unwrap()
    );
    nested_report!(
        primary_suspect,
        |x: &mut tailtriage_analyzer::Report| x.primary_suspect.score = 91,
        "/score",
        serde_json::json!(91)
    );
    nested_report!(
        primary_suspect,
        |x: &mut tailtriage_analyzer::Report| x.primary_suspect.confidence =
            tailtriage_analyzer::Confidence::Low,
        "/confidence",
        serde_json::json!("low")
    );
    nested_report!(
        primary_suspect,
        |x: &mut tailtriage_analyzer::Report| x.primary_suspect.confidence_notes =
            vec!["note-b".into(), "note-a".into()],
        "/confidence_notes",
        serde_json::json!(["note-b", "note-a"])
    );
    nested_report!(
        primary_suspect,
        |x: &mut tailtriage_analyzer::Report| x.primary_suspect.evidence =
            vec!["evidence-b".into(), "evidence-a".into()],
        "/evidence",
        serde_json::json!(["evidence-b", "evidence-a"])
    );
    nested_report!(
        primary_suspect,
        |x: &mut tailtriage_analyzer::Report| x.primary_suspect.next_checks =
            vec!["check-b".into(), "check-a".into()],
        "/next_checks",
        serde_json::json!(["check-b", "check-a"])
    );

    let mut secondary_source = typed.clone();
    let first = secondary_source.secondary_suspects[0].clone();
    let mut distinct = first.clone();
    distinct.score = 73;
    secondary_source.secondary_suspects = vec![distinct.clone(), first.clone(), first.clone()];
    secondary_source.secondary_suspects.reverse();
    let secondary_actual = project_report(&secondary_source);
    let expected_secondary = serde_json::to_value(&secondary_source.secondary_suspects).unwrap();
    assert_eq!(secondary_actual.secondary_suspects, expected_secondary);
    let mut secondary_expected = projected.clone();
    secondary_expected.secondary_suspects = expected_secondary;
    assert_eq!(secondary_actual, secondary_expected);

    macro_rules! breakdown_case {
        ($source_report:expr, $base_projection:expr, $field:ident, $source:expr, $pointer:literal, $value:expr) => {{
            let mut source = $source_report.clone();
            $source(&mut source);
            let actual = project_report(&source);
            assert_eq!(actual.$field.pointer($pointer), Some(&$value));
            let mut expected = $base_projection.clone();
            *expected.$field.pointer_mut($pointer).unwrap() = $value;
            assert_eq!(actual, expected);
        }};
    }
    breakdown_case!(
        typed,
        projected,
        route_breakdowns,
        |x: &mut tailtriage_analyzer::Report| x.route_breakdowns[0].route =
            "/mutated-route-breakdown".into(),
        "/0/route",
        serde_json::json!("/mutated-route-breakdown")
    );
    breakdown_case!(
        typed,
        projected,
        route_breakdowns,
        |x: &mut tailtriage_analyzer::Report| x.route_breakdowns[0].request_count = 41,
        "/0/request_count",
        serde_json::json!(41)
    );
    breakdown_case!(
        typed,
        projected,
        route_breakdowns,
        |x: &mut tailtriage_analyzer::Report| x.route_breakdowns[0].p50_latency_us = Some(111_111),
        "/0/p50_latency_us",
        serde_json::json!(111_111)
    );
    breakdown_case!(
        typed,
        projected,
        route_breakdowns,
        |x: &mut tailtriage_analyzer::Report| x.route_breakdowns[0].p95_latency_us = Some(222_222),
        "/0/p95_latency_us",
        serde_json::json!(222_222)
    );
    breakdown_case!(
        typed,
        projected,
        route_breakdowns,
        |x: &mut tailtriage_analyzer::Report| x.route_breakdowns[0].p99_latency_us = Some(333_333),
        "/0/p99_latency_us",
        serde_json::json!(333_333)
    );
    breakdown_case!(
        typed,
        projected,
        route_breakdowns,
        |x: &mut tailtriage_analyzer::Report| x.route_breakdowns[0].p95_queue_share_permille =
            Some(444),
        "/0/p95_queue_share_permille",
        serde_json::json!(444)
    );
    breakdown_case!(
        typed,
        projected,
        route_breakdowns,
        |x: &mut tailtriage_analyzer::Report| x.route_breakdowns[0].p95_service_share_permille =
            Some(555),
        "/0/p95_service_share_permille",
        serde_json::json!(555)
    );
    breakdown_case!(
        typed,
        projected,
        route_breakdowns,
        |x: &mut tailtriage_analyzer::Report| x.route_breakdowns[0].evidence_quality.limitations =
            vec!["route-quality".into()],
        "/0/evidence_quality/limitations",
        serde_json::json!(["route-quality"])
    );
    breakdown_case!(
        typed,
        projected,
        route_breakdowns,
        |x: &mut tailtriage_analyzer::Report| x.route_breakdowns[0].primary_suspect.score = 81,
        "/0/primary_suspect/score",
        serde_json::json!(81)
    );
    breakdown_case!(
        typed,
        projected,
        route_breakdowns,
        |x: &mut tailtriage_analyzer::Report| x.route_breakdowns[1].secondary_suspects =
            vec![x.route_breakdowns[1].secondary_suspects[0].clone(); 2],
        "/1/secondary_suspects",
        serde_json::to_value(vec![
            typed.route_breakdowns[1].secondary_suspects[0].clone();
            2
        ])
        .unwrap()
    );
    breakdown_case!(
        typed,
        projected,
        route_breakdowns,
        |x: &mut tailtriage_analyzer::Report| x.route_breakdowns[0].warnings =
            vec!["route-b".into(), "route-a".into()],
        "/0/warnings",
        serde_json::json!(["route-b", "route-a"])
    );
    let mut route_order = typed.clone();
    route_order.route_breakdowns.reverse();
    let route_actual = project_report(&route_order);
    let route_value = serde_json::to_value(&route_order.route_breakdowns).unwrap();
    assert_eq!(route_actual.route_breakdowns, route_value);
    let mut route_expected = projected.clone();
    route_expected.route_breakdowns = route_value;
    assert_eq!(route_actual, route_expected);

    let temporal = typed_report(&native_case("precise_temporal_movement"));
    let temporal_base = project_report(&temporal);
    breakdown_case!(
        temporal,
        temporal_base,
        temporal_segments,
        |x: &mut tailtriage_analyzer::Report| x.temporal_segments[0].name =
            "mutated-segment".into(),
        "/0/name",
        serde_json::json!("mutated-segment")
    );
    breakdown_case!(
        temporal,
        temporal_base,
        temporal_segments,
        |x: &mut tailtriage_analyzer::Report| x.temporal_segments[0].request_count = 42,
        "/0/request_count",
        serde_json::json!(42)
    );
    breakdown_case!(
        temporal,
        temporal_base,
        temporal_segments,
        |x: &mut tailtriage_analyzer::Report| x.temporal_segments[0].p50_latency_us = Some(111_111),
        "/0/p50_latency_us",
        serde_json::json!(111_111)
    );
    breakdown_case!(
        temporal,
        temporal_base,
        temporal_segments,
        |x: &mut tailtriage_analyzer::Report| x.temporal_segments[0].p95_latency_us = Some(222_222),
        "/0/p95_latency_us",
        serde_json::json!(222_222)
    );
    breakdown_case!(
        temporal,
        temporal_base,
        temporal_segments,
        |x: &mut tailtriage_analyzer::Report| x.temporal_segments[0].p99_latency_us = Some(333_333),
        "/0/p99_latency_us",
        serde_json::json!(333_333)
    );
    breakdown_case!(
        temporal,
        temporal_base,
        temporal_segments,
        |x: &mut tailtriage_analyzer::Report| x.temporal_segments[0].p95_queue_share_permille =
            Some(444),
        "/0/p95_queue_share_permille",
        serde_json::json!(444)
    );
    breakdown_case!(
        temporal,
        temporal_base,
        temporal_segments,
        |x: &mut tailtriage_analyzer::Report| x.temporal_segments[0].p95_service_share_permille =
            Some(555),
        "/0/p95_service_share_permille",
        serde_json::json!(555)
    );
    breakdown_case!(
        temporal,
        temporal_base,
        temporal_segments,
        |x: &mut tailtriage_analyzer::Report| x.temporal_segments[0].evidence_quality.limitations =
            vec!["temporal-quality".into()],
        "/0/evidence_quality/limitations",
        serde_json::json!(["temporal-quality"])
    );
    breakdown_case!(
        temporal,
        temporal_base,
        temporal_segments,
        |x: &mut tailtriage_analyzer::Report| x.temporal_segments[0].primary_suspect.score = 82,
        "/0/primary_suspect/score",
        serde_json::json!(82)
    );
    breakdown_case!(
        temporal,
        temporal_base,
        temporal_segments,
        |x: &mut tailtriage_analyzer::Report| x.temporal_segments[0].secondary_suspects =
            vec![x.temporal_segments[0].secondary_suspects[0].clone(); 2],
        "/0/secondary_suspects",
        serde_json::to_value(vec![
            temporal.temporal_segments[0].secondary_suspects[0]
                .clone();
            2
        ])
        .unwrap()
    );
    breakdown_case!(
        temporal,
        temporal_base,
        temporal_segments,
        |x: &mut tailtriage_analyzer::Report| x.temporal_segments[0].warnings =
            vec!["temporal-b".into(), "temporal-a".into()],
        "/0/warnings",
        serde_json::json!(["temporal-b", "temporal-a"])
    );
    let mut temporal_order = temporal.clone();
    temporal_order.temporal_segments.reverse();
    let temporal_order_actual = project_report(&temporal_order);
    assert_eq!(
        temporal_order_actual.temporal_segments[0]["name"],
        serde_json::json!("late")
    );
    assert_eq!(
        temporal_order_actual.temporal_segments[1]["name"],
        serde_json::json!("early")
    );
    let mut temporal_order_expected = temporal_base.clone();
    temporal_order_expected
        .temporal_segments
        .as_array_mut()
        .unwrap()
        .reverse();
    assert_eq!(temporal_order_actual, temporal_order_expected);

    let mut unix = temporal.clone();
    unix.temporal_segments[0].started_at_unix_ms = Some(1);
    unix.temporal_segments[0].finished_at_unix_ms = Some(2);
    assert_eq!(project_report(&unix), temporal_base);
    unix.temporal_segments[0].request_count = 43;
    let unix_actual = project_report(&unix);
    let mut unix_expected = temporal_base.clone();
    unix_expected.temporal_segments[0]["request_count"] = serde_json::json!(43);
    assert_eq!(unix_actual, unix_expected);
}

// TT-TEST: support
#[test]
fn native_and_tracing_projection_json_matches_checked_in_bytes() {
    for (name, limits) in [
        ("precise_route_divergent", None),
        ("precise_temporal_movement", None),
        ("duration_only_legacy", None),
        ("semantic_retention_limits", Some(limits())),
    ] {
        let (n, t) = case_pair(name, limits);
        for run in [&n, &t] {
            assert_eq!(
                format!(
                    "{}\n",
                    serde_json::to_string_pretty(&project_run(run).unwrap()).unwrap()
                ),
                expected_run_json_bytes(name)
            );
            assert_eq!(
                format!("{}\n", serde_json::to_string_pretty(&report(run)).unwrap()),
                expected_report_json_bytes(name)
            );
        }
    }
}
