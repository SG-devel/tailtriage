use std::process::Command;

use tailtriage_analyzer::{analyze_run, AnalyzeOptions};
use tailtriage_core::{CaptureMode, Run};

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
fn cli_strict_artifact_rejects_duplicate_completed_request_ids() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let artifact_path = dir.path().join("duplicate-ids.json");
    std::fs::write(&artifact_path, duplicate_request_id_artifact()).expect("fixture should write");

    let permissive = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("analyze")
        .arg(&artifact_path)
        .arg("--format")
        .arg("json")
        .output()
        .expect("cli should run");
    let report = parse_report_json(permissive);
    assert!(report["warnings"]
        .as_array()
        .expect("warnings should be array")
        .iter()
        .any(|warning| warning
            .as_str()
            .is_some_and(|warning| warning.contains("Duplicate completed request_id"))));

    let strict = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("analyze")
        .arg(&artifact_path)
        .arg("--strict-artifact")
        .output()
        .expect("cli should run");

    assert!(!strict.status.success(), "strict cli should fail");
    let stderr = String::from_utf8_lossy(&strict.stderr);
    assert!(stderr.contains("strict artifact validation failed"));
    assert!(stderr.contains("Duplicate completed request_id"));
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
fn import_tracing_spans_jsonl_creates_missing_output_parent_directories() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let spans_path = dir.path().join("spans.jsonl");
    let run_path = dir.path().join("missing-parent/run.json");
    std::fs::write(&spans_path, complete_span_jsonl_fixture()).expect("fixture should write");

    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("import")
        .arg("tracing-spans-jsonl")
        .arg(&spans_path)
        .arg("--service")
        .arg("checkout")
        .arg("--output")
        .arg(&run_path)
        .output()
        .expect("cli should run");

    assert!(output.status.success(), "cli failed: {output:?}");
    assert!(String::from_utf8_lossy(&output.stdout).trim().is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).trim().is_empty());
    assert!(run_path.exists(), "run artifact should be written");

    let loaded = tailtriage_cli::artifact::load_run_artifact(&run_path)
        .expect("imported run should load in cli loader");
    assert_eq!(loaded.run.requests.len(), 1);
}

#[test]
fn import_tracing_spans_jsonl_fails_when_output_parent_path_is_not_directory() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let spans_path = dir.path().join("spans.jsonl");
    let invalid_parent = dir.path().join("not-a-dir");
    let run_path = invalid_parent.join("run.json");
    std::fs::write(&spans_path, complete_span_jsonl_fixture()).expect("fixture should write");
    std::fs::write(&invalid_parent, b"not-a-directory").expect("sentinel file should write");

    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("import")
        .arg("tracing-spans-jsonl")
        .arg(&spans_path)
        .arg("--service")
        .arg("checkout")
        .arg("--output")
        .arg(&run_path)
        .output()
        .expect("cli should run");

    assert!(!output.status.success(), "cli unexpectedly succeeded");
    assert!(String::from_utf8_lossy(&output.stdout).trim().is_empty());
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    assert!(
        stderr.contains("failed to create output parent directory")
            || stderr.contains(&invalid_parent.display().to_string())
    );
    assert!(!run_path.exists(), "run artifact should not be written");
}

#[test]
fn import_tracing_spans_jsonl_writes_run_json_analyzable_by_existing_apis() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let spans_path = dir.path().join("spans.jsonl");
    let run_path = dir.path().join("run.json");
    std::fs::write(&spans_path, complete_span_jsonl_fixture()).expect("fixture should write");

    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("import")
        .arg("tracing-spans-jsonl")
        .arg(&spans_path)
        .arg("--service")
        .arg("checkout")
        .arg("--output")
        .arg(&run_path)
        .output()
        .expect("cli should run");

    assert!(output.status.success(), "cli failed: {output:?}");
    assert!(String::from_utf8_lossy(&output.stdout).trim().is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).trim().is_empty());

    let loaded = tailtriage_cli::artifact::load_run_artifact(&run_path)
        .expect("imported run should load in cli loader");
    let report = analyze_run(&loaded.run, AnalyzeOptions::default());
    assert_eq!(report.request_count, 1);
}

#[test]
fn import_tracing_spans_jsonl_writes_run_json_when_output_path_contains_spaces() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let spans_path = dir.path().join("spans.jsonl");
    let run_path = dir.path().join("run artifact with spaces.json");
    std::fs::write(&spans_path, complete_span_jsonl_fixture()).expect("fixture should write");

    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("import")
        .arg("tracing-spans-jsonl")
        .arg(&spans_path)
        .arg("--service")
        .arg("checkout")
        .arg("--output")
        .arg(&run_path)
        .output()
        .expect("cli should run");

    assert!(output.status.success(), "cli failed: {output:?}");
    assert!(String::from_utf8_lossy(&output.stdout).trim().is_empty());

    let loaded = tailtriage_cli::artifact::load_run_artifact(&run_path)
        .expect("imported run should load from spaced output path");
    assert_eq!(loaded.run.requests.len(), 1);
}

#[test]
fn import_tracing_spans_jsonl_mode_investigation_sets_run_metadata_mode() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let spans_path = dir.path().join("spans.jsonl");
    let run_path = dir.path().join("run.json");
    std::fs::write(&spans_path, complete_span_jsonl_fixture()).expect("fixture should write");
    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("import")
        .arg("tracing-spans-jsonl")
        .arg(&spans_path)
        .arg("--mode")
        .arg("investigation")
        .arg("--service")
        .arg("checkout")
        .arg("--output")
        .arg(&run_path)
        .output()
        .expect("cli should run");
    assert!(output.status.success(), "cli failed: {output:?}");
    let loaded = tailtriage_cli::artifact::load_run_artifact(&run_path).unwrap();
    assert_eq!(loaded.run.metadata.mode, CaptureMode::Investigation);
}

