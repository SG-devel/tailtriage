#![allow(clippy::semicolon_if_nothing_returned)]
#[path = "support/equivalence_harness.rs"]
mod equivalence_harness;
use equivalence_harness::*;
use tailtriage_core::{inspect_run, CaptureLimits, InFlightSnapshot, RuntimeSnapshot};

fn pair(name: &str) -> (tailtriage_core::Run, tailtriage_core::Run) {
    (native_case(name), import_case(name, None))
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
    let (n, t) = pair("precise_route_divergent");
    assert_eq!(report(&n).route_breakdowns, report(&t).route_breakdowns);
    assert!(!report(&n).route_breakdowns.as_array().unwrap().is_empty())
}
#[test]
fn temporal_segments_match_for_equivalent_native_and_tracing_evidence() {
    assert_both("precise_temporal_movement");
    let (n, t) = pair("precise_temporal_movement");
    assert_eq!(report(&n).temporal_segments, report(&t).temporal_segments);
    assert_eq!(
        report(&n)
            .temporal_segments
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["early", "late"]
    )
}
#[test]
fn duration_only_native_and_tracing_cases_share_core_warning_and_report_semantics() {
    assert_both("duration_only_legacy");
    let (n, t) = pair("duration_only_legacy");
    let ni = inspect_run(&n)
        .issues
        .into_iter()
        .map(|x| (x.code, x.message))
        .collect::<Vec<_>>();
    let ti = inspect_run(&t)
        .issues
        .into_iter()
        .map(|x| (x.code, x.message))
        .collect::<Vec<_>>();
    assert_eq!(ni, ti);
    assert_eq!(report(&n), report(&t));
    assert!(!ni.is_empty())
}
#[test]
fn semantic_limits_retain_the_same_evidence_and_drop_counts() {
    let l = CaptureLimits {
        max_requests: 2,
        max_stages: 2,
        max_queues: 2,
        max_inflight_snapshots: 200_000,
        max_runtime_snapshots: 100_000,
    };
    let n = native_case("semantic_retention_limits");
    let t = import_case("semantic_retention_limits", Some(l));
    let e = expected_run("semantic_retention_limits");
    assert_eq!(project_run(&n).unwrap(), e);
    assert_eq!(project_run(&t).unwrap(), e);
    assert_eq!(report(&n), report(&t));
}
#[test]
fn completed_span_jsonl_import_never_fabricates_runtime_or_inflight_evidence() {
    let t = import_case("precise_route_divergent", None);
    assert!(t.runtime_snapshots.is_empty());
    assert!(t.inflight.is_empty());
    assert_eq!(t.metadata.effective_tokio_sampler_config, None)
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
    )
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
    )
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
    assert_eq!(project_run(&a), project_run(&b))
}
#[test]
fn equivalence_projections_detect_every_contract_field_mutation() {
    let base = expected_run("precise_route_divergent");
    let mut mutations: Vec<RepresentableRunProjection> = vec![];
    macro_rules! m {
        ($x:expr) => {{
            let mut v = base.clone();
            $x(&mut v);
            mutations.push(v)
        }};
    }
    m!(|v: &mut RepresentableRunProjection| v.requests[0].route.push('x'));
    m!(|v: &mut RepresentableRunProjection| v.requests[0].kind = Some("x".into()));
    m!(|v: &mut RepresentableRunProjection| v.requests[0].outcome.push('x'));
    m!(|v: &mut RepresentableRunProjection| v.requests[0].latency_us += 1);
    m!(|v: &mut RepresentableRunProjection| v.requests[0].started_at_run_us = Some(9));
    m!(|v: &mut RepresentableRunProjection| v.stages[0].stage.push('x'));
    m!(|v: &mut RepresentableRunProjection| v.stages[0].success = !v.stages[0].success);
    m!(|v: &mut RepresentableRunProjection| v.stages[0].completed = false);
    m!(|v: &mut RepresentableRunProjection| v.stages[0].latency_us += 1);
    m!(|v: &mut RepresentableRunProjection| v.queues[0].queue.push('x'));
    m!(|v: &mut RepresentableRunProjection| v.queues[0].depth_at_start = Some(99));
    m!(|v: &mut RepresentableRunProjection| v.queues[0].completed = false);
    m!(|v: &mut RepresentableRunProjection| v.queues[0].wait_us += 1);
    m!(|v: &mut RepresentableRunProjection| v.semantic_truncation.dropped_requests += 1);
    for v in mutations {
        assert_ne!(v, base)
    }
    let r = report(&native_case("precise_route_divergent"));
    for pointer in [
        "/primary_suspect/score",
        "/primary_suspect/confidence",
        "/primary_suspect/evidence",
        "/primary_suspect/next_checks",
        "/secondary_suspects",
        "/evidence_quality",
        "/warnings",
        "/route_breakdowns",
        "/temporal_segments",
    ] {
        let mut v = serde_json::to_value(&r).unwrap();
        *v.pointer_mut(pointer).unwrap() = serde_json::json!("mutation");
        assert_ne!(v, serde_json::to_value(&r).unwrap())
    }
}
#[test]
fn native_and_tracing_comparable_report_json_is_byte_stable() {
    let (n, t) = pair("precise_route_divergent");
    let expected =
        serde_json::to_string_pretty(&expected_report("precise_route_divergent")).unwrap();
    assert_eq!(serde_json::to_string_pretty(&report(&n)).unwrap(), expected);
    assert_eq!(serde_json::to_string_pretty(&report(&t)).unwrap(), expected)
}
