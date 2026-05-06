use std::process::Command;

use tailtriage_analyzer::{analyze_run, AnalyzeOptions};
use tailtriage_cli::artifact::load_run_artifact;
use tailtriage_core::Run;

fn valid_run_json_with_requests(requests_json: &str) -> String {
    format!(
        "{{\"schema_version\":1,\"metadata\":{{\"run_id\":\"r1\",\"service_name\":\"svc\",\"service_version\":null,\"started_at_unix_ms\":1,\"finished_at_unix_ms\":2,\"mode\":\"light\",\"host\":null,\"pid\":null,\"lifecycle_warnings\":[],\"unfinished_requests\":{{\"count\":0,\"sample\":[]}}}},\"requests\":{requests_json},\"stages\":[],\"queues\":[],\"inflight\":[],\"runtime_snapshots\":[]}}"
    )
}

#[test]
fn cli_json_output_is_valid_report_json() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let run_path = dir.path().join("run.json");
    std::fs::write(
        &run_path,
        valid_run_json_with_requests(
            r#"[{"request_id":"req1","route":"/","kind":null,"started_at_unix_ms":1,"finished_at_unix_ms":2,"latency_us":10,"outcome":"ok"}]"#,
        ),
    )
    .expect("fixture should write");

    let bin = env!("CARGO_BIN_EXE_tailtriage");
    let output = Command::new(bin)
        .arg("analyze")
        .arg(&run_path)
        .arg("--format")
        .arg("json")
        .output()
        .expect("cli should run");

    assert!(
        output.status.success(),
        "expected successful exit, stderr was: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf8");
    let report_json: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout should be JSON");
    assert!(report_json.get("primary_suspect").is_some());
}

#[test]
fn cli_loader_rejects_empty_requests_but_analyzer_accepts_in_memory_zero_request_run() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let run_path = dir.path().join("empty-requests.json");
    std::fs::write(&run_path, valid_run_json_with_requests("[]")).expect("fixture should write");

    let error =
        load_run_artifact(&run_path).expect_err("empty requests should be rejected by CLI loader");
    assert!(error.to_string().contains("requests section is empty"));

    let run: Run = serde_json::from_str(&valid_run_json_with_requests("[]"))
        .expect("in-memory run should deserialize");
    let report = analyze_run(&run, AnalyzeOptions::default());
    assert_eq!(report.request_count, 0);
}
