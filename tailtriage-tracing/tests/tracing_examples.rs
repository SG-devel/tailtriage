use std::path::PathBuf;

use tailtriage_tracing::{import_jsonl_path, ImportOptions};

#[test]
fn tracing_fixture_jsonl_imports_expected_shapes() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("tracing_spans.jsonl");
    let imported = import_jsonl_path(
        fixture,
        ImportOptions::new("checkout-service")
            .service_version("example")
            .run_id("fixture-example")
            .strict(true),
    )
    .expect("fixture should parse as normalized completed-span jsonl");

    let run = imported.run();
    assert_eq!(run.requests.len(), 1);
    assert_eq!(run.stages.len(), 1);
    assert_eq!(run.queues.len(), 1);
    assert!(imported.warnings().is_empty());

    assert_eq!(run.requests[0].request_id, "req-42");
    assert_eq!(run.requests[0].route, "/checkout");
    assert_eq!(run.requests[0].outcome, "ok");
    assert_eq!(run.queues[0].queue, "db_pool");
    assert_eq!(run.queues[0].depth_at_start, Some(4));
    assert_eq!(run.stages[0].stage, "db");
    assert!(run.stages[0].success);
}
