use std::process::Command;

use std::collections::BTreeSet;

use tailtriage_core::{normalize_run_permissive, RequestOptions, Run, Tailtriage};

#[test]
fn cli_json_matches_analyzer_renderer_output() {
    let tempdir = tempfile::tempdir().expect("tempdir should build");
    let artifact_path = tempdir.path().join("run.json");

    let tailtriage = Tailtriage::builder("checkout-service")
        .output(&artifact_path)
        .build()
        .expect("tailtriage should build");

    let started = tailtriage.begin_request_with(
        "/checkout",
        RequestOptions::new().request_id("req-1").kind("http"),
    );
    started.completion.finish_ok();

    tailtriage.shutdown().expect("shutdown should succeed");

    let loaded = tailtriage_cli::artifact::load_run_artifact(&artifact_path)
        .expect("artifact should load successfully");
    assert!(loaded.warnings.is_empty());

    let report = tailtriage_analyzer::analyze_run(
        &loaded.run,
        tailtriage_analyzer::AnalyzeOptions::default(),
    );
    let expected_json = tailtriage_analyzer::render_json_pretty(&report)
        .expect("expected report JSON should render");

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

#[test]
fn permissive_cli_reports_preserve_core_warning_equivalence_for_boundary_artifacts() {
    for candidate in [
        Candidate {
            name: "orphan-stage",
            artifact: valid_request_plus_orphan_stage(),
            expected_code: "orphan_request_scoped_event",
            expected_stages_after_normalization: Some(0),
        },
        Candidate {
            name: "partial-precision",
            artifact: request_with_partial_optional_precision(),
            expected_code: "partial_run_relative_interval",
            expected_stages_after_normalization: None,
        },
        Candidate {
            name: "outside-child",
            artifact: precise_child_outside_parent(),
            expected_code: "child_interval_outside_request",
            expected_stages_after_normalization: Some(0),
        },
    ] {
        let original: Run =
            serde_json::from_str(candidate.artifact).expect("candidate should decode");
        let normalized = normalize_run_permissive(&original);
        let analyzer_report = tailtriage_analyzer::analyze_run(
            &original,
            tailtriage_analyzer::AnalyzeOptions::default(),
        );

        let tempdir = tempfile::tempdir().expect("tempdir should build");
        let artifact_path = tempdir.path().join(format!("{}.json", candidate.name));
        std::fs::write(&artifact_path, candidate.artifact).expect("artifact should write");
        let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
            .arg("analyze")
            .arg(&artifact_path)
            .arg("--format")
            .arg("json")
            .output()
            .expect("cli should run");
        assert!(output.status.success(), "cli failed: {output:?}");
        let cli_report: serde_json::Value =
            serde_json::from_slice(&output.stdout).expect("cli stdout should be report JSON");

        assert_eq!(
            cli_report["request_count"].as_u64(),
            Some(normalized.run.requests.len() as u64),
            "{} retained request count should match",
            candidate.name
        );
        assert_eq!(
            analyzer_report.request_count,
            normalized.run.requests.len(),
            "{} analyzer retained request count should match",
            candidate.name
        );
        if let Some(expected_stages) = candidate.expected_stages_after_normalization {
            assert_eq!(
                normalized.run.stages.len(),
                expected_stages,
                "{} normalized stage retention should match expected exclusion",
                candidate.name
            );
        }

        let core_codes = normalized
            .report
            .issues
            .iter()
            .map(|issue| issue.code.as_str())
            .collect::<BTreeSet<_>>();
        let analyzer_codes = warning_codes(&analyzer_report.warnings);
        let cli_codes = warning_codes_from_json(&cli_report);
        assert!(
            core_codes.contains(candidate.expected_code),
            "{} core should report expected code",
            candidate.name
        );
        assert!(
            analyzer_codes.contains(candidate.expected_code),
            "{} analyzer should report expected code",
            candidate.name
        );
        assert!(
            cli_codes.contains(candidate.expected_code),
            "{} CLI should not lose expected core warning",
            candidate.name
        );
        assert!(
            core_codes.iter().all(|code| cli_codes.contains(code)),
            "{} CLI warning codes should include all core issue codes: core={core_codes:?}, cli={cli_codes:?}",
            candidate.name
        );
    }
}

struct Candidate {
    name: &'static str,
    artifact: &'static str,
    expected_code: &'static str,
    expected_stages_after_normalization: Option<usize>,
}

fn warning_codes(warnings: &[String]) -> BTreeSet<&'static str> {
    stable_issue_codes()
        .into_iter()
        .filter(|code| warnings.iter().any(|warning| warning.contains(code)))
        .collect()
}

