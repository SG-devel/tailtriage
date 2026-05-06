use std::process::Command;

#[test]
fn analyze_json_output_matches_analyzer_schema() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let run_path = dir.path().join("run.json");
    std::fs::write(&run_path, sample_run_json()).expect("fixture should write");

    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("analyze")
        .arg(&run_path)
        .arg("--format")
        .arg("json")
        .output()
        .expect("cli should run");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let report: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be valid JSON report");

    assert_eq!(
        report["request_count"].as_u64(),
        Some(1),
        "unexpected report JSON: {report}"
    );
    assert!(report.get("primary_suspect").is_some());
}

fn sample_run_json() -> &'static str {
    r#"{"schema_version":1,"metadata":{"run_id":"r1","service_name":"svc","service_version":null,"started_at_unix_ms":1,"finished_at_unix_ms":2,"mode":"light","host":null,"pid":null,"lifecycle_warnings":[],"unfinished_requests":{"count":0,"sample":[]}},"requests":[{"request_id":"req1","route":"/","kind":null,"started_at_unix_ms":1,"finished_at_unix_ms":2,"latency_us":10,"outcome":"ok"}],"stages":[],"queues":[],"inflight":[],"runtime_snapshots":[]}"#
}
