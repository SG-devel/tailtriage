use std::path::Path;

use serde_json::Value;

fn load_demo_analysis(path: &str) -> Value {
    let path = Path::new("..").join(path);
    let content = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed reading fixture {}: {err}", path.display()));
    serde_json::from_str(&content)
        .unwrap_or_else(|err| panic!("failed parsing fixture {}: {err}", path.display()))
}

#[test]
fn queue_demo_fixture_reports_application_queue_saturation() {
    let report = load_demo_analysis("demos/queue_service/fixtures/sample-analysis.json");

    assert_eq!(
        report["primary_suspect"]["kind"],
        Value::String("ApplicationQueueSaturation".to_string())
    );
    assert!(
        report["primary_suspect"]["score"]
            .as_u64()
            .unwrap_or_default()
            >= 70,
        "queue demo should strongly prioritize queue saturation"
    );
}

#[test]
fn blocking_demo_fixture_reports_blocking_pool_pressure() {
    let report = load_demo_analysis("demos/blocking_service/fixtures/sample-analysis.json");

    assert_eq!(
        report["primary_suspect"]["kind"],
        Value::String("BlockingPoolPressure".to_string())
    );
    assert!(
        report["primary_suspect"]["score"]
            .as_u64()
            .unwrap_or_default()
            >= 70,
        "blocking demo should strongly prioritize blocking pressure"
    );
}
