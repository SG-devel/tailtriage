use std::path::Path;

use tailtriage_analyzer::{analyze_run, render_text, AnalyzeOptions};
use tailtriage_core::Run;

fn load_fixture(name: &str) -> Run {
    let path = Path::new("tests/fixtures").join(name);
    let content = std::fs::read_to_string(path).expect("fixture should exist");
    serde_json::from_str(&content).expect("fixture should deserialize")
}

#[test]
fn public_api_supports_analyze_render_and_json_contract_keys() {
    let run = load_fixture("queue_saturation.json");

    let report = analyze_run(&run, AnalyzeOptions::default());
    let text = render_text(&report);
    let json = serde_json::to_string_pretty(&report).expect("report should serialize to json");

    assert!(text.contains("Primary suspect:"));
    assert!(!text.trim().is_empty());

    for key in [
        "evidence_quality",
        "confidence_notes",
        "route_breakdowns",
        "temporal_segments",
    ] {
        assert!(json.contains(key), "expected key in json: {key}");
    }
}