#[test]
fn import_tracing_spans_jsonl_capture_limit_overrides_apply() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let spans_path = dir.path().join("spans.jsonl");
    let run_path = dir.path().join("run.json");
    std::fs::write(&spans_path, multi_span_jsonl_fixture()).expect("fixture should write");
    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("import")
        .arg("tracing-spans-jsonl")
        .arg(&spans_path)
        .arg("--service")
        .arg("checkout")
        .arg("--output")
        .arg(&run_path)
        .arg("--input-format")
        .arg("compatible")
        .arg("--max-requests")
        .arg("1")
        .arg("--max-stages")
        .arg("1")
        .arg("--max-queues")
        .arg("1")
        .output()
        .expect("cli should run");
    assert!(output.status.success(), "cli failed: {output:?}");
    let loaded = tailtriage_cli::artifact::load_run_artifact(&run_path).unwrap();
    assert_eq!(loaded.run.requests.len(), 1);
    assert_eq!(loaded.run.stages.len(), 1);
    assert_eq!(loaded.run.queues.len(), 1);
}

#[test]
fn import_tracing_spans_jsonl_rejects_zero_max_requests() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let spans_path = dir.path().join("spans.jsonl");
    let run_path = dir.path().join("run.json");
    std::fs::write(&spans_path, complete_span_jsonl_fixture()).expect("fixture should write");

    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("import")
        .arg("tracing-spans-jsonl")
        .arg(&spans_path)
        .arg("--service")
        .arg("checkout")
        .arg("--output")
        .arg(&run_path)
        .arg("--max-requests")
        .arg("0")
        .output()
        .expect("cli should run");

    assert!(!output.status.success(), "cli unexpectedly succeeded");
    assert!(String::from_utf8_lossy(&output.stdout).trim().is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--max-requests"));
    assert!(stderr.contains("at least 1"));
    assert!(stderr.contains("persisted tracing import"));
    assert!(stderr.contains("tailtriage analyze requires at least one request event"));
    assert!(!run_path.exists());
}

#[test]
fn import_tracing_spans_jsonl_allows_zero_stage_and_queue_limits() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let spans_path = dir.path().join("spans.jsonl");
    let run_path = dir.path().join("run.json");
    std::fs::write(&spans_path, complete_span_jsonl_fixture()).expect("fixture should write");

    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("import")
        .arg("tracing-spans-jsonl")
        .arg(&spans_path)
        .arg("--service")
        .arg("checkout")
        .arg("--output")
        .arg(&run_path)
        .arg("--max-stages")
        .arg("0")
        .arg("--max-queues")
        .arg("0")
        .output()
        .expect("cli should run");

    assert!(output.status.success(), "cli failed: {output:?}");
    assert!(String::from_utf8_lossy(&output.stdout).trim().is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).trim().is_empty());

    let loaded = tailtriage_cli::artifact::load_run_artifact(&run_path)
        .expect("imported request-only run should load in cli loader");
    assert!(!loaded.run.requests.is_empty());
    assert!(loaded.run.stages.is_empty());
    assert!(loaded.run.queues.is_empty());
}

#[test]
fn import_tracing_spans_jsonl_rejects_inert_runtime_snapshot_flags() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let spans_path = dir.path().join("spans.jsonl");
    let run_path = dir.path().join("run.json");
    std::fs::write(&spans_path, complete_span_jsonl_fixture()).expect("fixture should write");
    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("import")
        .arg("tracing-spans-jsonl")
        .arg(&spans_path)
        .arg("--service")
        .arg("checkout")
        .arg("--output")
        .arg(&run_path)
        .arg("--max-runtime-snapshots")
        .arg("1")
        .output()
        .expect("cli should run");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unexpected argument '--max-runtime-snapshots'"));
}

#[test]
fn import_tracing_spans_jsonl_rejects_inert_inflight_snapshot_flags() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let spans_path = dir.path().join("spans.jsonl");
    let run_path = dir.path().join("run.json");
    std::fs::write(&spans_path, complete_span_jsonl_fixture()).expect("fixture should write");
    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("import")
        .arg("tracing-spans-jsonl")
        .arg(&spans_path)
        .arg("--service")
        .arg("checkout")
        .arg("--output")
        .arg(&run_path)
        .arg("--max-inflight-snapshots")
        .arg("1")
        .output()
        .expect("cli should run");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unexpected argument '--max-inflight-snapshots'"));
}

#[test]
fn tailtriage_help_mentions_import_and_analyze_artifacts() {
    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("--help")
        .output()
        .expect("cli should run");
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Import and analyze tailtriage run artifacts"));
}

#[test]
fn import_tracing_spans_jsonl_input_format_tailtriage_wrapper_only_accepts_fixture() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let spans_path = dir.path().join("spans.jsonl");
    let run_path = dir.path().join("run.json");
    std::fs::write(&spans_path, complete_span_jsonl_fixture()).expect("fixture should write");

    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("import")
        .arg("tracing-spans-jsonl")
        .arg(&spans_path)
        .arg("--service")
        .arg("checkout")
        .arg("--output")
        .arg(&run_path)
        .output()
        .expect("cli should run");

    assert!(output.status.success(), "cli failed: {output:?}");
    assert!(String::from_utf8_lossy(&output.stdout).trim().is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).trim().is_empty());
    let loaded = tailtriage_cli::artifact::load_run_artifact(&run_path)
        .expect("imported run should load in cli loader");
    assert_eq!(loaded.run.requests.len(), 1);
    assert_eq!(loaded.run.stages.len(), 1);
    assert_eq!(loaded.run.queues.len(), 1);
    let report = analyze_run(&loaded.run, AnalyzeOptions::default());
    assert_eq!(report.request_count, 1);
}

