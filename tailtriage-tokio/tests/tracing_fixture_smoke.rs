use std::collections::BTreeSet;

#[test]
fn tracing_fixture_has_request_stage_and_queue_records() {
    let data = include_str!("../examples/tracing_spans.jsonl");
    let mut record_types = BTreeSet::new();

    for line in data.lines().filter(|line| !line.trim().is_empty()) {
        let value: serde_json::Value = serde_json::from_str(line).expect("valid jsonl line");
        let record_type = value
            .get("record_type")
            .and_then(serde_json::Value::as_str)
            .expect("record_type string");
        record_types.insert(record_type.to_string());
    }

    assert!(record_types.contains("request"));
    assert!(record_types.contains("stage"));
    assert!(record_types.contains("queue"));
}
