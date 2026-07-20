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

#[test]
#[allow(clippy::too_many_lines)]
fn canonical_run_integrity_equivalence_matrix_across_entries() {
    for case in canonical_candidates() {
        let run: Run = serde_json::from_str(case.artifact).expect(case.name);
        let reference = SemanticProjection::from_normalized(&normalize_run_permissive(&run));
        let strict = tailtriage_core::validate_run_strict(&run);
        assert_eq!(
            strict.is_ok(),
            case.strict_ok,
            "{} strict reference",
            case.name
        );

        let analyzer =
            tailtriage_analyzer::analyze_run(&run, tailtriage_analyzer::AnalyzeOptions::default());
        assert_eq!(
            analyzer.request_count,
            reference.requests.len(),
            "{} analyzer requests",
            case.name
        );
        for code in &reference.issue_codes {
            assert!(
                analyzer.warnings.iter().any(|w| w.contains(code)),
                "{} analyzer missing {code}: {:?}",
                case.name,
                analyzer.warnings
            );
        }
        let analyzer2 =
            tailtriage_analyzer::analyze_run(&run, tailtriage_analyzer::AnalyzeOptions::default());
        assert_eq!(
            analyzer.warnings, analyzer2.warnings,
            "{} analyzer warning order",
            case.name
        );

        match tailtriage_analyzer::validate_artifact_strict(&run) {
            Ok(()) => assert!(
                strict.is_ok(),
                "{} analyzer strict should match core",
                case.name
            ),
            Err(err) => {
                assert!(
                    strict.is_err(),
                    "{} analyzer strict rejected when core accepted: {err}",
                    case.name
                );
                let represented_codes = match &err {
                    tailtriage_analyzer::ArtifactValidationError::Core(core) => core
                        .report()
                        .issues
                        .iter()
                        .filter(|issue| {
                            issue.severity == tailtriage_core::RunValidationSeverity::Error
                        })
                        .map(|issue| issue.code.as_str().to_owned())
                        .collect::<BTreeSet<_>>(),
                    _ => stable_issue_codes()
                        .into_iter()
                        .filter(|code| err.to_string().contains(code))
                        .map(str::to_owned)
                        .collect::<BTreeSet<_>>(),
                };
                for code in &reference.strict_error_codes {
                    assert!(
                        represented_codes.contains(code),
                        "{} analyzer strict missing {code}: {represented_codes:?}",
                        case.name
                    );
                }
            }
        }

        let tempdir = tempfile::tempdir().expect("tempdir");
        let artifact_path = tempdir.path().join(format!("{}.json", case.name));
        std::fs::write(&artifact_path, case.artifact).expect("write artifact");
        let output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
            .arg("analyze")
            .arg(&artifact_path)
            .arg("--format")
            .arg("json")
            .output()
            .expect("cli should run");
        if reference.requests.is_empty() {
            assert!(
                !output.status.success(),
                "{} zero-request CLI analyze should fail",
                case.name
            );
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(
                stderr.contains("requests section is empty"),
                "{} command-level failure: {stderr}",
                case.name
            );
            assert!(
                !stderr.contains("JSON"),
                "{} should not be JSON decoding failure",
                case.name
            );
            assert!(
                !stderr.contains("strict artifact validation failed"),
                "{} should not be strict failure",
                case.name
            );
        } else {
            assert!(
                output.status.success(),
                "{} cli failed: {:?}",
                case.name,
                output
            );
            let cli_report: serde_json::Value =
                serde_json::from_slice(&output.stdout).expect("json report");
            assert_eq!(
                cli_report["request_count"].as_u64(),
                Some(reference.requests.len() as u64),
                "{} cli request count",
                case.name
            );
            let cli_codes = warning_codes_from_json(&cli_report);
            for code in &reference.issue_codes {
                assert!(
                    cli_codes.contains(code.as_str()),
                    "{} cli missing {code}: {cli_codes:?}",
                    case.name
                );
            }
        }

        let strict_output = Command::new(env!("CARGO_BIN_EXE_tailtriage"))
            .arg("analyze")
            .arg(&artifact_path)
            .arg("--strict-artifact")
            .output()
            .expect("strict cli should run");
        assert_eq!(
            strict_output.status.success(),
            strict.is_ok(),
            "{} cli strict status",
            case.name
        );
        if strict.is_err() {
            assert!(
                strict_output.stdout.is_empty(),
                "{} strict failure should not write report",
                case.name
            );
            let stderr = String::from_utf8_lossy(&strict_output.stderr);
            for code in &reference.strict_error_codes {
                assert!(
                    stderr.contains(code),
                    "{} strict stderr missing {code}: {stderr}",
                    case.name
                );
            }
            assert!(
                !stderr.contains("precise_interval_validation_unavailable"),
                "{} warning-only code not in strict failure heading",
                case.name
            );
        }
    }
}

