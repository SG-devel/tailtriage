use std::path::Path;

use serde_json::Value;
use tailscope_cli::analyze::analyze_run;
use tailscope_core::Run;

fn load_fixture(name: &str) -> Run {
    let path = Path::new("tests/fixtures").join(name);
    let content = std::fs::read_to_string(path).expect("fixture should exist");
    serde_json::from_str(&content).expect("fixture should deserialize")
}

#[test]
fn documented_report_keys_exist_in_json_output() {
    let run = load_fixture("queue_saturation.json");
    let report = analyze_run(&run);
    let json = serde_json::to_value(&report).expect("report should serialize");

    for key in [
        "request_count",
        "p50_latency_us",
        "p95_latency_us",
        "p99_latency_us",
        "p95_queue_share_permille",
        "p95_service_share_permille",
        "inflight_trend",
        "primary_suspect",
        "secondary_suspects",
    ] {
        assert!(
            json.get(key).is_some(),
            "expected top-level documented key '{key}'"
        );
    }

    let primary_suspect = json
        .get("primary_suspect")
        .and_then(Value::as_object)
        .expect("primary_suspect should be an object");

    assert!(primary_suspect.contains_key("kind"));
    assert!(primary_suspect.contains_key("evidence"));

    let evidence = primary_suspect
        .get("evidence")
        .and_then(Value::as_array)
        .expect("primary_suspect.evidence should be an array");
    assert!(
        evidence.iter().all(Value::is_string),
        "primary_suspect.evidence should contain strings"
    );
}