#[test]
fn import_tracing_spans_jsonl_input_format_tailtriage_wrapper_only_rejects_unwrapped() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let spans_path = dir.path().join("spans.jsonl");
    let run_path = dir.path().join("run.json");
    std::fs::write(&spans_path, one_valid_request_span_fixture()).expect("fixture should write");
    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("import")
        .arg("tracing-spans-jsonl")
        .arg(&spans_path)
        .arg("--service")
        .arg("checkout")
        .arg("--output")
        .arg(&run_path)
        .output()
        .expect("cli should run");
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("tailtriage.tracing-span.v1") || stderr.contains("stable wrapper"));
    assert!(stderr.contains("tracing_subscriber::fmt().json() logs are unsupported"));
    assert!(!stderr.contains("ordinary tracing log JSON"));
    assert!(!run_path.exists());
}

#[test]
fn import_tracing_spans_jsonl_default_wrapper_mode_rejects_wrong_wrapper_format_with_guidance() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let spans_path = dir.path().join("spans.jsonl");
    let run_path = dir.path().join("run.json");
    std::fs::write(&spans_path, r#"{"format":"tailtriage.tracing-span.v2","span":{"name":"req","started_at_unix_ms":1,"finished_at_unix_ms":2,"fields":{"tt.kind":"request","tt.request_id":"r1","tt.route":"/a"}}}"#).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("import")
        .arg("tracing-spans-jsonl")
        .arg(&spans_path)
        .arg("--service")
        .arg("checkout")
        .arg("--output")
        .arg(&run_path)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("tailtriage.tracing-span.v1"));
}

#[test]
fn import_tracing_spans_jsonl_default_wrapper_mode_semantic_missing_route_no_wrapper_guidance() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let spans_path = dir.path().join("spans.jsonl");
    let run_path = dir.path().join("run.json");
    std::fs::write(&spans_path, r#"{"format":"tailtriage.tracing-span.v1","span":{"name":"req","started_at_unix_ms":1,"finished_at_unix_ms":2,"fields":{"tt.kind":"request","tt.request_id":"r1"}}}"#).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("import")
        .arg("tracing-spans-jsonl")
        .arg(&spans_path)
        .arg("--service")
        .arg("checkout")
        .arg("--output")
        .arg(&run_path)
        .arg("--strict")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("tt.route") || stderr.contains("missing required field"));
    assert!(!stderr.contains("tracing_subscriber::fmt().json()"));
}

#[test]
fn import_tracing_spans_jsonl_default_wrapper_mode_semantic_invalid_kind_type_no_wrapper_guidance()
{
    let dir = tempfile::tempdir().expect("tempdir should build");
    let spans_path = dir.path().join("spans.jsonl");
    let run_path = dir.path().join("run.json");
    std::fs::write(&spans_path, r#"{"format":"tailtriage.tracing-span.v1","span":{"name":"req","started_at_unix_ms":1,"finished_at_unix_ms":2,"fields":{"tt.kind":1,"tt.request_id":"r1","tt.route":"/a"}}}"#).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("import")
        .arg("tracing-spans-jsonl")
        .arg(&spans_path)
        .arg("--service")
        .arg("checkout")
        .arg("--output")
        .arg(&run_path)
        .arg("--strict")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("tt.kind") || stderr.contains("invalid field"));
    assert!(!stderr.contains("tracing_subscriber::fmt().json()"));
}

#[test]
fn import_tracing_spans_jsonl_default_wrapper_mode_missing_input_does_not_append_wrapper_guidance()
{
    let dir = tempfile::tempdir().expect("tempdir should build");
    let missing_spans_path = dir.path().join("missing-spans.jsonl");
    let run_path = dir.path().join("run.json");
    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("import")
        .arg("tracing-spans-jsonl")
        .arg(&missing_spans_path)
        .arg("--service")
        .arg("checkout")
        .arg("--output")
        .arg(&run_path)
        .output()
        .expect("cli should run");
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains(&missing_spans_path.display().to_string()));
    assert!(!stderr.contains("tailtriage.tracing-span.v1"));
    assert!(!stderr.contains("tracing_subscriber::fmt().json()"));
}

#[test]
fn import_tracing_spans_jsonl_default_wrapper_mode_malformed_json_does_not_append_wrapper_guidance()
{
    let dir = tempfile::tempdir().expect("tempdir should build");
    let spans_path = dir.path().join("bad.jsonl");
    let run_path = dir.path().join("run.json");
    std::fs::write(&spans_path, "{\"format\":").expect("fixture should write");
    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("import")
        .arg("tracing-spans-jsonl")
        .arg(&spans_path)
        .arg("--service")
        .arg("checkout")
        .arg("--output")
        .arg(&run_path)
        .output()
        .expect("cli should run");
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("malformed JSONL"));
    assert!(!stderr.contains("tailtriage.tracing-span.v1"));
    assert!(!stderr.contains("tracing_subscriber::fmt().json()"));
}

#[test]
fn import_tracing_spans_jsonl_default_rejects_fmt_json_with_guidance() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let spans_path = dir.path().join("fmt.jsonl");
    let run_path = dir.path().join("run.json");
    std::fs::write(
        &spans_path,
        [
            r#"{"timestamp":"2026-01-01T00:00:00Z","level":"INFO","target":"svc","fields":{"message":"close-1"}}"#,
            r#"{"timestamp":"2026-01-01T00:00:01Z","level":"INFO","target":"svc","fields":{"message":"close-2"}}"#,
            r#"{"timestamp":"2026-01-01T00:00:02Z","level":"INFO","target":"svc","fields":{"message":"close-3"}}"#,
        ]
        .join("
"),
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("import")
        .arg("tracing-spans-jsonl")
        .arg(&spans_path)
        .arg("--service")
        .arg("checkout")
        .arg("--output")
        .arg(&run_path)
        .output()
        .expect("cli should run");
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("tailtriage.tracing-span.v1"));
    assert!(stderr.contains("tracing_subscriber::fmt().json() logs are unsupported"));
    assert!(!stderr.contains("line 1: expected stable wrapper shape"));
    assert!(!stderr.contains("line 2: expected stable wrapper shape"));
    assert!(!run_path.exists());
}

