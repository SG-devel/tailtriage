use std::time::Duration;

use tailtriage_core::RuntimeSnapshot;
use tailtriage_tracing::tokio::TracingTokioSession;
use tracing_subscriber::prelude::*;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let output_path = "target/tailtriage-examples/tokio-run.json";
    let session = TracingTokioSession::builder("checkout-service")
        .run_id("tokio-session-to-run")
        .sampler_interval(Duration::from_millis(25))
        .run_json_path(output_path)
        .start()
        .expect("start tracing tokio session");

    let _guard = tracing_subscriber::registry()
        .with(session.layer())
        .set_default();

    {
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
                "db.queue",
                tt.kind = "queue",
                tt.request_id = "req-1",
                tt.queue = "db-pool",
                tt.depth_at_start = 2_i64
            )
            .entered();
            std::thread::sleep(Duration::from_millis(1));
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
        std::thread::sleep(Duration::from_millis(1));
        }
    }

    session.record_runtime_snapshot(RuntimeSnapshot {
        at_unix_ms: 1_700_000_000_000,
        alive_tasks: Some(3),
        global_queue_depth: Some(0),
        local_queue_depth: Some(0),
        blocking_queue_depth: Some(0),
        remote_schedule_count: Some(1),
    });

    let _imported = session.shutdown().await.expect("shutdown session");
    println!("wrote run artifact to {output_path}");
}