#[test]
fn canonical_tracing_conversion_matches_core_for_supported_cases() {
    for case in tracing_candidates() {
        let run: Run = serde_json::from_str(case.artifact).expect(case.name);
        let reference = SemanticProjection::from_normalized(&normalize_run_permissive(&run));
        let imported = tailtriage_tracing::run_from_span_records(
            case.spans,
            tailtriage_tracing::ImportOptions::new("svc").run_id("r1"),
        )
        .expect(case.name);
        let tracing_projection = SemanticProjection::from_run_and_warnings(
            imported.run(),
            &imported
                .warnings()
                .iter()
                .map(|w| w.message().to_owned())
                .collect::<Vec<_>>(),
        );
        assert_eq!(
            tracing_projection.requests, reference.requests,
            "{} tracing requests",
            case.name
        );
        assert_eq!(
            tracing_projection.stages, reference.stages,
            "{} tracing stages",
            case.name
        );
        assert_eq!(
            tracing_projection.queues, reference.queues,
            "{} tracing queues",
            case.name
        );
        for code in &reference.issue_codes {
            assert!(
                tracing_projection.issue_codes.contains(code),
                "{} tracing missing {code}: {:?}",
                case.name,
                imported.warnings()
            );
        }

        let strict = tailtriage_core::validate_run_strict(&run);
        let strict_import = tailtriage_tracing::run_from_span_records(
            case.strict_spans,
            tailtriage_tracing::ImportOptions::new("svc")
                .run_id("r1")
                .strict(true),
        );
        assert_eq!(
            strict_import.is_ok(),
            strict.is_ok(),
            "{} tracing strict status",
            case.name
        );
        if let Err(err) = strict_import {
            let msg = err.to_string();
            for code in &reference.strict_error_codes {
                assert!(
                    msg.contains(code),
                    "{} strict tracing missing {code}: {msg}",
                    case.name
                );
            }
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct SemanticProjection {
    requests: Vec<EventProjection>,
    stages: Vec<EventProjection>,
    queues: Vec<EventProjection>,
    truncation: (u64, u64, u64),
    issue_codes: Vec<String>,
    strict_error_codes: Vec<String>,
}

type EventProjection = (String, String, u64, Option<u64>, Option<u64>);

impl SemanticProjection {
    fn from_normalized(normalized: &tailtriage_core::NormalizedRun) -> Self {
        let strict_error_codes = normalized
            .report
            .issues
            .iter()
            .filter(|i| i.severity == tailtriage_core::RunValidationSeverity::Error)
            .map(|i| i.code.as_str().to_owned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let mut out = Self::from_run_and_warnings(
            &normalized.run,
            &tailtriage_core::summarize_run_validation(normalized),
        );
        out.issue_codes = normalized
            .report
            .issues
            .iter()
            .map(|i| i.code.as_str().to_owned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        out.strict_error_codes = strict_error_codes;
        out
    }
    fn from_run_and_warnings(run: &Run, warnings: &[String]) -> Self {
        let mut requests = run
            .requests
            .iter()
            .map(|r| {
                (
                    r.request_id.clone(),
                    r.route.clone(),
                    r.latency_us,
                    r.started_at_run_us,
                    r.finished_at_run_us,
                )
            })
            .collect::<Vec<_>>();
        let mut stages = run
            .stages
            .iter()
            .map(|s| {
                (
                    s.request_id.clone(),
                    s.stage.clone(),
                    s.latency_us,
                    s.started_at_run_us,
                    s.finished_at_run_us,
                )
            })
            .collect::<Vec<_>>();
        let mut queues = run
            .queues
            .iter()
            .map(|q| {
                (
                    q.request_id.clone(),
                    q.queue.clone(),
                    q.wait_us,
                    q.waited_from_run_us,
                    q.waited_until_run_us,
                )
            })
            .collect::<Vec<_>>();
        requests.sort();
        stages.sort();
        queues.sort();
        Self {
            requests,
            stages,
            queues,
            truncation: (
                run.truncation.dropped_requests,
                run.truncation.dropped_stages,
                run.truncation.dropped_queues,
            ),
            issue_codes: stable_issue_codes()
                .into_iter()
                .filter(|c| warnings.iter().any(|w| w.contains(c)))
                .map(str::to_owned)
                .collect(),
            strict_error_codes: Vec::new(),
        }
    }
}

struct CanonicalCase {
    name: &'static str,
    artifact: &'static str,
    strict_ok: bool,
}
fn canonical_candidates() -> [CanonicalCase; 7] {
    [
        CanonicalCase {
            name: "valid-precise",
            artifact: valid_precise(),
            strict_ok: true,
        },
        CanonicalCase {
            name: "missing-precision",
            artifact: missing_precision(),
            strict_ok: true,
        },
        CanonicalCase {
            name: "duplicate-ambiguous-child",
            artifact: duplicate_ambiguous_child(),
            strict_ok: false,
        },
        CanonicalCase {
            name: "orphan-child",
            artifact: valid_request_plus_orphan_stage(),
            strict_ok: false,
        },
        CanonicalCase {
            name: "excluded-parent-child",
            artifact: excluded_parent_child(),
            strict_ok: false,
        },
        CanonicalCase {
            name: "invalid-optional-precision",
            artifact: request_with_partial_optional_precision(),
            strict_ok: false,
        },
        CanonicalCase {
            name: "outside-child",
            artifact: precise_child_outside_parent(),
            strict_ok: false,
        },
    ]
}

struct TracingCase {
    name: &'static str,
    artifact: &'static str,
    spans: Vec<tailtriage_tracing::SpanRecord>,
    strict_spans: Vec<tailtriage_tracing::SpanRecord>,
}
fn tracing_candidates() -> Vec<TracingCase> {
    canonical_candidates()
        .into_iter()
        .filter(|c| c.name != "excluded-parent-child")
        .map(|c| {
            let spans = spans_for(c.name);
            TracingCase {
                name: c.name,
                artifact: c.artifact,
                strict_spans: spans.clone(),
                spans,
            }
        })
        .collect()
}

fn req(id: &str, start: u64, end: u64, dur: u64) -> tailtriage_tracing::SpanRecord {
    tailtriage_tracing::SpanRecord::new("req", 1, 2)
        .field(tailtriage_tracing::TT_KIND, "request")
        .field(tailtriage_tracing::TT_REQUEST_ID, id)
        .field(tailtriage_tracing::TT_ROUTE, "/")
        .field(tailtriage_tracing::TT_OUTCOME, "ok")
        .started_at_run_us(start)
        .finished_at_run_us(end)
        .duration_us(dur)
}
fn req_no_precision(id: &str, dur: u64) -> tailtriage_tracing::SpanRecord {
    tailtriage_tracing::SpanRecord::new("req", 1, 2)
        .field(tailtriage_tracing::TT_KIND, "request")
        .field(tailtriage_tracing::TT_REQUEST_ID, id)
        .field(tailtriage_tracing::TT_ROUTE, "/")
        .field(tailtriage_tracing::TT_OUTCOME, "ok")
        .duration_us(dur)
}
fn stage(id: &str, start: u64, end: u64, dur: u64) -> tailtriage_tracing::SpanRecord {
    tailtriage_tracing::SpanRecord::new("stage", 1, 2)
        .field(tailtriage_tracing::TT_KIND, "stage")
        .field(tailtriage_tracing::TT_REQUEST_ID, id)
        .field(tailtriage_tracing::TT_STAGE, "db")
        .field(tailtriage_tracing::TT_SUCCESS, true)
        .started_at_run_us(start)
        .finished_at_run_us(end)
        .duration_us(dur)
}
fn queue(id: &str, start: u64, end: u64, dur: u64) -> tailtriage_tracing::SpanRecord {
    tailtriage_tracing::SpanRecord::new("queue", 1, 2)
        .field(tailtriage_tracing::TT_KIND, "queue")
        .field(tailtriage_tracing::TT_REQUEST_ID, id)
        .field(tailtriage_tracing::TT_QUEUE, "pool")
        .started_at_run_us(start)
        .finished_at_run_us(end)
        .duration_us(dur)
}
fn spans_for(name: &str) -> Vec<tailtriage_tracing::SpanRecord> {
    match name {
        "valid-precise" => vec![
            req("req1", 0, 10, 10),
            stage("req1", 2, 5, 3),
            queue("req1", 5, 7, 2),
        ],
        "missing-precision" => vec![
            req_no_precision("req1", 10),
            tailtriage_tracing::SpanRecord::new("stage", 1, 2)
                .field(tailtriage_tracing::TT_KIND, "stage")
                .field(tailtriage_tracing::TT_REQUEST_ID, "req1")
                .field(tailtriage_tracing::TT_STAGE, "db")
                .field(tailtriage_tracing::TT_SUCCESS, true)
                .duration_us(3),
            tailtriage_tracing::SpanRecord::new("queue", 1, 2)
                .field(tailtriage_tracing::TT_KIND, "queue")
                .field(tailtriage_tracing::TT_REQUEST_ID, "req1")
                .field(tailtriage_tracing::TT_QUEUE, "pool")
                .duration_us(2),
        ],
        "duplicate-ambiguous-child" => vec![
            req("dup", 0, 10, 10),
            req("dup", 20, 30, 10),
            stage("dup", 2, 5, 3),
        ],
        "orphan-child" => vec![req("req1", 1, 11, 10), stage("missing", 1, 2, 1)],
        "invalid-optional-precision" => vec![tailtriage_tracing::SpanRecord::new("req", 1, 2)
            .field(tailtriage_tracing::TT_KIND, "request")
            .field(tailtriage_tracing::TT_REQUEST_ID, "req1")
            .field(tailtriage_tracing::TT_ROUTE, "/")
            .field(tailtriage_tracing::TT_OUTCOME, "ok")
            .started_at_run_us(1)
            .duration_us(10)],
        "outside-child" => vec![req("req1", 10, 20, 10), stage("req1", 0, 5, 5)],
        _ => unreachable!(),
    }
}

fn valid_precise() -> &'static str {
    r#"{"schema_version":1,"metadata":{"run_id":"r1","service_name":"svc","service_version":null,"started_at_unix_ms":1,"finished_at_unix_ms":2,"mode":"light","host":null,"pid":null,"lifecycle_warnings":[],"unfinished_requests":{"count":0,"sample":[]}},"requests":[{"request_id":"req1","route":"/","kind":null,"started_at_unix_ms":1,"started_at_run_us":0,"finished_at_unix_ms":2,"finished_at_run_us":10,"latency_us":10,"outcome":"ok"}],"stages":[{"request_id":"req1","stage":"db","started_at_unix_ms":1,"started_at_run_us":2,"finished_at_unix_ms":2,"finished_at_run_us":5,"latency_us":3,"success":true}],"queues":[{"request_id":"req1","queue":"pool","waited_from_unix_ms":1,"waited_from_run_us":5,"waited_until_unix_ms":2,"waited_until_run_us":7,"wait_us":2}],"inflight":[],"runtime_snapshots":[]}"#
}
fn missing_precision() -> &'static str {
    r#"{"schema_version":1,"metadata":{"run_id":"r1","service_name":"svc","service_version":null,"started_at_unix_ms":1,"finished_at_unix_ms":2,"mode":"light","host":null,"pid":null,"lifecycle_warnings":["existing lifecycle warning"],"unfinished_requests":{"count":0,"sample":[]}},"requests":[{"request_id":"req1","route":"/","kind":null,"started_at_unix_ms":1,"finished_at_unix_ms":2,"latency_us":10,"outcome":"ok"}],"stages":[{"request_id":"req1","stage":"db","started_at_unix_ms":1,"finished_at_unix_ms":2,"latency_us":3,"success":true}],"queues":[{"request_id":"req1","queue":"pool","waited_from_unix_ms":1,"waited_until_unix_ms":2,"wait_us":2}],"inflight":[],"runtime_snapshots":[]}"#
}
fn duplicate_ambiguous_child() -> &'static str {
    r#"{"schema_version":1,"metadata":{"run_id":"r1","service_name":"svc","service_version":null,"started_at_unix_ms":1,"finished_at_unix_ms":2,"mode":"light","host":null,"pid":null,"lifecycle_warnings":[],"unfinished_requests":{"count":0,"sample":[]}},"requests":[{"request_id":"dup","route":"/","kind":null,"started_at_unix_ms":1,"started_at_run_us":0,"finished_at_unix_ms":2,"finished_at_run_us":10,"latency_us":10,"outcome":"ok"},{"request_id":"dup","route":"/","kind":null,"started_at_unix_ms":1,"started_at_run_us":20,"finished_at_unix_ms":2,"finished_at_run_us":30,"latency_us":10,"outcome":"ok"}],"stages":[{"request_id":"dup","stage":"db","started_at_unix_ms":1,"started_at_run_us":2,"finished_at_unix_ms":2,"finished_at_run_us":5,"latency_us":3,"success":true}],"queues":[],"inflight":[],"runtime_snapshots":[]}"#
}
fn excluded_parent_child() -> &'static str {
    r#"{"schema_version":1,"metadata":{"run_id":"r1","service_name":"svc","service_version":null,"started_at_unix_ms":1,"finished_at_unix_ms":2,"mode":"light","host":null,"pid":null,"lifecycle_warnings":[],"unfinished_requests":{"count":0,"sample":[]}},"requests":[{"request_id":"bad","route":"","kind":null,"started_at_unix_ms":1,"started_at_run_us":0,"finished_at_unix_ms":2,"finished_at_run_us":10,"latency_us":10,"outcome":"ok"}],"stages":[{"request_id":"bad","stage":"db","started_at_unix_ms":1,"started_at_run_us":2,"finished_at_unix_ms":2,"finished_at_run_us":5,"latency_us":3,"success":true}],"queues":[],"inflight":[],"runtime_snapshots":[]}"#
}