#[test]
fn import_tracing_spans_jsonl_compatible_rejects_ordinary_fmt_json_without_completed_span_timing() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let spans_path = dir.path().join("compatible.jsonl");
    let run_path = dir.path().join("run.json");
    std::fs::write(
        &spans_path,
        r#"{"timestamp":"2026-01-01T00:00:00Z","level":"INFO","target":"svc","message":"ordinary log"}"#,
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("import")
        .arg("tracing-spans-jsonl")
        .arg(&spans_path)
        .arg("--service")
        .arg("checkout")
        .arg("--output")
        .arg(&run_path)
        .arg("--input-format")
        .arg("compatible")
        .output()
        .expect("cli should run");
    assert!(
        !output.status.success(),
        "cli unexpectedly succeeded: {output:?}"
    );
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    assert!(stderr.contains("ordinary tracing log JSON"));
    assert!(!run_path.exists(), "run json should not be written");
}

#[test]
fn import_tracing_spans_jsonl_compatible_accepts_fmt_metadata_with_completed_span_timing() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let spans_path = dir.path().join("compatible.jsonl");
    let run_path = dir.path().join("run.json");
    std::fs::write(
        &spans_path,
        r#"{"timestamp":"2026-01-01T00:00:00Z","level":"INFO","target":"svc","span":{"name":"request","started_at_unix_ms":1000,"finished_at_unix_ms":2000,"fields":{"tt.kind":"request","tt.request_id":"req-1","tt.route":"/checkout","tt.outcome":"ok","tt.success":true}}}"#,
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("import")
        .arg("tracing-spans-jsonl")
        .arg(&spans_path)
        .arg("--service")
        .arg("checkout")
        .arg("--output")
        .arg(&run_path)
        .arg("--input-format")
        .arg("compatible")
        .output()
        .expect("cli should run");
    assert!(output.status.success(), "cli failed: {output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).trim().is_empty(),
        "stderr should be empty: {output:?}"
    );
    assert!(run_path.exists(), "run json should be written");

    let loaded = tailtriage_cli::artifact::load_run_artifact(&run_path)
        .expect("imported run should load in cli loader");
    assert_eq!(loaded.run.requests.len(), 1);
    assert_eq!(loaded.run.requests[0].route, "/checkout");
}

#[test]
fn import_tracing_spans_jsonl_help_shows_only_live_input_format_values() {
    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("import")
        .arg("tracing-spans-jsonl")
        .arg("--help")
        .output()
        .expect("cli should run");
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("compatible"));
    assert!(stdout.contains("tailtriage-span-jsonl"));
    assert!(!stdout.contains("tracing-subscriber-fmt-json"));
}

#[test]
fn import_tracing_spans_jsonl_auto_is_rejected() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let spans_path = dir.path().join("spans.jsonl");
    let run_path = dir.path().join("run.json");
    std::fs::write(&spans_path, complete_span_jsonl_fixture()).expect("fixture should write");
    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("import")
        .arg("tracing-spans-jsonl")
        .arg(&spans_path)
        .arg("--service")
        .arg("checkout")
        .arg("--output")
        .arg(&run_path)
        .arg("--input-format")
        .arg("auto")
        .output()
        .expect("cli should run");
    assert!(!output.status.success());
}

#[test]
fn import_tracing_spans_jsonl_strict_fails_on_incomplete_tailtriage_span() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let spans_path = dir.path().join("spans.jsonl");
    let run_path = dir.path().join("run.json");
    std::fs::write(&spans_path, incomplete_tailtriage_span_fixture())
        .expect("fixture should write");

    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("import")
        .arg("tracing-spans-jsonl")
        .arg(&spans_path)
        .arg("--service")
        .arg("checkout")
        .arg("--output")
        .arg(&run_path)
        .arg("--input-format")
        .arg("compatible")
        .arg("--strict")
        .output()
        .expect("cli should run");

    assert!(!output.status.success(), "cli unexpectedly succeeded");
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    assert!(!stderr.trim().is_empty());
    assert!(
        !run_path.exists(),
        "run output should not exist on strict failure"
    );
}

