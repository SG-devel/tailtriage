use std::path::Path;

use serde_json::Value;
use tailtriage_analyzer::{analyze_run, render_text, AnalyzeOptions};
use tailtriage_core::Run;

fn load_fixture(name: &str) -> Run {
    let path = Path::new("tests/fixtures").join(name);
    let content = std::fs::read_to_string(path).expect("fixture should exist");
    serde_json::from_str(&content).expect("fixture should deserialize")
}

#[test]
fn public_api_supports_typed_analysis_text_render_and_json_serialization() {
    let run = load_fixture("queue_saturation.json");

    let report = analyze_run(&run, AnalyzeOptions::default());
    let text = render_text(&report);
    let json = serde_json::to_string_pretty(&report).expect("report should serialize to JSON");
    let json_value: Value = serde_json::from_str(&json).expect("json should parse");

    assert!(text.contains("Primary suspect:"));
    assert!(json_value.get("evidence_quality").is_some());
    assert!(json_value["primary_suspect"]
        .get("confidence_notes")
        .is_some());
    assert!(json_value.get("route_breakdowns").is_some());
    assert!(json_value.get("temporal_segments").is_some());
}
