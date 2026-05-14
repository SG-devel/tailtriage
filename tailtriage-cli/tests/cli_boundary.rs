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
fn cli_help_analyzer_options_prints_known_path() {
    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("analyze")
        .arg("--help-analyzer-options")
        .output()
        .expect("cli should run");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf8");
    assert!(stdout.contains("queueing.trigger_permille"));
}

#[test]
fn cli_help_analyzer_options_does_not_require_run_json() {
    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("analyze")
        .arg("--help-analyzer-options")
        .output()
        .expect("cli should run");

    assert!(output.status.success());
}

#[test]
fn cli_missing_run_json_without_help_fails_clearly() {
    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("analyze")
        .output()
        .expect("cli should run");

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    assert!(stderr.contains("missing required argument RUN_JSON"));
}

#[test]
fn cli_analyzer_config_applies_toml() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let artifact_path = dir.path().join("run.json");
    let config_path = dir.path().join("analyzer.toml");
    std::fs::write(&artifact_path, valid_cli_artifact_with_requests())
        .expect("fixture should write");
    std::fs::write(
        &config_path,
        "[analyzer]\nschema_version=1\n[analyzer.queueing]\ntrigger_permille=400\n",
    )
    .expect("config should write");

    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("analyze")
        .arg(&artifact_path)
        .arg("--analyzer-config")
        .arg(&config_path)
        .output()
        .expect("cli should run");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn cli_analyzer_set_override_applies() {
    let output = run_analyze_with_args(&["--analyzer-set", "queueing.trigger_permille=400"]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn cli_analyzer_set_beats_toml_and_last_wins() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let artifact_path = dir.path().join("run.json");
    let config_path = dir.path().join("analyzer.toml");
    std::fs::write(&artifact_path, valid_cli_artifact_with_requests())
        .expect("fixture should write");
    std::fs::write(
        &config_path,
        "[analyzer]\nschema_version=1\n[analyzer.queueing]\ntrigger_permille=500\n",
    )
    .expect("config should write");

    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("analyze")
        .arg(&artifact_path)
        .arg("--analyzer-config")
        .arg(&config_path)
        .arg("--analyzer-set")
        .arg("queueing.trigger_permille=450")
        .arg("--analyzer-set")
        .arg("queueing.trigger_permille=410")
        .output()
        .expect("cli should run");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn cli_misspelled_override_reports_suggestion() {
    let output = run_analyze_with_args(&["--analyzer-set", "queuing.trigger_permille=400"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf8");
    let combined = format!("{stderr}{stdout}");
    assert!(combined.contains("queueing.trigger_permille"));
}

#[test]
fn cli_invalid_override_type_reports_expected_type() {
    let output = run_analyze_with_args(&["--analyzer-set", "queueing.trigger_permille=abc"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf8");
    let combined = format!("{stderr}{stdout}");
    assert!(combined.contains("queueing.trigger_permille"));
    assert!(combined.contains("u64") || combined.contains("invalid"));
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

fn run_analyze_with_args(args: &[&str]) -> std::process::Output {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let artifact_path = dir.path().join("run.json");
    std::fs::write(&artifact_path, valid_cli_artifact_with_requests())
        .expect("fixture should write");

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_tailtriage"));
    cmd.arg("analyze").arg(&artifact_path);
    for arg in args {
        cmd.arg(arg);
    }
    cmd.output().expect("cli should run")
}

fn valid_cli_artifact_with_requests() -> &'static str {
    r#"{"schema_version":1,"metadata":{"run_id":"r1","service_name":"svc","service_version":null,"started_at_unix_ms":1,"finished_at_unix_ms":2,"mode":"light","host":null,"pid":null,"lifecycle_warnings":[],"unfinished_requests":{"count":0,"sample":[]}},"requests":[{"request_id":"req1","route":"/","kind":null,"started_at_unix_ms":1,"finished_at_unix_ms":2,"latency_us":10,"outcome":"ok"}],"stages":[],"queues":[],"inflight":[],"runtime_snapshots":[]}"#
}

fn valid_cli_artifact_with_empty_requests() -> &'static str {
    r#"{"schema_version":1,"metadata":{"run_id":"r1","service_name":"svc","service_version":null,"started_at_unix_ms":1,"finished_at_unix_ms":2,"mode":"light","host":null,"pid":null,"lifecycle_warnings":[],"unfinished_requests":{"count":0,"sample":[]}},"requests":[],"stages":[],"queues":[],"inflight":[],"runtime_snapshots":[]}"#
}
