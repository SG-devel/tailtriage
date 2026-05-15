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

#[test]
fn help_analyzer_options_works_without_run_json() {
    let exe = env!("CARGO_BIN_EXE_tailtriage");
    let output = Command::new(exe)
        .arg("analyze")
        .arg("--help-analyzer-options")
        .output()
        .expect("cli should run");

    assert!(output.status.success(), "cli failed: {output:?}");
    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf8");
    assert!(stdout.contains("queueing.trigger_permille"));
}

#[test]
fn cli_analyzer_config_applies_toml_and_reports_non_default_config() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let artifact_path = write_valid_artifact(&dir);
    let config_path = dir.path().join("analyzer.toml");
    std::fs::write(
        &config_path,
        "[analyzer]\nschema_version = 1\n\n[analyzer.queueing]\ntrigger_permille = 410\n",
    )
    .expect("config should write");

    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("analyze")
        .arg(&artifact_path)
        .arg("--format")
        .arg("json")
        .arg("--analyzer-config")
        .arg(&config_path)
        .output()
        .expect("cli should run");

    let report = parse_report_json(output);
    let non_defaults = &report["analyzer_config"]["non_default_options"];
    assert_eq!(non_defaults.as_array().map(Vec::len), Some(1));
    assert_eq!(non_defaults[0]["path"], "queueing.trigger_permille");
    assert_eq!(non_defaults[0]["value"], "410");
}

#[test]
fn cli_analyzer_set_applies_override_and_reports_non_default_config() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let artifact_path = write_valid_artifact(&dir);

    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("analyze")
        .arg(&artifact_path)
        .arg("--format")
        .arg("json")
        .arg("--analyzer-set")
        .arg("queueing.trigger_permille=420")
        .output()
        .expect("cli should run");

    let report = parse_report_json(output);
    let non_defaults = &report["analyzer_config"]["non_default_options"];
    assert_eq!(non_defaults.as_array().map(Vec::len), Some(1));
    assert_eq!(non_defaults[0]["path"], "queueing.trigger_permille");
    assert_eq!(non_defaults[0]["value"], "420");
}

#[test]
fn cli_analyzer_set_beats_toml_and_repeated_overrides_are_last_wins() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let artifact_path = write_valid_artifact(&dir);
    let config_path = dir.path().join("analyzer.toml");
    std::fs::write(
        &config_path,
        "[analyzer]\nschema_version = 1\n\n[analyzer.queueing]\ntrigger_permille = 410\n",
    )
    .expect("config should write");

    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("analyze")
        .arg(&artifact_path)
        .arg("--format")
        .arg("json")
        .arg("--analyzer-config")
        .arg(&config_path)
        .arg("--analyzer-set")
        .arg("queueing.trigger_permille=430")
        .arg("--analyzer-set")
        .arg("queueing.trigger_permille=440")
        .output()
        .expect("cli should run");

    let report = parse_report_json(output);
    let non_defaults = &report["analyzer_config"]["non_default_options"];
    assert_eq!(non_defaults.as_array().map(Vec::len), Some(1));
    assert_eq!(non_defaults[0]["path"], "queueing.trigger_permille");
    assert_eq!(non_defaults[0]["value"], "440");
}

#[test]
fn cli_misspelled_analyzer_set_reports_suggestion() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let artifact_path = write_valid_artifact(&dir);

    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("analyze")
        .arg(&artifact_path)
        .arg("--analyzer-set")
        .arg("queuing.trigger_permille=400")
        .output()
        .expect("cli should run");

    assert!(!output.status.success(), "cli unexpectedly succeeded");
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    assert!(stderr.contains("queueing.trigger_permille"));
    assert!(String::from_utf8_lossy(&output.stdout).trim().is_empty());
}

#[test]
fn cli_invalid_analyzer_set_type_reports_expected_type() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let artifact_path = write_valid_artifact(&dir);

    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("analyze")
        .arg(&artifact_path)
        .arg("--analyzer-set")
        .arg("queueing.trigger_permille=abc")
        .output()
        .expect("cli should run");

    assert!(!output.status.success(), "cli unexpectedly succeeded");
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    assert!(stderr.contains("queueing.trigger_permille"));
    assert!(stderr.contains("u64") || stderr.contains("expected"));
    assert!(String::from_utf8_lossy(&output.stdout).trim().is_empty());
}

#[test]
fn cli_missing_analyzer_config_file_reports_path() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let artifact_path = write_valid_artifact(&dir);
    let missing_path = dir.path().join("missing-analyzer.toml");

    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("analyze")
        .arg(&artifact_path)
        .arg("--analyzer-config")
        .arg(&missing_path)
        .output()
        .expect("cli should run");

    assert!(!output.status.success(), "cli unexpectedly succeeded");
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    assert!(stderr.contains(&format!(
        "failed to read analyzer config '{}'",
        missing_path.display()
    )));
    assert!(!stderr.contains("ReadConfig"));
    assert!(!stderr.contains("analyzer.config_path"));
    assert!(String::from_utf8_lossy(&output.stdout).trim().is_empty());
}

