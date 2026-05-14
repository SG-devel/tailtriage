use std::process::Command;

fn valid_cli_artifact_with_requests() -> &'static str {
    r#"{"schema_version":1,"metadata":{"run_id":"r1","service_name":"svc","service_version":null,"started_at_unix_ms":1,"finished_at_unix_ms":2,"mode":"light","host":null,"pid":null,"lifecycle_warnings":[],"unfinished_requests":{"count":0,"sample":[]}},"requests":[{"request_id":"req1","route":"/","kind":null,"started_at_unix_ms":1,"finished_at_unix_ms":2,"latency_us":10,"outcome":"ok"}],"stages":[],"queues":[],"inflight":[],"runtime_snapshots":[]}"#
}

#[test]
fn help_analyzer_options_works_without_run_json() {
    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("analyze")
        .arg("--help-analyzer-options")
        .output()
        .expect("cli should run");
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8(output.stdout).expect("utf8");
    assert!(stdout.contains("queueing.trigger_permille"));
    assert!(stdout.contains("value type"));
}

#[test]
fn missing_run_json_without_help_fails_clearly() {
    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("analyze")
        .output()
        .expect("cli should run");
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("utf8");
    assert!(stderr.contains("RUN_JSON"));
}

#[test]
fn analyzer_config_and_overrides_apply_with_expected_precedence() {
    let dir = tempfile::tempdir().expect("tempdir");
    let artifact_path = dir.path().join("run.json");
    std::fs::write(&artifact_path, valid_cli_artifact_with_requests()).expect("write fixture");

    let config_path = dir.path().join("analyzer.toml");
    std::fs::write(
        &config_path,
        "[analyzer]\nschema_version=1\n[analyzer.queueing]\ntrigger_permille=400\n",
    )
    .expect("write toml");

    let base = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("analyze")
        .arg(&artifact_path)
        .arg("--format")
        .arg("json")
        .output()
        .expect("run base");
    assert!(base.status.success(), "{base:?}");
    let base_stdout = String::from_utf8(base.stdout).expect("utf8");
    assert!(!base_stdout.contains("analyzer_config"));

    let with_toml = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("analyze")
        .arg(&artifact_path)
        .arg("--format")
        .arg("json")
        .arg("--analyzer-config")
        .arg(&config_path)
        .output()
        .expect("run with toml");
    assert!(with_toml.status.success(), "{with_toml:?}");
    let toml_stdout = String::from_utf8(with_toml.stdout).expect("utf8");
    assert!(toml_stdout.contains("\"path\": \"queueing.trigger_permille\""));
    assert!(toml_stdout.contains("\"value\": \"400\""));

    let with_override = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("analyze")
        .arg(&artifact_path)
        .arg("--format")
        .arg("json")
        .arg("--analyzer-set")
        .arg("queueing.trigger_permille=450")
        .output()
        .expect("run with override");
    assert!(with_override.status.success(), "{with_override:?}");
    let override_stdout = String::from_utf8(with_override.stdout).expect("utf8");
    assert!(override_stdout.contains("\"value\": \"450\""));

    let toml_plus_override = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("analyze")
        .arg(&artifact_path)
        .arg("--format")
        .arg("json")
        .arg("--analyzer-config")
        .arg(&config_path)
        .arg("--analyzer-set")
        .arg("queueing.trigger_permille=500")
        .arg("--analyzer-set")
        .arg("queueing.trigger_permille=600")
        .output()
        .expect("run with both");
    assert!(
        toml_plus_override.status.success(),
        "{toml_plus_override:?}"
    );
    let both_stdout = String::from_utf8(toml_plus_override.stdout).expect("utf8");
    assert!(both_stdout.contains("\"value\": \"600\""));
}

#[test]
fn invalid_override_errors_are_user_facing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let artifact_path = dir.path().join("run.json");
    std::fs::write(&artifact_path, valid_cli_artifact_with_requests()).expect("write fixture");

    let misspelled = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("analyze")
        .arg(&artifact_path)
        .arg("--analyzer-set")
        .arg("queuing.trigger_permille=400")
        .output()
        .expect("run misspelled");
    assert!(!misspelled.status.success());
    let misspelled_stderr = String::from_utf8(misspelled.stderr).expect("utf8");
    assert!(misspelled_stderr.contains("queueing.trigger_permille"));

    let invalid_type = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("analyze")
        .arg(&artifact_path)
        .arg("--analyzer-set")
        .arg("queueing.trigger_permille=abc")
        .output()
        .expect("run invalid type");
    assert!(!invalid_type.status.success());
    let invalid_type_stderr = String::from_utf8(invalid_type.stderr).expect("utf8");
    assert!(invalid_type_stderr.contains("expected"));
    assert!(invalid_type_stderr.contains("u64"));
}
