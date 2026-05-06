use std::path::Path;

use tailtriage_analyzer::{analyze_run, render_text, AnalyzeOptions};
use tailtriage_core::Run;

fn load_fixture(name: &str) -> Run {
    let path = Path::new("tests/fixtures").join(name);
    let content = std::fs::read_to_string(path).expect("fixture should exist");
    serde_json::from_str(&content).expect("fixture should deserialize")
}

#[test]
fn public_api_supports_typed_analyze_render_and_json() {
    let run = load_fixture("queue_saturation.json");

    let report = analyze_run(&run, AnalyzeOptions::default());
    let text = render_text(&report);
    let json = serde_json::to_string_pretty(&report).expect("report should serialize to json");

    assert!(text.contains("Primary suspect:"));
    assert!(json.contains("\"evidence_quality\""));
    assert!(json.contains("\"confidence_notes\""));
    assert!(json.contains("\"route_breakdowns\""));
    assert!(json.contains("\"temporal_segments\""));
}