#[test]
fn import_tracing_spans_jsonl_strict_with_max_requests_keeps_retained_request_and_skips_overflow_children(
) {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let spans_path = dir.path().join("spans.jsonl");
    let run_path = dir.path().join("run.json");
    std::fs::write(
        &spans_path,
        [
            r#"{"format":"tailtriage.tracing-span.v1","span":{"name":"req-1","started_at_unix_ms":100,"finished_at_unix_ms":200,"fields":{"tt.kind":"request","tt.request_id":"r1","tt.route":"/checkout"}}}"#,
            r#"{"format":"tailtriage.tracing-span.v1","span":{"name":"st-1","started_at_unix_ms":120,"finished_at_unix_ms":150,"fields":{"tt.kind":"stage","tt.request_id":"r1","tt.stage":"db"}}}"#,
            r#"{"format":"tailtriage.tracing-span.v1","span":{"name":"q-1","started_at_unix_ms":121,"finished_at_unix_ms":130,"fields":{"tt.kind":"queue","tt.request_id":"r1","tt.queue":"permits"}}}"#,
            r#"{"format":"tailtriage.tracing-span.v1","span":{"name":"req-2","started_at_unix_ms":300,"finished_at_unix_ms":400,"fields":{"tt.kind":"request","tt.request_id":"r2","tt.route":"/checkout"}}}"#,
            r#"{"format":"tailtriage.tracing-span.v1","span":{"name":"st-2","started_at_unix_ms":320,"finished_at_unix_ms":350,"fields":{"tt.kind":"stage","tt.request_id":"r2","tt.stage":"db"}}}"#,
            r#"{"format":"tailtriage.tracing-span.v1","span":{"name":"q-2","started_at_unix_ms":321,"finished_at_unix_ms":330,"fields":{"tt.kind":"queue","tt.request_id":"r2","tt.queue":"permits"}}}"#,
        ]
        .join("
"),
    )
    .expect("fixture should write");

    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("import")
        .arg("tracing-spans-jsonl")
        .arg(&spans_path)
        .arg("--service")
        .arg("checkout")
        .arg("--output")
        .arg(&run_path)
        .arg("--strict")
        .arg("--max-requests")
        .arg("1")
        .output()
        .expect("cli should run");

    assert!(output.status.success(), "cli failed: {output:?}");
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    assert!(!stderr.contains("no retained request event was imported"));
    assert!(run_path.exists(), "run output should exist");

    let loaded = tailtriage_cli::artifact::load_run_artifact(&run_path)
        .expect("imported run should load in cli loader");
    assert_eq!(loaded.run.requests.len(), 1);
    assert_eq!(loaded.run.requests[0].request_id, "r1");
    assert_eq!(loaded.run.stages.len(), 1);
    assert_eq!(loaded.run.stages[0].request_id, "r1");
    assert_eq!(loaded.run.queues.len(), 1);
    assert_eq!(loaded.run.queues[0].request_id, "r1");
    assert_eq!(loaded.run.truncation.dropped_requests, 1);
    assert_eq!(loaded.run.truncation.dropped_stages, 1);
    assert_eq!(loaded.run.truncation.dropped_queues, 1);
    assert!(loaded.run.truncation.limits_hit);
}

#[test]
fn import_tracing_spans_jsonl_strict_with_max_requests_fails_on_invalid_overflow_stage() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let spans_path = dir.path().join("spans.jsonl");
    let run_path = dir.path().join("run.json");
    std::fs::write(
        &spans_path,
        [
            r#"{"format":"tailtriage.tracing-span.v1","span":{"name":"req-1","started_at_unix_ms":100,"finished_at_unix_ms":200,"fields":{"tt.kind":"request","tt.request_id":"r1","tt.route":"/checkout"}}}"#,
            r#"{"format":"tailtriage.tracing-span.v1","span":{"name":"req-2","started_at_unix_ms":300,"finished_at_unix_ms":400,"fields":{"tt.kind":"request","tt.request_id":"r2","tt.route":"/checkout"}}}"#,
            r#"{"format":"tailtriage.tracing-span.v1","span":{"name":"st-2","started_at_unix_ms":320,"finished_at_unix_ms":450,"fields":{"tt.kind":"stage","tt.request_id":"r2","tt.stage":"db"}}}"#,
        ]
        .join("\n"),
    )
    .expect("fixture should write");

    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("import")
        .arg("tracing-spans-jsonl")
        .arg(&spans_path)
        .arg("--service")
        .arg("checkout")
        .arg("--output")
        .arg(&run_path)
        .arg("--strict")
        .arg("--max-requests")
        .arg("1")
        .output()
        .expect("cli should run");

    assert!(!output.status.success(), "cli unexpectedly succeeded");
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    assert!(stderr.contains("falls outside request interval"));
    assert!(!stderr.contains("valid but not retained due to max_requests"));
    assert!(
        !run_path.exists(),
        "run output should not exist on strict failure"
    );
}

#[test]
fn import_tracing_spans_jsonl_non_strict_writes_output_and_emits_warning_to_stderr() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let spans_path = dir.path().join("spans.jsonl");
    let run_path = dir.path().join("run.json");
    std::fs::write(
        &spans_path,
        mixed_valid_and_incomplete_request_span_fixture(),
    )
    .expect("fixture should write");

    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("import")
        .arg("tracing-spans-jsonl")
        .arg(&spans_path)
        .arg("--service")
        .arg("checkout")
        .arg("--output")
        .arg(&run_path)
        .arg("--input-format")
        .arg("compatible")
        .output()
        .expect("cli should run");

    assert!(output.status.success(), "cli failed: {output:?}");
    assert!(String::from_utf8_lossy(&output.stdout).trim().is_empty());
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    assert!(stderr.contains("warning:"));
    assert!(run_path.exists(), "run output should be written");

    let loaded = tailtriage_cli::artifact::load_run_artifact(&run_path)
        .expect("imported run should load in cli loader");
    assert_eq!(loaded.run.requests.len(), 1);
}

#[test]
fn import_tracing_spans_jsonl_writes_metadata_flags_into_run_json() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let spans_path = dir.path().join("spans.jsonl");
    let run_path = dir.path().join("run.json");
    std::fs::write(&spans_path, one_valid_request_span_fixture()).expect("fixture should write");

    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("import")
        .arg("tracing-spans-jsonl")
        .arg(&spans_path)
        .arg("--service")
        .arg("checkout")
        .arg("--service-version")
        .arg("v1")
        .arg("--run-id")
        .arg("run-42")
        .arg("--output")
        .arg(&run_path)
        .arg("--input-format")
        .arg("compatible")
        .output()
        .expect("cli should run");

    assert!(output.status.success(), "cli failed: {output:?}");
    assert!(String::from_utf8_lossy(&output.stdout).trim().is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).trim().is_empty());

    let loaded = tailtriage_cli::artifact::load_run_artifact(&run_path)
        .expect("imported run should load in cli loader");
    assert_eq!(loaded.run.metadata.service_name, "checkout");
    assert_eq!(loaded.run.metadata.service_version.as_deref(), Some("v1"));
    assert_eq!(loaded.run.metadata.run_id, "run-42");
}

#[test]
fn import_tracing_spans_jsonl_accepts_paths_with_spaces() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let spans_path = dir.path().join("tracing spans.jsonl");
    let run_path = dir.path().join("imported run.json");
    std::fs::write(&spans_path, one_valid_request_span_fixture()).expect("fixture should write");

    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("import")
        .arg("tracing-spans-jsonl")
        .arg(&spans_path)
        .arg("--service")
        .arg("checkout")
        .arg("--output")
        .arg(&run_path)
        .arg("--input-format")
        .arg("compatible")
        .output()
        .expect("cli should run");

    assert!(output.status.success(), "cli failed: {output:?}");
    assert!(String::from_utf8_lossy(&output.stdout).trim().is_empty());
    assert!(run_path.exists(), "run output should be written");

    tailtriage_cli::artifact::load_run_artifact(&run_path)
        .expect("imported run should load in cli loader");
}

