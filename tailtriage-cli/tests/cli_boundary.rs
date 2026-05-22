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
fn import_tracing_json_writes_run_json_analyzable_by_existing_apis() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let spans_path = dir.path().join("spans.jsonl");
    let run_path = dir.path().join("run.json");
    std::fs::write(&spans_path, complete_span_jsonl_fixture()).expect("fixture should write");

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
    assert!(String::from_utf8_lossy(&output.stdout).trim().is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).trim().is_empty());

    let loaded = tailtriage_cli::artifact::load_run_artifact(&run_path)
        .expect("imported run should load in cli loader");
    let report = analyze_run(&loaded.run, AnalyzeOptions::default());
    assert_eq!(report.request_count, 1);
}

#[test]
fn import_tracing_json_input_format_tailtriage_wrapper_only_accepts_fixture() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let spans_path = dir.path().join("spans.jsonl");
    let run_path = dir.path().join("run.json");
    std::fs::write(&spans_path, complete_span_jsonl_fixture()).expect("fixture should write");

    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("import")
        .arg("tracing-json")
        .arg(&spans_path)
        .arg("--input-format")
        .arg("tailtriage-span-jsonl")
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
fn import_tracing_json_input_format_tailtriage_wrapper_only_rejects_unwrapped() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let spans_path = dir.path().join("spans.jsonl");
    let run_path = dir.path().join("run.json");
    std::fs::write(&spans_path, one_valid_request_span_fixture()).expect("fixture should write");
    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("import")
        .arg("tracing-json")
        .arg(&spans_path)
        .arg("--input-format")
        .arg("tailtriage-span-jsonl")
        .arg("--service")
        .arg("checkout")
        .arg("--output")
        .arg(&run_path)
        .output()
        .expect("cli should run");
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("tailtriage.tracing-span.v1") || stderr.contains("stable wrapper"));
    assert!(!stderr.contains("ordinary tracing log JSON"));
    assert!(!run_path.exists());
}

#[test]
fn import_tracing_json_auto_rejects_fmt_json_with_guidance() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let spans_path = dir.path().join("fmt.jsonl");
    let run_path = dir.path().join("run.json");
    std::fs::write(&spans_path, r#"{"timestamp":"2026-01-01T00:00:00Z","level":"INFO","target":"svc","fields":{"message":"close"}}"#).unwrap();
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
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("ordinary tracing log JSON"));
    assert!(stderr.contains("literal dotted tt.* keys"));
    assert!(stderr.contains("explicit unix-ms start/end timestamps"));
    assert!(stderr.contains("TracingIntakeSession"));
    assert!(stderr.contains("tailtriage-span-jsonl"));
    assert!(!run_path.exists());
}

#[test]
fn import_tracing_json_auto_accepts_fmt_like_record_with_top_level_explicit_timestamps() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let spans_path = dir.path().join("compatible.jsonl");
    let run_path = dir.path().join("run.json");
    std::fs::write(
        &spans_path,
        r#"{"timestamp":"2026-01-01T00:00:00Z","level":"INFO","target":"svc","name":"request","started_at_unix_ms":1000,"finished_at_unix_ms":2000,"tt.kind":"request","tt.request_id":"req-1","tt.route":"/checkout","tt.outcome":"ok"}"#,
    )
    .unwrap();
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
    assert!(
        !output.status.success(),
        "cli unexpectedly succeeded: {output:?}"
    );
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    assert!(!stderr.contains("ordinary tracing log JSON"));
    assert!(stderr.contains("zero request events"));
}

#[test]
fn import_tracing_json_auto_accepts_fmt_like_record_with_nested_explicit_timestamps() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let spans_path = dir.path().join("compatible.jsonl");
    let run_path = dir.path().join("run.json");
    std::fs::write(
        &spans_path,
        r#"{"timestamp":"2026-01-01T00:00:00Z","level":"INFO","target":"svc","span":{"started_at_unix_ms":1000,"finished_at_unix_ms":2000,"tt.kind":"request","tt.request_id":"req-1","tt.route":"/checkout","tt.outcome":"ok"}}"#,
    )
    .unwrap();
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
    assert!(
        !output.status.success(),
        "cli unexpectedly succeeded: {output:?}"
    );
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    assert!(!stderr.contains("ordinary tracing log JSON"));
    assert!(stderr.contains("zero request events"));
}

#[test]
fn import_tracing_json_help_shows_only_live_input_format_values() {
    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("import")
        .arg("tracing-json")
        .arg("--help")
        .output()
        .expect("cli should run");
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("auto"));
    assert!(stdout.contains("tailtriage-span-jsonl"));
    assert!(!stdout.contains("tracing-subscriber-fmt-json"));
}

