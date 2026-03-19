use std::path::Path;

use tailscope_cli::analyze::{analyze_run, render_text, DiagnosisKind};
use tailscope_core::Run;

fn load_fixture(name: &str) -> Run {
    let path = Path::new("tests/fixtures").join(name);
    let content = std::fs::read_to_string(path).expect("fixture should exist");
    serde_json::from_str(&content).expect("fixture should deserialize")
}

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
        let report = analyze_run(&run);
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

#[test]
fn fixture_reports_render_to_text_and_json() {
    let run = load_fixture("queue_saturation.json");
    let report = analyze_run(&run);

    let text = render_text(&report);
    assert!(text.contains("primary:"));
    assert!(text.contains("request_time_share_permille"));
    assert!(text.contains("secondary suspects") || report.secondary_suspects.is_empty());

    let json = serde_json::to_string_pretty(&report).expect("json rendering should work");
    assert!(json.contains("primary_suspect"));
    assert!(json.contains("confidence"));
    assert!(json.contains("p95_queue_share_permille"));
    assert!(json.contains("p95_service_share_permille"));
}

#[test]
fn fixture_reports_include_expected_request_time_shares() {
    let queue_run = load_fixture("queue_saturation.json");
    let queue_report = analyze_run(&queue_run);
    assert_eq!(queue_report.p95_queue_share_permille, Some(666));
    assert_eq!(queue_report.p95_service_share_permille, Some(500));

    let downstream_run = load_fixture("downstream_stage.json");
    let downstream_report = analyze_run(&downstream_run);
    assert_eq!(downstream_report.p95_queue_share_permille, Some(0));
    assert_eq!(downstream_report.p95_service_share_permille, Some(1000));
}
