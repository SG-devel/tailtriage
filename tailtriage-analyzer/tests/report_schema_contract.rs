use std::path::Path;

use serde_json::Value;
use tailtriage_analyzer::{
    analyze_run, render_json, AnalyzeOptions, DiagnosisKind, RelatedEvidenceBasis,
    RelatedEvidenceGroup, RelatedEvidenceMeasurement, RelatedEvidenceMember,
};
use tailtriage_core::{Run, StageRelation};

fn load_fixture(name: &str) -> Run {
    let path = Path::new("tests/fixtures").join(name);
    let content = std::fs::read_to_string(path).expect("fixture should exist");
    serde_json::from_str(&content).expect("fixture should deserialize")
}

fn json_path_exists<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    path.iter()
        .try_fold(value, |current, key| current.get(*key))
}

// TT-TEST: A13 primary
#[test]
fn documented_report_keys_exist_in_json_output() {
    let run = load_fixture("queue_saturation.json");
    let report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    let json = serde_json::to_value(&report).expect("report should serialize");

    // Keep this contract aligned with the keys called out in README's JSON-output section.
    for path in [
        ["primary_suspect", "kind"].as_slice(),
        ["p95_queue_share_permille"].as_slice(),
        ["p95_service_share_permille"].as_slice(),
        ["evidence_quality"].as_slice(),
        ["primary_suspect", "confidence_notes"].as_slice(),
        ["route_breakdowns"].as_slice(),
        ["temporal_segments"].as_slice(),
        ["primary_suspect", "evidence"].as_slice(),
    ] {
        assert!(
            json_path_exists(&json, path).is_some(),
            "expected documented JSON path {path:?}",
        );
    }

    assert!(
        json_path_exists(&json, &["evidence_quality"]).is_some_and(Value::is_object),
        "evidence_quality should be an object"
    );
    assert!(
        json_path_exists(&json, &["primary_suspect", "confidence_notes"])
            .is_some_and(Value::is_array),
        "primary_suspect.confidence_notes should be an array"
    );
    assert!(
        json_path_exists(&json, &["route_breakdowns"]).is_some_and(Value::is_array),
        "route_breakdowns should be an array"
    );
    assert!(
        json_path_exists(&json, &["temporal_segments"]).is_some_and(Value::is_array),
        "temporal_segments should be an array"
    );
    assert_eq!(report.related_groups, Vec::new());
    assert!(
        json.get("related_groups").is_none(),
        "ordinary reports must preserve the versionless JSON shape"
    );

    let evidence = json_path_exists(&json, &["primary_suspect", "evidence"])
        .and_then(Value::as_array)
        .expect("primary_suspect.evidence should be an array");
    assert!(!evidence.is_empty(), "evidence array should not be empty");
    assert!(
        evidence.iter().all(Value::is_string),
        "primary_suspect.evidence should contain strings"
    );
}

// TT-TEST: A13 primary
#[test]
fn nonempty_related_group_has_the_selected_explicit_schema() {
    let mut report = analyze_run(
        &load_fixture("queue_saturation.json"),
        AnalyzeOptions::default(),
    )
    .expect("analyzer options should be valid");
    report.related_groups = vec![RelatedEvidenceGroup {
        relation: StageRelation::BlockingPool,
        representative: DiagnosisKind::BlockingPoolPressure,
        members: vec![
            RelatedEvidenceMember {
                diagnosis: DiagnosisKind::BlockingPoolPressure,
                stage: None,
                evidence_basis: RelatedEvidenceBasis::Completed,
                relevant_support: 40,
                measurement: RelatedEvidenceMeasurement::BlockingPool {
                    usable_snapshots: 40,
                    p95_depth: 12,
                    peak_depth: 20,
                    nonzero_share_permille: 700,
                },
            },
            RelatedEvidenceMember {
                diagnosis: DiagnosisKind::DownstreamStageDominance,
                stage: Some("blocking_lookup".to_owned()),
                evidence_basis: RelatedEvidenceBasis::ObservedLowerBound,
                relevant_support: 7,
                measurement: RelatedEvidenceMeasurement::DownstreamStage {
                    tail_contribution_permille: 650,
                    cumulative_contribution_permille: 420,
                },
            },
        ],
    }];

    let value: Value =
        serde_json::from_str(&render_json(&report).expect("report should serialize"))
            .expect("report JSON should parse");
    assert_eq!(
        value["related_groups"],
        serde_json::json!([{
            "relation": "blocking_pool",
            "representative": "blocking_pool_pressure",
            "members": [
                {
                    "diagnosis": "blocking_pool_pressure",
                    "evidence_basis": "completed",
                    "relevant_support": 40,
                    "measurement": {
                        "kind": "blocking_pool",
                        "usable_snapshots": 40,
                        "p95_depth": 12,
                        "peak_depth": 20,
                        "nonzero_share_permille": 700
                    }
                },
                {
                    "diagnosis": "downstream_stage_dominance",
                    "stage": "blocking_lookup",
                    "evidence_basis": "observed_lower_bound",
                    "relevant_support": 7,
                    "measurement": {
                        "kind": "downstream_stage",
                        "tail_contribution_permille": 650,
                        "cumulative_contribution_permille": 420
                    }
                }
            ]
        }])
    );
}