#[test]
fn import_tracing_spans_jsonl_rejects_whitespace_service_name() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let spans_path = dir.path().join("spans.jsonl");
    let run_path = dir.path().join("run.json");
    std::fs::write(&spans_path, one_valid_request_span_fixture()).expect("fixture should write");
    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("import")
        .arg("tracing-spans-jsonl")
        .arg(&spans_path)
        .arg("--service")
        .arg(" ")
        .arg("--output")
        .arg(&run_path)
        .output()
        .expect("cli should run");
    assert!(!output.status.success(), "cli unexpectedly succeeded");
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    assert!(stderr.contains("expected stable wrapper shape"));
    assert!(!run_path.exists(), "run output should not be written");
}

#[test]
fn import_tracing_spans_jsonl_fails_when_only_unrelated_lines_are_present() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let spans_path = dir.path().join("spans.jsonl");
    let run_path = dir.path().join("run.json");
    std::fs::write(&spans_path, r#"{"format":"tailtriage.tracing-span.v1","span":{"name":"plain","started_at_unix_ms":1,"finished_at_unix_ms":2}}"#).expect("fixture should write");
    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("import")
        .arg("tracing-spans-jsonl")
        .arg(&spans_path)
        .arg("--service")
        .arg("checkout")
        .arg("--output")
        .arg(&run_path)
        .output()
        .expect("cli should run");
    assert!(!output.status.success(), "cli unexpectedly succeeded");
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    assert!(stderr.contains("zero request events"));
    assert!(stderr.contains("zero request events"));
    assert!(!run_path.exists(), "run output should not be written");
}

#[test]
fn import_tracing_spans_jsonl_fails_when_non_strict_skips_all_malformed_tt_spans() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let spans_path = dir.path().join("spans.jsonl");
    let run_path = dir.path().join("run.json");
    std::fs::write(&spans_path, malformed_tailtriage_span_fixture()).expect("fixture should write");
    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("import")
        .arg("tracing-spans-jsonl")
        .arg(&spans_path)
        .arg("--service")
        .arg("checkout")
        .arg("--output")
        .arg(&run_path)
        .output()
        .expect("cli should run");
    assert!(!output.status.success(), "cli unexpectedly succeeded");
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    assert!(stderr.contains("expected stable wrapper shape"));
    assert!(!stderr.trim().is_empty());
    assert!(!run_path.exists(), "run output should not be written");
}

#[test]
fn import_tracing_spans_jsonl_fails_when_only_tt_spans_are_missing_kind() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let spans_path = dir.path().join("spans.jsonl");
    let run_path = dir.path().join("run.json");
    std::fs::write(&spans_path, only_missing_kind_tailtriage_spans_fixture())
        .expect("fixture should write");
    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("import")
        .arg("tracing-spans-jsonl")
        .arg(&spans_path)
        .arg("--service")
        .arg("checkout")
        .arg("--output")
        .arg(&run_path)
        .arg("--input-format")
        .arg("compatible")
        .output()
        .expect("cli should run");
    assert!(!output.status.success(), "cli unexpectedly succeeded");
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    assert!(stderr.contains("warning:"));
    assert!(stderr.contains("missing required field 'tt.kind'"));
    assert!(stderr.contains("zero request events"));
    assert!(!run_path.exists(), "run output should not be written");
}

#[test]
fn import_tracing_spans_jsonl_warns_for_tt_fields_missing_kind_and_still_writes_run() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let spans_path = dir.path().join("spans.jsonl");
    let run_path = dir.path().join("run.json");
    std::fs::write(&spans_path, mixed_valid_and_missing_kind_fixture())
        .expect("fixture should write");
    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("import")
        .arg("tracing-spans-jsonl")
        .arg(&spans_path)
        .arg("--service")
        .arg("checkout")
        .arg("--output")
        .arg(&run_path)
        .arg("--input-format")
        .arg("compatible")
        .output()
        .expect("cli should run");
    assert!(
        output.status.success(),
        "cli unexpectedly failed: {output:?}"
    );
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    assert!(stderr.contains("warning:"));
    assert!(stderr.contains("missing required field 'tt.kind'"));
    assert!(run_path.exists(), "run output should be written");
    let loaded = tailtriage_cli::artifact::load_run_artifact(&run_path)
        .expect("imported run should load in cli loader");
    let warning_matches = loaded
        .run
        .metadata
        .lifecycle_warnings
        .iter()
        .filter(|warning| warning.as_str() == "missing required field 'tt.kind' in span 'oops'")
        .count();
    assert_eq!(warning_matches, 1);
}

#[test]
fn import_tracing_spans_jsonl_persists_unknown_kind_warning_in_run_artifact() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let spans_path = dir.path().join("spans.jsonl");
    let run_path = dir.path().join("run.json");
    std::fs::write(&spans_path, mixed_valid_and_unknown_kind_fixture())
        .expect("fixture should write");
    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("import")
        .arg("tracing-spans-jsonl")
        .arg(&spans_path)
        .arg("--service")
        .arg("checkout")
        .arg("--output")
        .arg(&run_path)
        .arg("--input-format")
        .arg("compatible")
        .output()
        .expect("cli should run");

    assert!(
        output.status.success(),
        "cli unexpectedly failed: {output:?}"
    );
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    assert!(stderr.contains("warning:"));
    assert!(stderr.contains("unknown tt.kind 'mystery' in span 'unknown'"));
    let loaded = tailtriage_cli::artifact::load_run_artifact(&run_path)
        .expect("imported run should load in cli loader");
    let warning_matches = loaded
        .run
        .metadata
        .lifecycle_warnings
        .iter()
        .filter(|warning| warning.as_str() == "unknown tt.kind 'mystery' in span 'unknown'")
        .count();
    assert_eq!(warning_matches, 1);
}

