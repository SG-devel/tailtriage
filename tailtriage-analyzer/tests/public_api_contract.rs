use std::path::Path;

use serde_json::Value;
use tailtriage_analyzer::{
    analyze_run, render_json, render_json_pretty, render_text, AnalyzeOptions, Report,
};
use tailtriage_core::{validate_run_strict, Run};

fn load_fixture(name: &str) -> Run {
    let path = Path::new("tests/fixtures").join(name);
    let content = std::fs::read_to_string(path).expect("fixture should exist");
    serde_json::from_str(&content).expect("fixture should deserialize")
}

#[test]
fn public_api_supports_checked_analysis_and_canonical_renderers() {
    let run = load_fixture("queue_saturation.json");

    validate_run_strict(&run).expect("fixture should pass explicit strict validation");
    let report: Report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    let text = render_text(&report);
    let compact = render_json(&report).expect("report should render as compact JSON");
    let pretty = render_json_pretty(&report).expect("report should render as pretty JSON");
    let json_value: Value = serde_json::from_str(&pretty).expect("json should parse");

    assert!(text.contains("Primary suspect:"));
    assert_eq!(
        serde_json::from_str::<Value>(&compact).expect("compact JSON should parse"),
        json_value
    );
    for path in [
        "/evidence_quality",
        "/primary_suspect/confidence_notes",
        "/route_breakdowns",
        "/temporal_segments",
    ] {
        assert!(
            json_value.pointer(path).is_some(),
            "expected JSON path {path}"
        );
    }
}
