use std::process::Command;

use tailtriage_analyzer::{analyze_run, render_json_pretty, AnalyzeOptions};
use tailtriage_core::{RequestOptions, Tailtriage};

#[test]
fn cli_json_matches_analyzer_renderer() {
    let tempdir = tempfile::tempdir().expect("tempdir should build");
    let artifact_path = tempdir.path().join("run.json");

    let run = Tailtriage::builder("checkout-service")
        .output(&artifact_path)
        .build()
        .expect("tailtriage should build");

    let started = run.begin_request_with(
        "/checkout",
        RequestOptions::new().request_id("req-1").kind("http"),
    );
    started.completion.finish_ok();
    run.shutdown().expect("shutdown should write artifact");

    let loaded = tailtriage_cli::artifact::load_run_artifact(&artifact_path)
        .expect("cli loader should load generated artifact");
    assert!(
        loaded.warnings.is_empty(),
        "expected generated fixture to have no loader warnings"
    );

    let report = analyze_run(&loaded.run, AnalyzeOptions::default());
    let expected_json = render_json_pretty(&report).expect("expected report JSON should render");

    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .arg("analyze")
        .arg(&artifact_path)
        .arg("--format")
        .arg("json")
        .output()
        .expect("cli should run");

    assert!(output.status.success(), "cli failed: {output:?}");

    let stdout = std::str::from_utf8(&output.stdout).expect("stdout should be utf8");
    let stderr = std::str::from_utf8(&output.stderr).expect("stderr should be utf8");

    assert_eq!(stderr, "");
    assert_eq!(stdout, format!("{expected_json}\n"));
}