#[test]
fn import_tracing_spans_jsonl_persists_optional_default_assumption_warnings_in_run_artifact() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let spans_path = dir.path().join("spans.jsonl");
    let run_path = dir.path().join("run.json");
    std::fs::write(&spans_path, missing_optional_defaults_fixture()).expect("fixture should write");
    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("import")
        .arg("tracing-spans-jsonl")
        .arg(&spans_path)
        .arg("--service")
        .arg("checkout")
        .arg("--output")
        .arg(&run_path)
        .arg("--input-format")
        .arg("compatible")
        .output()
        .expect("cli should run");

    assert!(
        output.status.success(),
        "cli unexpectedly failed: {output:?}"
    );
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    assert!(stderr.contains("warning:"));
    assert!(stderr.contains("missing optional 'tt.outcome'; assumed 'ok'"));
    assert!(stderr.contains("missing optional 'tt.success'; assumed true"));

    let loaded = tailtriage_cli::artifact::load_run_artifact(&run_path)
        .expect("imported run should load in cli loader");
    assert!(loaded
        .run
        .metadata
        .lifecycle_warnings
        .iter()
        .any(|warning| warning.contains("missing optional 'tt.outcome'; assumed 'ok'")));
    assert!(loaded
        .run
        .metadata
        .lifecycle_warnings
        .iter()
        .any(|warning| warning.contains("missing optional 'tt.success'; assumed true")));

    let report = analyze_run(&loaded.run, AnalyzeOptions::default());
    assert_eq!(report.request_count, 1);
}

#[test]
fn import_tracing_spans_jsonl_whitespace_outcome_non_strict_warns_and_fails_zero_request() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let spans_path = dir.path().join("spans.jsonl");
    let run_path = dir.path().join("run.json");
    std::fs::write(&spans_path, invalid_whitespace_outcome_only_fixture())
        .expect("fixture should write");
    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("import")
        .arg("tracing-spans-jsonl")
        .arg(&spans_path)
        .arg("--service")
        .arg("checkout")
        .arg("--output")
        .arg(&run_path)
        .output()
        .expect("cli should run");

    assert!(!output.status.success(), "cli unexpectedly succeeded");
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    assert!(stderr.contains("warning:"));
    assert!(stderr.contains("expected non-empty, non-whitespace string"));
    assert!(stderr.contains("zero request events"));
}

#[test]
fn import_tracing_spans_jsonl_whitespace_outcome_strict_fails_with_message() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let spans_path = dir.path().join("spans.jsonl");
    let run_path = dir.path().join("run.json");
    std::fs::write(&spans_path, invalid_whitespace_outcome_only_fixture())
        .expect("fixture should write");
    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("import")
        .arg("tracing-spans-jsonl")
        .arg(&spans_path)
        .arg("--service")
        .arg("checkout")
        .arg("--output")
        .arg(&run_path)
        .arg("--strict")
        .output()
        .expect("cli should run");

    assert!(!output.status.success(), "cli unexpectedly succeeded");
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    assert!(stderr.contains("expected non-empty, non-whitespace string"));
}

#[test]
fn import_tracing_spans_jsonl_valid_outcomes_import_successfully() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let spans_path = dir.path().join("spans.jsonl");
    let run_path = dir.path().join("run.json");
    std::fs::write(&spans_path, valid_outcomes_fixture()).expect("fixture should write");
    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("import")
        .arg("tracing-spans-jsonl")
        .arg(&spans_path)
        .arg("--service")
        .arg("checkout")
        .arg("--output")
        .arg(&run_path)
        .output()
        .expect("cli should run");
    assert!(output.status.success(), "cli failed: {output:?}");

    let loaded = tailtriage_cli::artifact::load_run_artifact(&run_path)
        .expect("imported run should load in cli loader");
    let outcomes = loaded
        .run
        .requests
        .iter()
        .map(|request| request.outcome.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        outcomes,
        vec!["ok", "error", "timeout", "cancelled", "rejected"]
    );
}

fn duplicate_request_id_artifact() -> &'static str {
    r#"{"schema_version":1,"metadata":{"run_id":"r1","service_name":"svc","service_version":null,"started_at_unix_ms":1,"finished_at_unix_ms":3,"mode":"light","host":null,"pid":null,"lifecycle_warnings":[],"unfinished_requests":{"count":0,"sample":[]}},"requests":[{"request_id":"req1","route":"/","kind":null,"started_at_unix_ms":1,"finished_at_unix_ms":2,"latency_us":10,"outcome":"ok"},{"request_id":"req1","route":"/","kind":null,"started_at_unix_ms":2,"finished_at_unix_ms":3,"latency_us":11,"outcome":"ok"}],"stages":[],"queues":[],"inflight":[],"runtime_snapshots":[]}"#
}

fn valid_cli_artifact_with_requests() -> &'static str {
    r#"{"schema_version":1,"metadata":{"run_id":"r1","service_name":"svc","service_version":null,"started_at_unix_ms":1,"finished_at_unix_ms":2,"mode":"light","host":null,"pid":null,"lifecycle_warnings":[],"unfinished_requests":{"count":0,"sample":[]}},"requests":[{"request_id":"req1","route":"/","kind":null,"started_at_unix_ms":1,"finished_at_unix_ms":2,"latency_us":10,"outcome":"ok"}],"stages":[],"queues":[],"inflight":[],"runtime_snapshots":[]}"#
}

fn missing_optional_defaults_fixture() -> &'static str {
    r#"{"span":{"name":"request","started_at_unix_ms":1000,"finished_at_unix_ms":1010,"fields":{"tt.kind":"request","tt.request_id":"req-1","tt.route":"/checkout"}}}
{"span":{"name":"stage","started_at_unix_ms":1001,"finished_at_unix_ms":1009,"fields":{"tt.kind":"stage","tt.request_id":"req-1","tt.stage":"db"}}}
"#
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

fn complete_span_jsonl_fixture() -> &'static str {
    include_str!("../../tailtriage-tracing/tests/fixtures/tailtriage-span-v1.jsonl")
}