fn warning_codes_from_json(report: &serde_json::Value) -> BTreeSet<&'static str> {
    let warnings = report["warnings"]
        .as_array()
        .expect("warnings should be array")
        .iter()
        .map(|value| value.as_str().expect("warning should be string"))
        .collect::<Vec<_>>();
    stable_issue_codes()
        .into_iter()
        .filter(|code| warnings.iter().any(|warning| warning.contains(code)))
        .collect()
}

fn stable_issue_codes() -> [&'static str; 11] {
    [
        "unsupported_schema_version",
        "empty_required_field",
        "inverted_interval",
        "partial_run_relative_interval",
        "duration_mismatch",
        "duplicate_completed_request_id",
        "ambiguous_parent_request_id",
        "orphan_request_scoped_event",
        "parent_request_excluded",
        "child_interval_outside_request",
        "precise_interval_validation_unavailable",
    ]
}

fn valid_request_plus_orphan_stage() -> &'static str {
    r#"{"schema_version":1,"metadata":{"run_id":"r1","service_name":"svc","service_version":null,"started_at_unix_ms":1,"finished_at_unix_ms":2,"mode":"light","host":null,"pid":null,"lifecycle_warnings":[],"unfinished_requests":{"count":0,"sample":[]}},"requests":[{"request_id":"req1","route":"/","kind":null,"started_at_unix_ms":1,"started_at_run_us":1,"finished_at_unix_ms":2,"finished_at_run_us":11,"latency_us":10,"outcome":"ok"}],"stages":[{"request_id":"missing","stage":"db","started_at_unix_ms":1,"started_at_run_us":1,"finished_at_unix_ms":2,"finished_at_run_us":2,"latency_us":1,"success":true}],"queues":[],"inflight":[],"runtime_snapshots":[]}"#
}

fn request_with_partial_optional_precision() -> &'static str {
    r#"{"schema_version":1,"metadata":{"run_id":"r1","service_name":"svc","service_version":null,"started_at_unix_ms":1,"finished_at_unix_ms":2,"mode":"light","host":null,"pid":null,"lifecycle_warnings":[],"unfinished_requests":{"count":0,"sample":[]}},"requests":[{"request_id":"req1","route":"/","kind":null,"started_at_unix_ms":1,"started_at_run_us":1,"finished_at_unix_ms":2,"latency_us":10,"outcome":"ok"}],"stages":[],"queues":[],"inflight":[],"runtime_snapshots":[]}"#
}

fn precise_child_outside_parent() -> &'static str {
    r#"{"schema_version":1,"metadata":{"run_id":"r1","service_name":"svc","service_version":null,"started_at_unix_ms":1,"finished_at_unix_ms":2,"mode":"light","host":null,"pid":null,"lifecycle_warnings":[],"unfinished_requests":{"count":0,"sample":[]}},"requests":[{"request_id":"req1","route":"/","kind":null,"started_at_unix_ms":1,"started_at_run_us":10,"finished_at_unix_ms":2,"finished_at_run_us":20,"latency_us":10,"outcome":"ok"}],"stages":[{"request_id":"req1","stage":"db","started_at_unix_ms":1,"started_at_run_us":0,"finished_at_unix_ms":2,"finished_at_run_us":5,"latency_us":5,"success":true}],"queues":[],"inflight":[],"runtime_snapshots":[]}"#
}
