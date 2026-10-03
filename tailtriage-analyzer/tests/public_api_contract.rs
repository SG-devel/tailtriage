use std::path::Path;

use serde_json::Value;
use tailtriage_analyzer::{
    analyze_run, render_json, render_json_pretty, render_text, AnalyzeOptions, DiagnosisKind,
    RelatedEvidenceBasis, RelatedEvidenceGroup, RelatedEvidenceMeasurement, RelatedEvidenceMember,
    Report,
};
use tailtriage_core::{validate_run_strict, Run, StageRelation};

fn load_fixture(name: &str) -> Run {
    let path = Path::new("tests/fixtures").join(name);
    let content = std::fs::read_to_string(path).expect("fixture should exist");
    serde_json::from_str(&content).expect("fixture should deserialize")
}

// TT-TEST: A12 primary
#[test]
fn related_group_is_equivalent_across_public_renderers() {
    let mut report = analyze_run(
        &load_fixture("queue_saturation.json"),
        AnalyzeOptions::default(),
    )
    .expect("analyzer options should be valid");
    let ordinary_text = render_text(&report);
    assert!(!ordinary_text.contains("Related evidence groups:"));
    report.related_groups.push(RelatedEvidenceGroup {
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
                stage: Some("lookup\nworker".to_owned()),
                evidence_basis: RelatedEvidenceBasis::ObservedLowerBound,
                relevant_support: 7,
                measurement: RelatedEvidenceMeasurement::DownstreamStage {
                    tail_contribution_permille: 650,
                    cumulative_contribution_permille: 420,
                },
            },
        ],
    });

    let compact = render_json(&report).expect("compact JSON should render");
    let pretty = render_json_pretty(&report).expect("pretty JSON should render");
    assert_eq!(
        serde_json::from_str::<Value>(&compact).expect("compact JSON should parse"),
        serde_json::from_str::<Value>(&pretty).expect("pretty JSON should parse")
    );
    assert_eq!(
        compact,
        render_json(&report).expect("JSON is deterministic")
    );

    let text = render_text(&report);
    assert!(text.contains("Related evidence groups:"));
    assert!(text.contains("representative: blocking pool pressure; relation: blocking_pool"));
    assert!(text.contains("diagnosis: downstream stage dominance"));
    assert!(text.contains("diagnosis: blocking pool pressure"));
    assert!(text.contains("evidence_basis: completed; relevant_support: 40"));
    assert!(text.contains("usable_snapshots 40"));
    assert!(text.contains("p95_depth 12"));
    assert!(text.contains("peak_depth 20"));
    assert!(text.contains("nonzero_share_permille 700"));
    assert!(text.contains("stage: lookup\\nworker"));
    assert!(text.contains("evidence_basis: observed_lower_bound; relevant_support: 7"));
    assert!(text.contains("tail_contribution_permille 650"));
    assert!(text.contains("cumulative_contribution_permille 420"));
}

// TT-TEST: A11 secondary
// TT-TEST: A12 secondary
#[test]
fn public_api_supports_checked_analysis_and_canonical_renderers() {
    let run = load_fixture("queue_saturation.json");

    validate_run_strict(&run).expect("fixture should pass explicit strict validation");
    let report: Report =
        analyze_run(&run, AnalyzeOptions::default()).expect("analyzer options should be valid");
    let text = render_text(&report);
    let compact = render_json(&report).expect("report should render as compact JSON");
    let pretty = render_json_pretty(&report).expect("report should render as pretty JSON");
    let json_value: Value = serde_json::from_str(&pretty).expect("json should parse");

    assert!(text.contains("Primary suspect:"));
    assert_eq!(
        serde_json::from_str::<Value>(&compact).expect("compact JSON should parse"),
        json_value
    );
    for path in [
        "/evidence_quality",
        "/primary_suspect/confidence_notes",
        "/route_breakdowns",
        "/temporal_segments",
    ] {
        assert!(
            json_value.pointer(path).is_some(),
            "expected JSON path {path}"
        );
    }
}