fn incomplete_tailtriage_span_fixture() -> &'static str {
    r#"{"span":{"name":"http.request","started_at_unix_ms":1000,"finished_at_unix_ms":1010,"fields":{"tt.kind":"request","tt.request_id":"req-1"}}}
"#
}

fn one_valid_request_span_fixture() -> &'static str {
    r#"{"span":{"name":"http.request","started_at_unix_ms":1000,"finished_at_unix_ms":1010,"fields":{"tt.kind":"request","tt.request_id":"req-1","tt.route":"/checkout","tt.outcome":"ok"}}}
"#
}

fn multi_span_jsonl_fixture() -> &'static str {
    r#"{"span":{"name":"req-1","started_at_unix_ms":1000,"finished_at_unix_ms":1010,"fields":{"tt.kind":"request","tt.request_id":"req-1","tt.route":"/checkout"}}}
{"span":{"name":"req-2","started_at_unix_ms":1020,"finished_at_unix_ms":1030,"fields":{"tt.kind":"request","tt.request_id":"req-2","tt.route":"/checkout"}}}
{"span":{"name":"stage-1","started_at_unix_ms":1001,"finished_at_unix_ms":1009,"fields":{"tt.kind":"stage","tt.request_id":"req-1","tt.stage":"db"}}}
{"span":{"name":"stage-2","started_at_unix_ms":1021,"finished_at_unix_ms":1029,"fields":{"tt.kind":"stage","tt.request_id":"req-2","tt.stage":"cache"}}}
{"span":{"name":"queue-1","started_at_unix_ms":1002,"finished_at_unix_ms":1008,"fields":{"tt.kind":"queue","tt.request_id":"req-1","tt.queue":"permits"}}}
{"span":{"name":"queue-2","started_at_unix_ms":1022,"finished_at_unix_ms":1028,"fields":{"tt.kind":"queue","tt.request_id":"req-2","tt.queue":"permits"}}}
"#
}

fn malformed_tailtriage_span_fixture() -> &'static str {
    r#"{"span":{"name":"req","started_at_unix_ms":"bad","finished_at_unix_ms":2,"fields":{"tt.kind":"request","tt.request_id":"r1","tt.route":"/a"}}}"#
}

fn mixed_valid_and_incomplete_request_span_fixture() -> &'static str {
    r#"{"span":{"name":"http.request","started_at_unix_ms":1000,"finished_at_unix_ms":1010,"fields":{"tt.kind":"request","tt.request_id":"req-1"}}}
{"span":{"name":"http.request","started_at_unix_ms":1020,"finished_at_unix_ms":1032,"fields":{"tt.kind":"request","tt.request_id":"req-2","tt.route":"/checkout","tt.outcome":"ok"}}}
"#
}

fn mixed_valid_and_missing_kind_fixture() -> &'static str {
    r#"{"span":{"name":"oops","started_at_unix_ms":1000,"finished_at_unix_ms":1005,"fields":{"tt.request_id":"req-0","tt.route":"/oops"}}}
{"span":{"name":"http.request","started_at_unix_ms":1020,"finished_at_unix_ms":1032,"fields":{"tt.kind":"request","tt.request_id":"req-2","tt.route":"/checkout","tt.outcome":"ok"}}}
"#
}

fn only_missing_kind_tailtriage_spans_fixture() -> &'static str {
    r#"{"span":{"name":"oops-1","started_at_unix_ms":1000,"finished_at_unix_ms":1005,"fields":{"tt.request_id":"req-0","tt.route":"/oops"}}}
{"span":{"name":"oops-2","started_at_unix_ms":1010,"finished_at_unix_ms":1015,"fields":{"tt.request_id":"req-1","tt.route":"/oops2"}}}
"#
}

fn mixed_valid_and_unknown_kind_fixture() -> &'static str {
    r#"{"span":{"name":"unknown","started_at_unix_ms":1000,"finished_at_unix_ms":1005,"fields":{"tt.kind":"mystery"}}}
{"span":{"name":"http.request","started_at_unix_ms":1020,"finished_at_unix_ms":1032,"fields":{"tt.kind":"request","tt.request_id":"req-2","tt.route":"/checkout","tt.outcome":"ok"}}}
"#
}

fn invalid_whitespace_outcome_only_fixture() -> &'static str {
    r#"{"format":"tailtriage.tracing-span.v1","span":{"name":"http.request","started_at_unix_ms":1000,"finished_at_unix_ms":1010,"fields":{"tt.kind":"request","tt.request_id":"req-1","tt.route":"/checkout","tt.outcome":" "}}}
"#
}

fn valid_outcomes_fixture() -> &'static str {
    r#"{"format":"tailtriage.tracing-span.v1","span":{"name":"http.request-1","started_at_unix_ms":1000,"finished_at_unix_ms":1010,"fields":{"tt.kind":"request","tt.request_id":"req-1","tt.route":"/checkout","tt.outcome":"ok"}}}
{"format":"tailtriage.tracing-span.v1","span":{"name":"http.request-2","started_at_unix_ms":1010,"finished_at_unix_ms":1020,"fields":{"tt.kind":"request","tt.request_id":"req-2","tt.route":"/checkout","tt.outcome":"error"}}}
{"format":"tailtriage.tracing-span.v1","span":{"name":"http.request-3","started_at_unix_ms":1020,"finished_at_unix_ms":1030,"fields":{"tt.kind":"request","tt.request_id":"req-3","tt.route":"/checkout","tt.outcome":"timeout"}}}
{"format":"tailtriage.tracing-span.v1","span":{"name":"http.request-4","started_at_unix_ms":1030,"finished_at_unix_ms":1040,"fields":{"tt.kind":"request","tt.request_id":"req-4","tt.route":"/checkout","tt.outcome":"cancelled"}}}
{"format":"tailtriage.tracing-span.v1","span":{"name":"http.request-5","started_at_unix_ms":1040,"finished_at_unix_ms":1050,"fields":{"tt.kind":"request","tt.request_id":"req-5","tt.route":"/checkout","tt.outcome":"rejected"}}}
"#
}
