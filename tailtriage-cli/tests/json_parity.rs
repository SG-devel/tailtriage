use std::process::Command;

use tailtriage_cli::artifact::load_run_artifact;
use tailtriage_core::{RequestOptions, Tailtriage};

#[test]
fn cli_json_matches_analyzer_pretty_json() {
    let tempdir = tempfile::tempdir().expect("tempdir should build");
    let artifact_path = tempdir.path().join("run.json");

    let run = Tailtriage::builder("checkout-service")
        .output(&artifact_path)
        .build()
        .expect("tailtriage run should build");

    let started = run.begin_request_with(
        "/checkout",
        RequestOptions::new().request_id("req-1").kind("http"),
    );
    started.completion.finish_ok();

    run.shutdown().expect("shutdown should succeed");

    let loaded = load_run_artifact(&artifact_path).expect("artifact should load");
    assert!(
        loaded.warnings.is_empty(),
        "fixture should produce no loader warnings"
    );

    let report = tailtriage_analyzer::analyze_run(
        &loaded.run,
        tailtriage_analyzer::AnalyzeOptions::default(),
    );
    let expected_json = tailtriage_analyzer::render_json_pretty(&report)
        .expect("expected report JSON should render");

    let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
        .args([
            "analyze",
            artifact_path
                .to_str()
                .expect("artifact path should be valid UTF-8"),
            "--format",
            "json",
        ])
        .output()
        .expect("CLI should execute");

    assert!(output.status.success(), "CLI should exit successfully");

    let stdout = std::str::from_utf8(&output.stdout).expect("stdout should be valid UTF-8");
    let stderr = std::str::from_utf8(&output.stderr).expect("stderr should be valid UTF-8");

    assert_eq!(stderr, "");
    assert_eq!(stdout, format!("{expected_json}\n"));
}
