use std::error::Error;

use tailtriage_analyzer::{analyze_run, render_text, AnalyzeOptions};
use tailtriage_tracing::TracingRecorder;
use tracing_subscriber::prelude::*;

fn main() -> Result<(), Box<dyn Error>> {
    let recorder = TracingRecorder::builder("checkout-service")
        .service_version("example")
        .run_id("live-recorder-example")
        .strict(false)
        .build()?;

    let subscriber = tracing_subscriber::registry().with(recorder.layer());

    // Scoped/local example usage: services should install the layer in their
    // normal process-wide subscriber setup during startup.
    tracing::subscriber::with_default(subscriber, || {
        let request = tracing::info_span!(
            "http.request",
            tt.kind = "request",
            tt.request_id = "req-1",
            tt.route = "/checkout",
            tt.outcome = "ok"
        );
        let _request_entered = request.enter();

        {
            let queue = tracing::info_span!(
                "checkout.queue",
                tt.kind = "queue",
                tt.request_id = "req-1",
                tt.queue = "db-pool",
                tt.depth_at_start = 4_u64
            );
            let _queue_entered = queue.enter();
        }

        {
            let stage = tracing::info_span!(
                "checkout.stage",
                tt.kind = "stage",
                tt.request_id = "req-1",
                tt.stage = "db.query",
                tt.success = true
            );
            let _stage_entered = stage.enter();
        }
    });

    let imported = recorder.shutdown()?;
    let run = imported.run();
    let diagnosis = analyze_run(run, AnalyzeOptions::default());
    println!("{}", render_text(&diagnosis));
    Ok(())
}
