use std::error::Error;

use tailtriage_tracing::TracingIntakeSession;
use tracing_subscriber::prelude::*;

fn main() -> Result<(), Box<dyn Error>> {
    let spans_path = std::path::PathBuf::from("target/tailtriage-examples/completed-spans.jsonl");
    std::fs::create_dir_all(spans_path.parent().expect("parent path exists"))?;

    let session = TracingIntakeSession::builder("checkout-service")
        .completed_span_jsonl_path(&spans_path)
        .build()?;

    let subscriber = tracing_subscriber::registry().with(session.layer());
    tracing::subscriber::with_default(subscriber, || {
        let request = tracing::info_span!(
            "http.request",
            tt.kind = "request",
            tt.request_id = "req-1",
            tt.route = "/checkout",
            tt.outcome = "ok"
        );
        let _request_guard = request.enter();
        let _queue_guard = tracing::info_span!(
            "admission.queue",
            tt.kind = "queue",
            tt.request_id = "req-1",
            tt.queue = "admission",
            tt.depth_at_start = 5_u64
        )
        .entered();
        let _stage_guard = tracing::info_span!(
            "db.stage",
            tt.kind = "stage",
            tt.request_id = "req-1",
            tt.stage = "db",
            tt.success = true
        )
        .entered();
    });

    session.shutdown()?;
    println!("wrote completed spans to: {}", spans_path.display());
    // Import + analyze with:
    // tailtriage import tracing-json target/tailtriage-examples/completed-spans.jsonl --input-format tailtriage-span-jsonl --service checkout-service --output target/tailtriage-examples/run.json
    // tailtriage analyze target/tailtriage-examples/run.json
    Ok(())
}
