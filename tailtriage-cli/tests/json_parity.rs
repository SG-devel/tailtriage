use std::process::Command;

use tailtriage_analyzer::{analyze_run, render_json_pretty, AnalyzeOptions};
use tailtriage_core::Run;

#[test]
fn analyze_json_matches_analyzer_renderer_output() {
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

    let cli_stdout = String::from_utf8(output.stdout).expect("stdout should be utf8");

    let run: Run =
        serde_json::from_str(valid_cli_artifact_with_requests()).expect("fixture should decode");
    let report = analyze_run(&run, AnalyzeOptions::default());
    let expected = render_json_pretty(&report).expect("renderer should succeed");

    assert_eq!(cli_stdout.trim_end(), expected);
}

fn valid_cli_artifact_with_requests() -> &'static str {
    r#"{"schema_version":1,"metadata":{"run_id":"r1","service_name":"svc","service_version":null,"started_at_unix_ms":1,"finished_at_unix_ms":2,"mode":"light","host":null,"pid":null,"lifecycle_warnings":[],"unfinished_requests":{"count":0,"sample":[]}},"requests":[{"request_id":"req1","route":"/","kind":null,"started_at_unix_ms":1,"finished_at_unix_ms":2,"latency_us":10,"outcome":"ok"}],"stages":[],"queues":[],"inflight":[],"runtime_snapshots":[]}"#
}
