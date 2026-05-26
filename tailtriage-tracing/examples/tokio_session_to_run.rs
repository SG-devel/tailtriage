use tailtriage_core::RuntimeSnapshot;
use tailtriage_tracing::tokio::TracingTokioSession;
use tracing_subscriber::prelude::*;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let output = "target/tailtriage-examples/tokio-run.json";
    let session = TracingTokioSession::builder("tokio-session-demo")
        .run_json_path(output)
        .disable_background_sampler()
        .start()
        .expect("start session");

    let subscriber = tracing_subscriber::registry().with(session.layer());
    tracing::subscriber::with_default(subscriber, || {
        let request = tracing::info_span!(
            "tt.request",
            tt.kind = "request",
            tt.request_id = "req-1",
            tt.route = "/checkout",
            tt.outcome = "ok"
        );
        request.in_scope(|| {
            let queue = tracing::info_span!(
                "tt.queue",
                tt.kind = "queue",
                tt.request_id = "req-1",
                tt.queue = "work",
                tt.depth_at_start = 2_u64
            );
            queue.in_scope(|| {});

            let stage = tracing::info_span!(
                "tt.stage",
                tt.kind = "stage",
                tt.request_id = "req-1",
                tt.stage = "db",
                tt.success = true
            );
            stage.in_scope(|| {});
        });
    });

    session.record_runtime_snapshot(RuntimeSnapshot {
        at_unix_ms: 1_700_000_000_000,
        alive_tasks: Some(12),
        global_queue_depth: Some(3),
        local_queue_depth: Some(1),
        blocking_queue_depth: Some(0),
        remote_schedule_count: Some(5),
    });

    session.shutdown().await.expect("shutdown session");
    println!("wrote run artifact to {output}");
}
