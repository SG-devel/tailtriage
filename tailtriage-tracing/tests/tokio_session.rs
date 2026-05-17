#![cfg(feature = "tokio")]

use std::time::Duration;

use tailtriage_tokio::SamplerStartError;
use tailtriage_tracing::tokio::{TracingTokioSession, TracingTokioSessionStartError};
use tracing_subscriber::prelude::*;

#[tokio::test(flavor = "current_thread")]
async fn session_merges_tracing_and_runtime() {
    let session = TracingTokioSession::builder("svc")
        .sampler_interval(Duration::from_millis(1))
        .start()
        .expect("start session");

    let subscriber = tracing_subscriber::registry().with(session.layer());
    tracing::subscriber::with_default(subscriber, || {
        tracing::info_span!(
            "req",
            tt.kind = "request",
            tt.request_id = "r1",
            tt.route = "/checkout"
        )
        .in_scope(|| {
            tracing::info_span!(
                "stage",
                tt.kind = "stage",
                tt.request_id = "r1",
                tt.stage = "db"
            )
            .in_scope(|| {
                tracing::info_span!(
                    "queue",
                    tt.kind = "queue",
                    tt.request_id = "r1",
                    tt.queue = "global",
                    tt.depth_at_start = 2_u64
                )
                .in_scope(|| {});
            });
        });
    });

    tokio::time::sleep(Duration::from_millis(5)).await;
    let imported = session.shutdown().await.expect("shutdown session");
    let run = imported.run();
    assert!(!run.requests.is_empty());
    assert!(!run.stages.is_empty());
    assert!(!run.queues.is_empty());
    assert_eq!(run.queues[0].depth_at_start, Some(2));
    assert!(!run.runtime_snapshots.is_empty());
    assert!(run.metadata.effective_tokio_sampler_config.is_some());
}

#[test]
fn start_outside_runtime_fails_clearly() {
    let err = TracingTokioSession::builder("svc")
        .start()
        .expect_err("must fail outside tokio runtime");
    assert!(matches!(
        err,
        TracingTokioSessionStartError::SamplerStart(SamplerStartError::MissingRuntime)
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn zero_sampler_interval_fails_clearly() {
    let err = TracingTokioSession::builder("svc")
        .sampler_interval(Duration::ZERO)
        .start()
        .expect_err("zero interval must fail");
    assert!(matches!(
        err,
        TracingTokioSessionStartError::SamplerStart(SamplerStartError::ZeroInterval)
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn runtime_merge_keeps_tracing_requests() {
    let session = TracingTokioSession::builder("svc")
        .sampler_interval(Duration::from_millis(1))
        .start()
        .expect("start session");

    let subscriber = tracing_subscriber::registry().with(session.layer());
    tracing::subscriber::with_default(subscriber, || {
        tracing::info_span!(
            "req",
            tt.kind = "request",
            tt.request_id = "same",
            tt.route = "/same"
        )
        .in_scope(|| {});
    });

    tokio::time::sleep(Duration::from_millis(5)).await;
    let imported = session.snapshot_run().expect("snapshot run");
    assert_eq!(imported.run().requests.len(), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn shutdown_preserves_tracing_spans() {
    let session = TracingTokioSession::builder("svc")
        .sampler_interval(Duration::from_millis(1))
        .start()
        .expect("start session");
    let subscriber = tracing_subscriber::registry().with(session.layer());
    tracing::subscriber::with_default(subscriber, || {
        tracing::info_span!(
            "req",
            tt.kind = "request",
            tt.request_id = "r-shutdown",
            tt.route = "/shutdown"
        )
        .in_scope(|| {});
    });
    tokio::time::sleep(Duration::from_millis(5)).await;
    let imported = session.shutdown().await.expect("shutdown");
    assert_eq!(imported.run().requests.len(), 1);
    assert_eq!(imported.run().requests[0].request_id, "r-shutdown");
}