#[test]
fn import_tracing_json_strict_fails_on_incomplete_tailtriage_span() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let spans_path = dir.path().join("spans.jsonl");
    let run_path = dir.path().join("run.json");
    std::fs::write(&spans_path, incomplete_tailtriage_span_fixture())
        .expect("fixture should write");

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
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    assert!(stderr.contains("strict violation") || stderr.contains("missing required field"));
    assert!(
        !run_path.exists(),
        "run output should not exist on strict failure"
    );
}

#[test]
fn import_tracing_json_non_strict_writes_output_and_emits_warning_to_stderr() {
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
        .arg("tracing-json")
        .arg(&spans_path)
        .arg("--service")
        .arg("checkout")
        .arg("--output")
        .arg(&run_path)
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
fn import_tracing_json_writes_metadata_flags_into_run_json() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let spans_path = dir.path().join("spans.jsonl");
    let run_path = dir.path().join("run.json");
    std::fs::write(&spans_path, one_valid_request_span_fixture()).expect("fixture should write");

    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("import")
        .arg("tracing-json")
        .arg(&spans_path)
        .arg("--service")
        .arg("checkout")
        .arg("--service-version")
        .arg("v1")
        .arg("--run-id")
        .arg("run-42")
        .arg("--output")
        .arg(&run_path)
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
fn import_tracing_json_accepts_paths_with_spaces() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let spans_path = dir.path().join("tracing spans.jsonl");
    let run_path = dir.path().join("imported run.json");
    std::fs::write(&spans_path, one_valid_request_span_fixture()).expect("fixture should write");

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
    assert!(String::from_utf8_lossy(&output.stdout).trim().is_empty());
    assert!(run_path.exists(), "run output should be written");

    tailtriage_cli::artifact::load_run_artifact(&run_path)
        .expect("imported run should load in cli loader");
}

#[test]
fn import_tracing_json_rejects_whitespace_service_name() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let spans_path = dir.path().join("spans.jsonl");
    let run_path = dir.path().join("run.json");
    std::fs::write(&spans_path, one_valid_request_span_fixture()).expect("fixture should write");
    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("import")
        .arg("tracing-json")
        .arg(&spans_path)
        .arg("--service")
        .arg(" ")
        .arg("--output")
        .arg(&run_path)
        .output()
        .expect("cli should run");
    assert!(!output.status.success(), "cli unexpectedly succeeded");
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    assert!(stderr.contains("service name must not be empty"));
    assert!(!run_path.exists(), "run output should not be written");
}

#[test]
fn import_tracing_json_fails_when_only_unrelated_lines_are_present() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let spans_path = dir.path().join("spans.jsonl");
    let run_path = dir.path().join("run.json");
    std::fs::write(&spans_path, r#"{"message":"ordinary"}"#).expect("fixture should write");
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
    assert!(!output.status.success(), "cli unexpectedly succeeded");
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    assert!(stderr.contains("zero request events"));
    assert!(!run_path.exists(), "run output should not be written");
}

#[test]
fn import_tracing_json_fails_when_non_strict_skips_all_malformed_tt_spans() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let spans_path = dir.path().join("spans.jsonl");
    let run_path = dir.path().join("run.json");
    std::fs::write(&spans_path, malformed_tailtriage_span_fixture()).expect("fixture should write");
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
    assert!(!output.status.success(), "cli unexpectedly succeeded");
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    assert!(stderr.contains("warning:"));
    assert!(stderr.contains("zero request events"));
    assert!(!run_path.exists(), "run output should not be written");
}

#[test]
fn import_tracing_json_fails_when_only_tt_spans_are_missing_kind() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let spans_path = dir.path().join("spans.jsonl");
    let run_path = dir.path().join("run.json");
    std::fs::write(&spans_path, only_missing_kind_tailtriage_spans_fixture())
        .expect("fixture should write");
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
    assert!(!output.status.success(), "cli unexpectedly succeeded");
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    assert!(stderr.contains("warning:"));
    assert!(stderr.contains("missing required field 'tt.kind'"));
    assert!(stderr.contains("zero request events"));
    assert!(!run_path.exists(), "run output should not be written");
}

#[test]
fn import_tracing_json_warns_for_tt_fields_missing_kind_and_still_writes_run() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let spans_path = dir.path().join("spans.jsonl");
    let run_path = dir.path().join("run.json");
    std::fs::write(&spans_path, mixed_valid_and_missing_kind_fixture())
        .expect("fixture should write");
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
fn import_tracing_json_persists_unknown_kind_warning_in_run_artifact() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let spans_path = dir.path().join("spans.jsonl");
    let run_path = dir.path().join("run.json");
    std::fs::write(&spans_path, mixed_valid_and_unknown_kind_fixture())
        .expect("fixture should write");
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
fn import_tracing_json_persists_optional_default_assumption_warnings_in_run_artifact() {
    let dir = tempfile::tempdir().expect("tempdir should build");
    let spans_path = dir.path().join("spans.jsonl");
    let run_path = dir.path().join("run.json");
    std::fs::write(&spans_path, missing_optional_defaults_fixture()).expect("fixture should write");
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
