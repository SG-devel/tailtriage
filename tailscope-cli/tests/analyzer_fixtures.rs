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
    assert!(text.contains("secondary suspects") || report.secondary_suspects.is_empty());

    let json = serde_json::to_string_pretty(&report).expect("json rendering should work");
    assert!(json.contains("primary_suspect"));
    assert!(json.contains("confidence"));
}
