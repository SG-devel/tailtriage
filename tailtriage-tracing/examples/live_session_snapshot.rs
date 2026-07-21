use std::error::Error;

use tailtriage_analyzer::{analyze_run, render_text, AnalyzeOptions};
use tailtriage_tracing::TracingSession;
use tracing_subscriber::prelude::*;

fn main() -> Result<(), Box<dyn Error>> {
    let session = TracingSession::builder("checkout-service")
        .service_version("example")
        .run_id("live-session-example")
        .strict(false)
        .build()?;

    // This standalone example uses a scoped local subscriber; service startup
    // should install the tailtriage layer in the process-wide subscriber setup.
    let subscriber = tracing_subscriber::registry().with(session.layer());

    tracing::subscriber::with_default(subscriber, || {
        let _request_entered = tracing::info_span!(
            "http.request",
            tt.kind = "request",
            tt.request_id = "req-1",
            tt.route = "/checkout",
            tt.outcome = "ok"
        )
        .entered();

        {
            let _queue_entered = tracing::info_span!(
                "checkout.queue",
                tt.kind = "queue",
                tt.request_id = "req-1",
                tt.queue = "db-pool",
                tt.depth_at_start = 4_u64
            )
            .entered();
        }

        {
            let _stage_entered = tracing::info_span!(
                "checkout.stage",
                tt.kind = "stage",
                tt.request_id = "req-1",
                tt.stage = "db.query",
                tt.success = true
            )
            .entered();
        }
    });

    let imported = futures_executor::block_on(session.shutdown())?;
    let run = imported.run();
    let diagnosis = analyze_run(run, AnalyzeOptions::default());
    println!("{}", render_text(&diagnosis));
    Ok(())
}
