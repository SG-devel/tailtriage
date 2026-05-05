use std::path::Path;

use serde_json::Value;
use tailtriage_cli::analyze::analyze_run;
use tailtriage_core::Run;

fn load_fixture(name: &str) -> Run {
    let path = Path::new("tests/fixtures").join(name);
    let content = std::fs::read_to_string(path).expect("fixture should exist");
    serde_json::from_str(&content).expect("fixture should deserialize")
}

fn json_path_exists<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    path.iter()
        .try_fold(value, |current, key| current.get(*key))
}

#[test]
fn documented_report_keys_exist_in_json_output() {
    let run = load_fixture("queue_saturation.json");
    let report = analyze_run(&run);
    let json = serde_json::to_value(&report).expect("report should serialize");

    // Keep this contract aligned with the keys called out in README's JSON-output section.
    for path in [
        ["primary_suspect", "kind"].as_slice(),
        ["p95_queue_share_permille"].as_slice(),
        ["p95_service_share_permille"].as_slice(),
        ["primary_suspect", "evidence"].as_slice(),
        ["temporal_segments"].as_slice(),
    ] {
        assert!(
            json_path_exists(&json, path).is_some(),
            "expected documented JSON path {path:?}",
        );
    }

    let evidence = json_path_exists(&json, &["primary_suspect", "evidence"])
        .and_then(Value::as_array)
        .expect("primary_suspect.evidence should be an array");
    assert!(!evidence.is_empty(), "evidence array should not be empty");
    assert!(
        evidence.iter().all(Value::is_string),
        "primary_suspect.evidence should contain strings"
    );
}
