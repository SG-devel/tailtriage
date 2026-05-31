use std::error::Error;

use tailtriage_tracing::TracingIntakeSession;
use tracing_subscriber::prelude::*;

fn main() -> Result<(), Box<dyn Error>> {
    let run_path = std::path::PathBuf::from("target/tailtriage-examples/live-session-run.json");

    let session = TracingIntakeSession::builder("checkout-service")
        .run_json_path(&run_path)
        .build()?;

    let subscriber = tracing_subscriber::registry().with(session.layer());
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
        let _request_guard = request.enter();

        {
            let _queue_guard = tracing::info_span!(
                "admission.queue",
                tt.kind = "queue",
                tt.request_id = "req-1",
                tt.queue = "admission",
                tt.depth_at_start = 3_u64
            )
            .entered();
        }

        {
            let _stage_guard = tracing::info_span!(
                "db.stage",
                tt.kind = "stage",
                tt.request_id = "req-1",
                tt.stage = "db",
                tt.success = true
            )
            .entered();
        }
    });

    session.shutdown()?;
    println!("wrote run JSON to: {}", run_path.display());
    // Analyze with:
    // tailtriage analyze target/tailtriage-examples/live-session-run.json
    Ok(())
}
