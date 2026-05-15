use tailtriage_analyzer::{analyze_run, render_text, AnalyzeOptions};
use tailtriage_tracing::TracingRecorder;
use tracing_subscriber::prelude::*;

fn main() -> Result<(), tailtriage_tracing::ImportError> {
    let recorder = TracingRecorder::builder("checkout-service")
        .service_version("example")
        .run_id("live-recorder-example")
        .strict(false)
        .build();

    let subscriber = tracing_subscriber::registry().with(recorder.layer());
    tracing::subscriber::with_default(subscriber, || {
        let request = tracing::info_span!(
            "http.request",
            tt.kind = "request",
            tt.request_id = "req-42",
            tt.route = "/checkout",
            tt.outcome = tracing::field::Empty
        );
        let _request_entered = request.enter();

        let queue = tracing::info_span!(
            "queue.db_pool",
            tt.kind = "queue",
            tt.request_id = "req-42",
            tt.queue = "db_pool",
            tt.depth_at_start = 4_u64
        );
        drop(queue);

        let stage = tracing::info_span!(
            "stage.db",
            tt.kind = "stage",
            tt.request_id = "req-42",
            tt.stage = "db",
            tt.success = true
        );
        drop(stage);

        request.record("tt.outcome", "ok");
    });

    let imported = recorder.shutdown()?;
    let report = analyze_run(imported.run(), AnalyzeOptions::default());

    println!("=== TracingRecorder run summary ===");
    println!(
        "requests={} stages={} queues={} warnings={}",
        imported.run().requests.len(),
        imported.run().stages.len(),
        imported.run().queues.len(),
        imported.warnings().len()
    );
    println!("\n=== Analyzer text report ===\n{}", render_text(&report));

    Ok(())
}
