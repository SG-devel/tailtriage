use tailtriage_analyzer::{analyze_run, AnalyzeOptions};
use tailtriage_tracing::{import_jsonl_path, ImportOptions, TracingRecorder};
use tracing_subscriber::prelude::*;

#[test]
fn jsonl_fixture_imports_and_analyzes() {
    let imported = import_jsonl_path(
        "examples/tracing_spans.jsonl",
        ImportOptions::new("checkout-service").strict(false),
    )
    .expect("fixture should import");

    let run = imported.run();
    assert_eq!(run.requests.len(), 1);
    assert_eq!(run.queues.len(), 1);
    assert_eq!(run.stages.len(), 1);

    let report = analyze_run(run, AnalyzeOptions::default());
    assert!(report.request_count >= 1);
}

#[test]
fn live_recorder_captures_and_analyzes() {
    let recorder = TracingRecorder::builder("checkout-service")
        .service_version("example")
        .run_id("live-recorder-example-test")
        .strict(false)
        .build();

    let subscriber = tracing_subscriber::registry().with(recorder.layer());
    tracing::subscriber::with_default(subscriber, || {
        let request = tracing::info_span!(
            "http.request",
            tt.kind = "request",
            tt.request_id = "req-live-test",
            tt.route = "/checkout"
        );
        let _request_entered = request.enter();

        let stage = tracing::info_span!(
            "checkout.db",
            tt.kind = "stage",
            tt.request_id = "req-live-test",
            tt.stage = "db",
            tt.success = true
        );
        let _stage_entered = stage.enter();
    });

    let imported = recorder.shutdown().expect("recorder should convert spans");
    let run = imported.run();
    assert_eq!(run.requests.len(), 1);
    assert_eq!(run.stages.len(), 1);

    let report = analyze_run(run, AnalyzeOptions::default());
    assert!(report.request_count >= 1);
}
