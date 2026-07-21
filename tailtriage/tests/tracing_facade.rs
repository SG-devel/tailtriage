#[cfg(feature = "tracing-live")]
#[test]
fn tracing_live_facade_exposes_builder_layer_snapshot_and_async_shutdown() {
    let session = tailtriage::tracing::TracingSession::builder("svc")
        .build()
        .expect("build live session through facade");
    let _layer = session.layer();
    let snapshot = session
        .snapshot_run()
        .expect("sync snapshot through facade");
    assert_eq!(snapshot.run().metadata.service_name, "svc");
    let final_run =
        futures_executor::block_on(session.shutdown()).expect("async shutdown through facade");
    assert_eq!(final_run.run().metadata.service_name, "svc");
}

#[cfg(feature = "tracing-tokio")]
#[tokio::test(flavor = "current_thread")]
async fn tracing_tokio_facade_exposes_runtime_methods_and_async_shutdown() {
    let manual = tailtriage::tracing::TracingSession::builder("manual-svc")
        .manual_runtime_snapshots()
        .build()
        .expect("manual runtime session through facade");
    manual
        .record_runtime_snapshot(tailtriage::RuntimeSnapshot {
            at_unix_ms: 77,
            at_run_us: Some(11),
            alive_tasks: Some(2),
            global_queue_depth: Some(3),
            local_queue_depth: Some(4),
            blocking_queue_depth: Some(5),
            remote_schedule_count: Some(6),
        })
        .expect("fallible manual snapshot succeeds through facade");
    let manual_run = manual.shutdown().await.expect("manual shutdown");
    assert_eq!(manual_run.run().runtime_snapshots.len(), 1);
    assert_eq!(manual_run.run().runtime_snapshots[0].at_unix_ms, 77);

    let sampled = tailtriage::tracing::TracingSession::builder("sampled-svc")
        .sampler_interval(std::time::Duration::from_millis(1))
        .build()
        .expect("sampled runtime session through facade");
    sampled
        .record_runtime_snapshot(tailtriage::RuntimeSnapshot {
            at_unix_ms: 88,
            at_run_us: None,
            alive_tasks: Some(12),
            global_queue_depth: Some(13),
            local_queue_depth: Some(14),
            blocking_queue_depth: Some(15),
            remote_schedule_count: Some(16),
        })
        .expect("fallible manual snapshot also succeeds with sampler");
    let sampled_run = sampled.shutdown().await.expect("sampled shutdown");
    assert!(sampled_run
        .run()
        .runtime_snapshots
        .iter()
        .any(|snapshot| snapshot.at_unix_ms == 88 && snapshot.alive_tasks == Some(12)));
}
