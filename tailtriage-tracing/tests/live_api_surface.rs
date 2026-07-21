#![cfg(feature = "live")]

use tailtriage_tracing::{RecorderLimits, TailtriageLayer, TracingSession, TracingSessionBuilder};

#[test]
fn public_live_api_imports_only_session_layer_and_limits() {
    fn accepts_builder(_: TracingSessionBuilder) {}
    fn accepts_layer(_: TailtriageLayer) {}

    let builder = TracingSession::builder("svc").limits(RecorderLimits::default());
    accepts_builder(builder.clone());
    let session = builder.build().expect("session builds");
    accepts_layer(session.layer());
    let imported =
        futures_executor::block_on(session.shutdown()).expect("async shutdown returns run");
    assert_eq!(imported.run().metadata.service_name, "svc");
}

#[cfg(feature = "tokio")]
#[tokio::test(flavor = "current_thread")]
async fn direct_crate_tokio_methods_compile_and_work() {
    let manual = TracingSession::builder("manual-svc")
        .manual_runtime_snapshots()
        .build()
        .expect("manual runtime session builds");
    let result: Result<(), tailtriage_tracing::ImportError> =
        manual.record_runtime_snapshot(tailtriage_core::RuntimeSnapshot {
            at_unix_ms: 101,
            at_run_us: Some(202),
            alive_tasks: Some(303),
            global_queue_depth: Some(404),
            local_queue_depth: Some(505),
            blocking_queue_depth: Some(606),
            remote_schedule_count: Some(707),
        });
    result.expect("fallible manual snapshot result is ok");
    let manual_run = manual.shutdown().await.expect("manual shutdown");
    assert_eq!(manual_run.run().runtime_snapshots.len(), 1);
    assert_eq!(manual_run.run().runtime_snapshots[0].at_unix_ms, 101);

    let sampled = TracingSession::builder("sampled-svc")
        .sampler_interval(std::time::Duration::from_millis(1))
        .build()
        .expect("sampled session builds");
    sampled
        .record_runtime_snapshot(tailtriage_core::RuntimeSnapshot {
            at_unix_ms: 808,
            at_run_us: None,
            alive_tasks: Some(909),
            global_queue_depth: None,
            local_queue_depth: None,
            blocking_queue_depth: None,
            remote_schedule_count: None,
        })
        .expect("manual snapshot coexists with sampler");
    let sampled_run = sampled.shutdown().await.expect("sampled shutdown");
    assert!(sampled_run
        .run()
        .runtime_snapshots
        .iter()
        .any(|snapshot| snapshot.at_unix_ms == 808 && snapshot.alive_tasks == Some(909)));
}
