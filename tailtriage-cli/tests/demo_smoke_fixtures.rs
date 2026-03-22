use std::path::Path;

use serde_json::Value;

fn load_demo_analysis(path: &str) -> Value {
    let path = Path::new("..").join(path);
    let content = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed reading fixture {}: {err}", path.display()));
    serde_json::from_str(&content)
        .unwrap_or_else(|err| panic!("failed parsing fixture {}: {err}", path.display()))
}

fn assert_primary_kind_in_allowed_set(report: &Value, allowed_kinds: &[&str], fixture: &str) {
    let kind = report["primary_suspect"]["kind"].as_str().unwrap_or("");
    assert!(
        allowed_kinds.contains(&kind),
        "fixture {fixture} expected primary suspect kind in {allowed_kinds:?}, got {kind}"
    );
}

fn assert_primary_score_floor(report: &Value, min_score: u64, fixture: &str) {
    let score = report["primary_suspect"]["score"]
        .as_u64()
        .unwrap_or_default();
    assert!(
        score >= min_score,
        "fixture {fixture} expected primary suspect score >= {min_score}, got {score}"
    );
}

fn assert_primary_evidence_contains_any(report: &Value, cues: &[&str], fixture: &str) {
    let evidence = report["primary_suspect"]["evidence"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let evidence_text: Vec<&str> = evidence.iter().filter_map(|item| item.as_str()).collect();
    assert!(
        evidence_text
            .iter()
            .any(|line| cues.iter().any(|cue| line.contains(cue))),
        "fixture {fixture} expected evidence to contain one of {cues:?}, got {evidence_text:?}"
    );
}

#[test]
fn queue_demo_fixture_reports_application_queue_saturation() {
    let fixture = "demos/queue_service/fixtures/sample-analysis.json";
    let report = load_demo_analysis(fixture);

    assert_primary_kind_in_allowed_set(&report, &["application_queue_saturation"], fixture);
    assert_primary_score_floor(&report, 70, fixture);
}

#[test]
fn blocking_demo_fixture_reports_blocking_pool_pressure() {
    let fixture = "demos/blocking_service/fixtures/sample-analysis.json";
    let report = load_demo_analysis(fixture);

    assert_primary_kind_in_allowed_set(&report, &["blocking_pool_pressure"], fixture);
    assert_primary_score_floor(&report, 70, fixture);
}

#[test]
fn downstream_demo_fixture_reports_downstream_stage_dominance() {
    let fixture = "demos/downstream_service/fixtures/sample-analysis.json";
    let report = load_demo_analysis(fixture);

    assert_primary_kind_in_allowed_set(&report, &["downstream_stage_dominates"], fixture);
    assert_primary_score_floor(&report, 60, fixture);
}

#[test]
fn executor_demo_fixture_reports_executor_pressure() {
    let fixture = "demos/executor_pressure_service/fixtures/sample-analysis.json";
    let report = load_demo_analysis(fixture);

    assert_primary_kind_in_allowed_set(&report, &["executor_pressure_suspected"], fixture);
    assert_primary_score_floor(&report, 60, fixture);
}

#[test]
fn mixed_contention_baseline_fixture_has_queue_primary_with_secondary_contention_cues() {
    let fixture = "demos/mixed_contention_service/fixtures/baseline-analysis.json";
    let report = load_demo_analysis(fixture);

    assert_primary_kind_in_allowed_set(
        &report,
        &[
            "application_queue_saturation",
            "executor_pressure_suspected",
        ],
        fixture,
    );
    assert_primary_evidence_contains_any(
        &report,
        &["Queue wait", "queue depth", "In-flight gauge"],
        fixture,
    );
    assert_primary_score_floor(&report, 70, fixture);
}

#[test]
fn cold_start_burst_before_fixture_has_cold_start_queue_evidence() {
    let fixture = "demos/cold_start_burst_service/fixtures/before-analysis.json";
    let report = load_demo_analysis(fixture);

    assert_primary_kind_in_allowed_set(
        &report,
        &[
            "application_queue_saturation",
            "executor_pressure_suspected",
        ],
        fixture,
    );
    assert_primary_evidence_contains_any(
        &report,
        &[
            "Queue wait",
            "queue depth",
            "In-flight gauge",
            "cold_start_burst_inflight",
        ],
        fixture,
    );
    assert_primary_score_floor(&report, 70, fixture);
}

#[test]
fn db_pool_saturation_before_fixture_preserves_queue_signal_floor() {
    let fixture = "demos/db_pool_saturation_service/fixtures/before-analysis.json";
    let report = load_demo_analysis(fixture);

    assert_primary_kind_in_allowed_set(
        &report,
        &["application_queue_saturation", "downstream_stage_dominates"],
        fixture,
    );
    assert_primary_evidence_contains_any(&report, &["Queue wait", "queue depth"], fixture);
    assert_primary_score_floor(&report, 65, fixture);
}

#[test]
fn retry_storm_before_fixture_preserves_downstream_retry_cues() {
    let fixture = "demos/retry_storm_service/fixtures/before-analysis.json";
    let report = load_demo_analysis(fixture);

    assert_primary_kind_in_allowed_set(
        &report,
        &["downstream_stage_dominates", "application_queue_saturation"],
        fixture,
    );
    assert_primary_evidence_contains_any(
        &report,
        &["downstream_total", "downstream", "retry"],
        fixture,
    );
    assert_primary_score_floor(&report, 60, fixture);
}
