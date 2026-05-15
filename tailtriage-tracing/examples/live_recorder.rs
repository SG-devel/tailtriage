use tailtriage_analyzer::{analyze_run, render_text, AnalyzeOptions};
use tailtriage_tracing::TracingRecorder;
use tracing_subscriber::prelude::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
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

        {
            let queue = tracing::info_span!(
                "queue.db",
                tt.kind = "queue",
                tt.request_id = "req-42",
                tt.queue = "db_pool",
                tt.depth_at_start = 3_u64
            );
            let _queue_entered = queue.enter();
        }

        {
            let stage = tracing::info_span!(
                "stage.db",
                tt.kind = "stage",
                tt.request_id = "req-42",
                tt.stage = "db.query",
                tt.success = true
            );
            let _stage_entered = stage.enter();
        }

        request.record("tt.outcome", "ok");
    });

    let imported = recorder.shutdown()?;
    let run = imported.run();
    let report = analyze_run(run, AnalyzeOptions::default());

    println!("{}", render_text(&report));

    Ok(())
}
