use std::{fs::File, path::PathBuf};

use tailtriage_analyzer::{analyze_run, AnalyzeOptions};
use tailtriage_tracing::{import_jsonl_path, import_jsonl_reader, ImportOptions};

#[test]
fn jsonl_fixture_imports_and_analyzes() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("tracing_spans.jsonl");

    let imported = import_jsonl_path(
        &fixture,
        ImportOptions::new("checkout-service").strict(true),
    )
    .expect("fixture should parse and import");

    let run = imported.run();
    assert_eq!(run.requests.len(), 1);
    assert_eq!(run.queues.len(), 1);
    assert_eq!(run.stages.len(), 1);

    let report = analyze_run(run, AnalyzeOptions::default());
    assert_eq!(report.request_count, 1);
    assert!(
        !report.primary_suspect.evidence.is_empty(),
        "expected at least one suspect lead from imported data"
    );
}

#[test]
fn jsonl_fixture_reader_and_path_match() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("tracing_spans.jsonl");

    let from_path = import_jsonl_path(
        &fixture,
        ImportOptions::new("checkout-service").strict(true),
    )
    .expect("path import should succeed");

    let file = File::open(&fixture).expect("fixture should open");
    let from_reader =
        import_jsonl_reader(file, ImportOptions::new("checkout-service").strict(true))
            .expect("reader import should succeed");

    assert_eq!(from_path.run().requests, from_reader.run().requests);
    assert_eq!(from_path.run().queues, from_reader.run().queues);
    assert_eq!(from_path.run().stages, from_reader.run().stages);
}
