use std::{fs::File, path::PathBuf};

use tailtriage_analyzer::{analyze_run, AnalyzeOptions};
use tailtriage_tracing::{import_jsonl_path, import_jsonl_reader, ImportOptions};

#[test]
fn jsonl_fixture_imports_completed_span_shape() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("tracing_spans.jsonl");

    let imported = import_jsonl_path(
        &fixture,
        ImportOptions::new("checkout-service")
            .service_version("example")
            .run_id("fixture-example")
            .strict(true),
    )
    .expect("fixture should import");

    let run = imported.run();
    assert_eq!(run.requests.len(), 1);
    assert_eq!(run.queues.len(), 1);
    assert_eq!(run.stages.len(), 1);
    assert_eq!(run.requests[0].request_id, "req-42");
    assert_eq!(run.requests[0].route, "/checkout");
    assert_eq!(run.requests[0].outcome, "ok");
    assert_eq!(run.queues[0].queue, "db-pool");
    assert_eq!(run.queues[0].depth_at_start, Some(7));
    assert_eq!(run.stages[0].stage, "db.query");
    assert!(run.stages[0].success);
    assert!(imported.warnings().is_empty());
}

#[test]
fn jsonl_fixture_reader_and_path_import_parity_on_counts() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("tracing_spans.jsonl");

    let options = ImportOptions::new("checkout-service")
        .service_version("example")
        .run_id("fixture-example")
        .strict(true);

    let from_path =
        import_jsonl_path(&fixture, options.clone()).expect("path import should succeed");

    let file = File::open(&fixture).expect("fixture should open");
    let from_reader = import_jsonl_reader(file, options).expect("reader import should succeed");

    let run_path = from_path.run();
    let run_reader = from_reader.run();
    assert_eq!(run_path.requests.len(), run_reader.requests.len());
    assert_eq!(run_path.queues.len(), run_reader.queues.len());
    assert_eq!(run_path.stages.len(), run_reader.stages.len());
}

#[test]
fn imported_fixture_run_is_analyzable_and_has_no_runtime_snapshots() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("tracing_spans.jsonl");
    let imported = import_jsonl_path(&fixture, ImportOptions::new("checkout-service"))
        .expect("fixture import should succeed");

    let run = imported.run();
    assert_eq!(run.requests.len(), 1);
    assert_eq!(run.queues.len(), 1);
    assert_eq!(run.stages.len(), 1);
    assert!(
        run.runtime_snapshots.is_empty(),
        "tracing-only import must not fabricate runtime snapshots"
    );
    let report = analyze_run(run, AnalyzeOptions::default());
    assert_eq!(report.request_count, 1);
}
