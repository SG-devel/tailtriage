use std::path::Path;

use tailtriage_analyzer::{
    analyze_run, render_json_pretty, render_text, AnalyzeOptions, DiagnosisKind,
};
use tailtriage_core::Run;

fn load_fixture(name: &str) -> Run {
    let path = Path::new("tests/fixtures").join(name);
    let content = std::fs::read_to_string(path).expect("fixture should exist");
    serde_json::from_str(&content).expect("fixture should deserialize")
}

// TT-TEST: F01 primary
#[test]
fn fixture_reports_match_canonical_pretty_json_golden_files() {
    for fixture in [
        "queue_saturation.json",
        "blocking_pressure.json",
        "executor_pressure.json",
        "downstream_stage.json",
        "insufficient_evidence.json",
        "mixed_queue_vs_blocking.json",
        "mixed_blocking_vs_downstream.json",
        "scoped_route.json",
        "scoped_temporal.json",
    ] {
        let run = load_fixture(fixture);
        let report =
            analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
        let actual = render_json_pretty(&report).expect("report should render");
        let stem = fixture.strip_suffix(".json").expect("fixture suffix");
        let expected_path = Path::new("tests/expected").join(format!("{stem}.report.json"));
        let expected = std::fs::read_to_string(&expected_path).expect("golden report should exist");
        assert_eq!(actual, expected, "fixture={fixture}");
    }
}

// TT-TEST: support
#[test]
fn scoped_route_fixture_preserves_breakdown_contract() {
    let report = analyze_run(
        &load_fixture("scoped_route.json"),
        AnalyzeOptions::default(),
    )
    .expect("analyzer options should be valid");
    assert_eq!(
        report
            .route_breakdowns
            .iter()
            .map(|breakdown| breakdown.route.as_str())
            .collect::<Vec<_>>(),
        ["/queue", "/downstream"]
    );
    assert_eq!(
        report
            .route_breakdowns
            .iter()
            .map(|breakdown| breakdown.request_count)
            .collect::<Vec<_>>(),
        [3, 3]
    );
    assert_eq!(
        report
            .route_breakdowns
            .iter()
            .map(|breakdown| breakdown.primary_suspect.kind.as_str())
            .collect::<Vec<_>>(),
        ["application_queue_saturation", "downstream_stage_dominates"]
    );
    assert!(report.route_breakdowns.iter().all(|breakdown| breakdown
        .warnings
        .iter()
        .any(|warning| warning
            == "Runtime and in-flight signals are global and are not attributed to this route.")));
}

// TT-TEST: support
#[test]
fn scoped_temporal_fixture_preserves_windowed_evidence_contract() {
    let report = analyze_run(
        &load_fixture("scoped_temporal.json"),
        AnalyzeOptions::default(),
    )
    .expect("analyzer options should be valid");
    assert_eq!(
        report
            .temporal_segments
            .iter()
            .map(|segment| segment.name.as_str())
            .collect::<Vec<_>>(),
        ["early", "late"]
    );
    assert_eq!(
        report
            .temporal_segments
            .iter()
            .map(|segment| segment.request_count)
            .collect::<Vec<_>>(),
        [10, 10]
    );
    assert_eq!(
        report
            .temporal_segments
            .iter()
            .map(|segment| segment.p95_latency_us)
            .collect::<Vec<_>>(),
        [Some(1_000), Some(6_000)]
    );
    for segment in &report.temporal_segments {
        assert_eq!(segment.evidence_quality.runtime_snapshot_count, 1);
        assert_eq!(segment.evidence_quality.inflight_snapshot_count, 1);
        assert!(segment.warnings.iter().all(
            |warning| !warning.contains("Temporal segment used wall-clock timestamp fallback")
        ));
    }
}

// TT-TEST: support
#[test]
fn fixture_categories_produce_expected_primary_suspect() {
    let cases = [
        (
            "queue_saturation.json",
            DiagnosisKind::ApplicationQueueSaturation,
        ),
        (
            "blocking_pressure.json",
            DiagnosisKind::BlockingPoolPressure,
        ),
        (
            "executor_pressure.json",
            DiagnosisKind::ExecutorPressureSuspected,
        ),
        (
            "downstream_stage.json",
            DiagnosisKind::DownstreamStageDominates,
        ),
        (
            "insufficient_evidence.json",
            DiagnosisKind::InsufficientEvidence,
        ),
    ];

    for (fixture, expected) in cases {
        let run = load_fixture(fixture);
        let report =
            analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
        assert_eq!(report.primary_suspect.kind, expected, "fixture={fixture}");
        assert!(
            !report.primary_suspect.evidence.is_empty(),
            "fixture={fixture} should include evidence"
        );
        assert!(
            !report.primary_suspect.next_checks.is_empty(),
            "fixture={fixture} should include next checks"
        );
    }
}

// TT-TEST: support
#[test]
fn fixture_reports_render_to_text_and_json() {
    let run = load_fixture("queue_saturation.json");
    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");

    let text = render_text(&report);
    assert!(text.contains("Primary suspect:"));
    assert!(text.contains("Request time at p95:"));
    assert!(text.contains("queue 66.6%"));
    assert!(text.contains("non-queue service 50.0%"));
    assert!(text.contains("Secondary suspects:") || report.secondary_suspects.is_empty());

    let json = serde_json::to_string_pretty(&report).expect("json rendering should work");
    assert!(json.contains("primary_suspect"));
    assert!(json.contains("confidence"));
    assert!(json.contains("p95_queue_share_permille"));
    assert!(json.contains("p95_service_share_permille"));
}

// TT-TEST: support
#[test]
fn fixture_reports_include_expected_request_time_shares() {
    let queue_run = load_fixture("queue_saturation.json");
    let queue_report = analyze_run(&queue_run, AnalyzeOptions::default())
        .expect("analyzer options should be valid");
    assert_eq!(queue_report.p95_queue_share_permille, Some(666));
    assert_eq!(queue_report.p95_service_share_permille, Some(500));

    let downstream_run = load_fixture("downstream_stage.json");
    let downstream_report = analyze_run(&downstream_run, AnalyzeOptions::default())
        .expect("analyzer options should be valid");
    assert_eq!(downstream_report.p95_queue_share_permille, Some(0));
    assert_eq!(downstream_report.p95_service_share_permille, Some(1000));
}

// TT-TEST: support
#[test]
fn queue_fixture_includes_inflight_trend_evidence() {
    let run = load_fixture("queue_saturation.json");
    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");

    assert!(report.inflight_trend.is_some());
    assert!(
        report
            .primary_suspect
            .evidence
            .iter()
            .any(|item| item.contains("In-flight gauge")),
        "queue saturation fixture should include in-flight evidence"
    );
}
