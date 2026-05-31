use std::error::Error;

use tailtriage_core::{unix_time_ms, RuntimeSnapshot};
use tailtriage_tracing::tokio::TracingTokioSession;
use tracing_subscriber::prelude::*;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let run_path = std::path::PathBuf::from("target/tailtriage-examples/tokio-run.json");
    let session = TracingTokioSession::builder("checkout-service")
        .run_json_path(&run_path)
        .disable_background_sampler()
        .start()?;

    // This runnable example uses `with_default` as a scoped/local harness.
    // Services should install the layer in process-wide startup subscriber setup.
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

        let _queue_guard = tracing::info_span!(
            "admission.queue",
            tt.kind = "queue",
            tt.request_id = "req-1",
            tt.queue = "admission",
            tt.depth_at_start = 2_u64
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

    session.record_runtime_snapshot(RuntimeSnapshot {
        at_unix_ms: unix_time_ms(),
        alive_tasks: Some(4),
        global_queue_depth: Some(1),
        local_queue_depth: Some(0),
        blocking_queue_depth: Some(0),
        remote_schedule_count: Some(2),
    });

    session.shutdown().await?;
    println!("wrote run JSON to: {}", run_path.display());
    Ok(())
}
