use std::fs;

use serde::Deserialize;

#[derive(Deserialize)]
struct SpanRecord {
    kind: String,
}

#[test]
fn tracing_spans_fixture_covers_request_stage_and_queue() {
    let fixture_path = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/tracing_spans.jsonl");
    let content = fs::read_to_string(fixture_path).expect("fixture should exist");

    let mut has_request = false;
    let mut has_stage = false;
    let mut has_queue = false;

    for line in content.lines() {
        let parsed: SpanRecord =
            serde_json::from_str(line).expect("each line should be valid json");
        match parsed.kind.as_str() {
            "request" => has_request = true,
            "stage" => has_stage = true,
            "queue" => has_queue = true,
            _ => {}
        }
    }

    assert!(has_request, "fixture should contain a request record");
    assert!(has_stage, "fixture should contain a stage record");
    assert!(has_queue, "fixture should contain a queue record");
}