#[test]
fn missing_run_json_without_help_flag_fails_clearly() {
    let exe = env!("CARGO_BIN_EXE_tailtriage");
    let output = Command::new(exe)
        .arg("analyze")
        .output()
        .expect("cli should run");

    assert!(!output.status.success(), "cli unexpectedly succeeded");
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    assert!(stderr.contains("RUN_JSON") || stderr.contains("missing required"));
}

#[test]
fn import_tracing_json_writes_valid_run_json_artifact() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let spans_path = dir.path().join("spans.jsonl");
    let run_path = dir.path().join("run.json");
    std::fs::write(&spans_path, request_span_jsonl()).expect("fixture should write");

    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("import")
        .arg("tracing-json")
        .arg(&spans_path)
        .arg("--service")
        .arg("checkout")
        .arg("--output")
        .arg(&run_path)
        .output()
        .expect("cli should run");

    assert!(output.status.success(), "cli failed: {output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).trim().is_empty());

    let loaded = tailtriage_cli::artifact::load_run_artifact(&run_path)
        .expect("imported run should load via cli artifact loader");
    let report = analyze_run(&loaded.run, AnalyzeOptions::default());
    assert_eq!(report.request_count, 1);
}

#[test]
fn import_tracing_json_strict_fails_on_incomplete_tailtriage_span() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let spans_path = dir.path().join("incomplete.jsonl");
    let run_path = dir.path().join("run.json");
    std::fs::write(&spans_path, incomplete_request_span_jsonl()).expect("fixture should write");

    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("import")
        .arg("tracing-json")
        .arg(&spans_path)
        .arg("--service")
        .arg("checkout")
        .arg("--output")
        .arg(&run_path)
        .arg("--strict")
        .output()
        .expect("cli should run");

    assert!(!output.status.success(), "cli unexpectedly succeeded");
    assert!(!run_path.exists(), "output should not be created");
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    assert!(stderr.contains("strict import violation"));
}

#[test]
fn import_tracing_json_non_strict_emits_warnings_and_writes_output() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let spans_path = dir.path().join("incomplete.jsonl");
    let run_path = dir.path().join("run.json");
    std::fs::write(&spans_path, incomplete_request_span_jsonl()).expect("fixture should write");

    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("import")
        .arg("tracing-json")
        .arg(&spans_path)
        .arg("--service")
        .arg("checkout")
        .arg("--output")
        .arg(&run_path)
        .output()
        .expect("cli should run");

    assert!(output.status.success(), "cli failed: {output:?}");
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    assert!(stderr.contains("warning:"));
    assert!(run_path.exists(), "output should be created");
}

fn valid_cli_artifact_with_requests() -> &'static str {
    r#"{"schema_version":1,"metadata":{"run_id":"r1","service_name":"svc","service_version":null,"started_at_unix_ms":1,"finished_at_unix_ms":2,"mode":"light","host":null,"pid":null,"lifecycle_warnings":[],"unfinished_requests":{"count":0,"sample":[]}},"requests":[{"request_id":"req1","route":"/","kind":null,"started_at_unix_ms":1,"finished_at_unix_ms":2,"latency_us":10,"outcome":"ok"}],"stages":[],"queues":[],"inflight":[],"runtime_snapshots":[]}"#
}

fn write_valid_artifact(dir: &tempfile::TempDir) -> std::path::PathBuf {
    let artifact_path = dir.path().join("run.json");
    std::fs::write(&artifact_path, valid_cli_artifact_with_requests())
        .expect("fixture should write");
    artifact_path
}

fn parse_report_json(output: std::process::Output) -> serde_json::Value {
    assert!(output.status.success(), "cli failed: {output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).trim().is_empty());

    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf8");
    serde_json::from_str(&stdout).expect("stdout should be valid json")
}

fn valid_cli_artifact_with_empty_requests() -> &'static str {
    r#"{"schema_version":1,"metadata":{"run_id":"r1","service_name":"svc","service_version":null,"started_at_unix_ms":1,"finished_at_unix_ms":2,"mode":"light","host":null,"pid":null,"lifecycle_warnings":[],"unfinished_requests":{"count":0,"sample":[]}},"requests":[],"stages":[],"queues":[],"inflight":[],"runtime_snapshots":[]}"#
}

fn request_span_jsonl() -> &'static str {
    r#"{"span":{"name":"http.request","started_at_unix_ms":1000,"finished_at_unix_ms":1010,"fields":{"tt.kind":"request","tt.request_id":"req-1","tt.route":"/checkout","tt.success":true}}}
"#
}

fn incomplete_request_span_jsonl() -> &'static str {
    r#"{"span":{"name":"http.request","started_at_unix_ms":1000,"finished_at_unix_ms":1010,"fields":{"tt.kind":"request","tt.request_id":"req-1","tt.success":true}}}
"#
}
