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
#[test]
fn precise_native_and_tracing_runs_match_independent_representable_projection() {
    assert_both("precise_route_divergent")
}
#[test]
fn precise_native_and_tracing_reports_match_independent_expected_projection() {
    assert_reports("precise_route_divergent")
}
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
#[test]
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
    assert_eq!(
        std::fs::read_to_string(fixture_path("semantic_retention_limits"))
            .unwrap()
            .lines()
            .count(),
        15
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
#[test]
fn completed_span_jsonl_import_never_fabricates_runtime_or_inflight_evidence() {
    let t = import_case("precise_route_divergent", None);
    assert!(t.runtime_snapshots.is_empty());
    assert!(t.inflight.is_empty());
    assert_eq!(t.metadata.effective_tokio_sampler_config, None);
}
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

#[test]
#[allow(clippy::too_many_lines)]
fn equivalence_projections_detect_every_contract_field_mutation() {
    let run = native_case("precise_route_divergent");
    let base = project_run(&run).unwrap();
    macro_rules! run_mut {
        ($label:literal,$change:expr) => {{
            let mut x = run.clone();
            $change(&mut x);
            let changed = project_run(&x).unwrap();
            assert_ne!(changed, base, $label);
        }};
    }
    run_mut!("service_name", |x: &mut tailtriage_core::Run| x
        .metadata
        .service_name
        .push('x'));
    run_mut!("mode", |x: &mut tailtriage_core::Run| x.metadata.mode =
        tailtriage_core::CaptureMode::Investigation);
    run_mut!("config", |x: &mut tailtriage_core::Run| x
        .metadata
        .effective_core_config =
        None);
    run_mut!("request_id", |x: &mut tailtriage_core::Run| x.requests[0]
        .request_id
        .push('x'));
    run_mut!("route", |x: &mut tailtriage_core::Run| x.requests[0]
        .route
        .push('x'));
    run_mut!("kind", |x: &mut tailtriage_core::Run| x.requests[0].kind =
        Some("x".into()));
    run_mut!("outcome", |x: &mut tailtriage_core::Run| x.requests[0]
        .outcome
        .push('x'));
    run_mut!("request latency", |x: &mut tailtriage_core::Run| x
        .requests[0]
        .latency_us +=
        1);
    run_mut!("request start", |x: &mut tailtriage_core::Run| x.requests
        [0]
    .started_at_run_us =
        Some(9));
    run_mut!("request finish", |x: &mut tailtriage_core::Run| x
        .requests[0]
        .finished_at_run_us =
        Some(9));
    run_mut!("stage request", |x: &mut tailtriage_core::Run| x.stages[0]
        .request_id
        .push('x'));
    run_mut!("stage", |x: &mut tailtriage_core::Run| x.stages[0]
        .stage
        .push('x'));
    run_mut!("success", |x: &mut tailtriage_core::Run| x.stages[0]
        .success =
        !x.stages[0].success);
    run_mut!("stage latency", |x: &mut tailtriage_core::Run| x.stages
        [0]
    .latency_us +=
        1);
    run_mut!("stage start", |x: &mut tailtriage_core::Run| x.stages[0]
        .started_at_run_us =
        Some(9));
    run_mut!("stage finish", |x: &mut tailtriage_core::Run| x.stages[0]
        .finished_at_run_us =
        Some(9));
    let mut partial = run.clone();
    partial.stages[0].completed = false;
    assert_eq!(
        project_run(&partial),
        Err(UnsupportedParityEvidence::PartialStage)
    );
    run_mut!("queue request", |x: &mut tailtriage_core::Run| x.queues[0]
        .request_id
        .push('x'));
    run_mut!("queue", |x: &mut tailtriage_core::Run| x.queues[0]
        .queue
        .push('x'));
    run_mut!("depth", |x: &mut tailtriage_core::Run| x.queues[0]
        .depth_at_start =
        Some(99));
    run_mut!("wait", |x: &mut tailtriage_core::Run| x.queues[0]
        .wait_us += 1);
    run_mut!("queue start", |x: &mut tailtriage_core::Run| x.queues[0]
        .waited_from_run_us =
        Some(9));
    run_mut!("queue finish", |x: &mut tailtriage_core::Run| x.queues[0]
        .waited_until_run_us =
        Some(9));
    let mut partial = run.clone();
    partial.queues[0].completed = false;
    assert_eq!(
        project_run(&partial),
        Err(UnsupportedParityEvidence::PartialQueue)
    );
    run_mut!("limits", |x: &mut tailtriage_core::Run| x
        .truncation
        .limits_hit =
        true);
    run_mut!("dropped requests", |x: &mut tailtriage_core::Run| x
        .truncation
        .dropped_requests +=
        1);
    run_mut!("dropped stages", |x: &mut tailtriage_core::Run| x
        .truncation
        .dropped_stages +=
        1);
    run_mut!("dropped queues", |x: &mut tailtriage_core::Run| x
        .truncation
        .dropped_queues +=
        1);

    let typed = typed_report(&run);
    let projected = project_report(&typed);
    macro_rules! report_mut {
        ($label:literal,$change:expr) => {{
            let mut x = typed.clone();
            $change(&mut x);
            assert_ne!(project_report(&x), projected, $label);
        }};
    }
    report_mut!("count", |x: &mut tailtriage_analyzer::Report| x
        .request_count +=
        1);
    report_mut!("p50", |x: &mut tailtriage_analyzer::Report| x
        .p50_latency_us =
        Some(1));
    report_mut!("p95", |x: &mut tailtriage_analyzer::Report| x
        .p95_latency_us =
        Some(1));
    report_mut!("p99", |x: &mut tailtriage_analyzer::Report| x
        .p99_latency_us =
        Some(1));
    report_mut!("queue share", |x: &mut tailtriage_analyzer::Report| x
        .p95_queue_share_permille =
        Some(1));
    report_mut!("service share", |x: &mut tailtriage_analyzer::Report| x
        .p95_service_share_permille =
        Some(1));
    report_mut!("trend", |x: &mut tailtriage_analyzer::Report| x
        .inflight_trend =
        Some(tailtriage_analyzer::InflightTrend {
            gauge: "x".into(),
            sample_count: 2,
            peak_count: 2,
            p95_count: 2,
            growth_delta: 1,
            growth_per_sec_milli: Some(1)
        }));
    report_mut!("quality", |x: &mut tailtriage_analyzer::Report| x
        .evidence_quality
        .limitations
        .push("x".into()));
    report_mut!("primary kind", |x: &mut tailtriage_analyzer::Report| x
        .primary_suspect
        .kind =
        x.secondary_suspects[0].kind.clone());
    report_mut!("primary score", |x: &mut tailtriage_analyzer::Report| x
        .primary_suspect
        .score +=
        1);
    report_mut!("confidence", |x: &mut tailtriage_analyzer::Report| x
        .primary_suspect
        .confidence =
        tailtriage_analyzer::Confidence::Low);
    report_mut!("notes order", |x: &mut tailtriage_analyzer::Report| x
        .primary_suspect
        .confidence_notes
        .reverse());
    report_mut!("evidence order", |x: &mut tailtriage_analyzer::Report| x
        .primary_suspect
        .evidence
        .reverse());
    report_mut!("checks order", |x: &mut tailtriage_analyzer::Report| x
        .primary_suspect
        .next_checks
        .reverse());
    report_mut!(
        "secondary content",
        |x: &mut tailtriage_analyzer::Report| x.secondary_suspects[0].score += 1
    );
    report_mut!("secondary order", |x: &mut tailtriage_analyzer::Report| {
        let mut other = x.secondary_suspects[0].clone();
        other.score += 1;
        x.secondary_suspects.push(other);
        x.secondary_suspects.reverse();
    });
    report_mut!("warning", |x: &mut tailtriage_analyzer::Report| x.warnings
        [0]
    .push('x'));
    report_mut!("warning order", |x: &mut tailtriage_analyzer::Report| x
        .warnings
        .reverse());
    report_mut!("route name", |x: &mut tailtriage_analyzer::Report| x
        .route_breakdowns[0]
        .route
        .push('x'));
    report_mut!("route order", |x: &mut tailtriage_analyzer::Report| x
        .route_breakdowns
        .reverse());
    report_mut!("route count", |x: &mut tailtriage_analyzer::Report| x
        .route_breakdowns[0]
        .request_count +=
        1);
    report_mut!("route percentile", |x: &mut tailtriage_analyzer::Report| {
        x.route_breakdowns[0].p95_latency_us = Some(1)
    });
    report_mut!("route shares", |x: &mut tailtriage_analyzer::Report| x
        .route_breakdowns[0]
        .p95_queue_share_permille =
        Some(1));
    report_mut!("route quality", |x: &mut tailtriage_analyzer::Report| x
        .route_breakdowns[0]
        .evidence_quality
        .limitations
        .push("x".into()));
    report_mut!("route primary", |x: &mut tailtriage_analyzer::Report| x
        .route_breakdowns[0]
        .primary_suspect
        .score +=
        1);
    report_mut!("route secondary", |x: &mut tailtriage_analyzer::Report| {
        x.route_breakdowns[1].secondary_suspects[0].score += 1
    });
    report_mut!("route warnings", |x: &mut tailtriage_analyzer::Report| x
        .route_breakdowns[0]
        .warnings
        .reverse());
    let temporal = typed_report(&native_case("precise_temporal_movement"));
    let tp = project_report(&temporal);
    macro_rules! temporal_mut {
        ($label:literal,$change:expr) => {{
            let mut x = temporal.clone();
            $change(&mut x);
            assert_ne!(project_report(&x), tp, $label);
        }};
    }
    temporal_mut!("segment name", |x: &mut tailtriage_analyzer::Report| x
        .temporal_segments[0]
        .name
        .push('x'));
    temporal_mut!("segment order", |x: &mut tailtriage_analyzer::Report| x
        .temporal_segments
        .reverse());
    temporal_mut!("segment count", |x: &mut tailtriage_analyzer::Report| x
        .temporal_segments[0]
        .request_count +=
        1);
    temporal_mut!(
        "segment percentile",
        |x: &mut tailtriage_analyzer::Report| x.temporal_segments[0].p95_latency_us = Some(1)
    );
    temporal_mut!("segment shares", |x: &mut tailtriage_analyzer::Report| x
        .temporal_segments[0]
        .p95_queue_share_permille =
        Some(1));
    temporal_mut!("segment quality", |x: &mut tailtriage_analyzer::Report| x
        .temporal_segments[0]
        .evidence_quality
        .limitations
        .push("x".into()));
    temporal_mut!("segment primary", |x: &mut tailtriage_analyzer::Report| {
        x.temporal_segments[0].primary_suspect.score += 1
    });
    temporal_mut!(
        "segment secondary",
        |x: &mut tailtriage_analyzer::Report| {
            let suspect = x.temporal_segments[0].primary_suspect.clone();
            x.temporal_segments[0].secondary_suspects.push(suspect);
        }
    );
    temporal_mut!("segment warnings", |x: &mut tailtriage_analyzer::Report| x
        .temporal_segments[0]
        .warnings
        .reverse());
    let mut unix = temporal.clone();
    unix.temporal_segments[0].started_at_unix_ms = Some(1);
    unix.temporal_segments[0].finished_at_unix_ms = Some(2);
    assert_eq!(project_report(&unix), tp);
    unix.temporal_segments[0].request_count += 1;
    assert_ne!(project_report(&unix), tp);
}
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
