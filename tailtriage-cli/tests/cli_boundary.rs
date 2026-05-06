use std::process::Command;

use tailtriage_analyzer::{analyze_run, AnalyzeOptions};
use tailtriage_core::Run;

#[test]
fn cli_json_output_is_valid_report_json() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let artifact_path = dir.path().join("run.json");

    std::fs::write(&artifact_path, valid_cli_artifact_with_requests())
        .expect("fixture should write");

    let exe = env!("CARGO_BIN_EXE_tailtriage");
    let output = Command::new(exe)
        .arg("analyze")
        .arg(&artifact_path)
        .arg("--format")
        .arg("json")
        .output()
        .expect("cli should run");

    assert!(output.status.success(), "cli failed: {output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).trim().is_empty());

    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf8");
    let report: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout should be valid json");
    assert_eq!(report["request_count"].as_u64(), Some(1));
    assert!(report.get("primary_suspect").is_some());
}

#[test]
fn cli_loader_rejects_empty_requests_but_analyzer_accepts_zero_request_run() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let artifact_path = dir.path().join("empty-requests.json");

    std::fs::write(&artifact_path, valid_cli_artifact_with_empty_requests())
        .expect("fixture should write");

    let err = tailtriage_cli::artifact::load_run_artifact(&artifact_path)
        .expect_err("cli loader should reject empty requests artifacts");
    assert!(err.to_string().contains("requests section is empty"));

    let run: Run = serde_json::from_str(valid_cli_artifact_with_empty_requests())
        .expect("fixture should decode to run");

    let report = analyze_run(&run, AnalyzeOptions::default());
    assert_eq!(report.request_count, 0);
}

fn valid_cli_artifact_with_requests() -> &'static str {
    r#"{"schema_version":1,"metadata":{"run_id":"r1","service_name":"svc","service_version":null,"started_at_unix_ms":1,"finished_at_unix_ms":2,"mode":"light","host":null,"pid":null,"lifecycle_warnings":[],"unfinished_requests":{"count":0,"sample":[]}},"requests":[{"request_id":"req1","route":"/","kind":null,"started_at_unix_ms":1,"finished_at_unix_ms":2,"latency_us":10,"outcome":"ok"}],"stages":[],"queues":[],"inflight":[],"runtime_snapshots":[]}"#
}

fn valid_cli_artifact_with_empty_requests() -> &'static str {
    r#"{"schema_version":1,"metadata":{"run_id":"r1","service_name":"svc","service_version":null,"started_at_unix_ms":1,"finished_at_unix_ms":2,"mode":"light","host":null,"pid":null,"lifecycle_warnings":[],"unfinished_requests":{"count":0,"sample":[]}},"requests":[],"stages":[],"queues":[],"inflight":[],"runtime_snapshots":[]}"#
}
