use std::error::Error;

use tailtriage_tracing::TracingSession;
use tracing_subscriber::prelude::*;

fn main() -> Result<(), Box<dyn Error>> {
    let spans_path = std::path::PathBuf::from("target/tailtriage-examples/completed-spans.jsonl");

    let session = TracingSession::builder("checkout-service")
        .completed_span_jsonl_path(&spans_path)
        .build()?;

    // This standalone example uses a scoped local subscriber; service startup
    // should install the tailtriage layer in the process-wide subscriber setup.
    let subscriber = tracing_subscriber::registry().with(session.layer());
    tracing::subscriber::with_default(subscriber, || {
        let _request_guard = tracing::info_span!(
            "http.request",
            tt.kind = "request",
            tt.request_id = "req-1",
            tt.route = "/checkout",
            tt.outcome = "ok"
        )
        .entered();
        {
            let _queue_guard = tracing::info_span!(
                "admission.queue",
                tt.kind = "queue",
                tt.request_id = "req-1",
                tt.queue = "admission",
                tt.depth_at_start = 5_u64
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

    let imported = futures_executor::block_on(session.shutdown())?;
    for warning in imported.warnings() {
        eprintln!("warning: {}", warning.message());
    }
    println!("wrote completed spans to: {}", spans_path.display());
    // Import + analyze with:
    // tailtriage import tracing-spans-jsonl target/tailtriage-examples/completed-spans.jsonl --service checkout-service --output target/tailtriage-examples/run.json
    // tailtriage analyze target/tailtriage-examples/run.json
    Ok(())
}
