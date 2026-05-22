#![cfg(feature = "tokio")]

use std::time::Duration;

use tailtriage_core::{unix_time_ms, CaptureLimitsOverride, RuntimeSnapshot};
use tailtriage_tokio::SamplerStartError;
use tailtriage_tracing::tokio::{TracingTokioSession, TracingTokioSessionStartError};
use tracing_subscriber::prelude::*;

async fn wait_for_runtime_snapshot(session: &TracingTokioSession) {
    let deadline = tokio::time::Instant::now() + Duration::from_millis(500);
    loop {
        let imported = session.snapshot_run().expect("snapshot run");
        if !imported.run().runtime_snapshots.is_empty() {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "runtime sampler did not produce a snapshot before timeout"
        );
        tokio::task::yield_now().await;
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

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

    wait_for_runtime_snapshot(&session).await;
    let imported = session.shutdown().await.expect("shutdown session");
    let run = imported.run();
    assert!(!run.requests.is_empty());
    assert!(!run.stages.is_empty());
    assert!(!run.queues.is_empty());
    assert_eq!(run.queues[0].depth_at_start, Some(2));
    assert!(!run.runtime_snapshots.is_empty());
    assert!(run.metadata.effective_tokio_sampler_config.is_some());
    assert_eq!(
        run.metadata.finalized_at_unix_ms,
        Some(run.metadata.finished_at_unix_ms)
    );
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

#[tokio::test(flavor = "current_thread")]
async fn record_runtime_snapshot_is_visible_in_snapshot_run() {
    let session = TracingTokioSession::builder("svc")
        .start()
        .expect("start session");
    let at = unix_time_ms();
    session.record_runtime_snapshot(RuntimeSnapshot {
        at_unix_ms: at,
        alive_tasks: Some(3),
        global_queue_depth: Some(2),
        local_queue_depth: Some(1),
        blocking_queue_depth: Some(4),
        remote_schedule_count: Some(5),
    });

    let imported = session.snapshot_run().expect("snapshot run");
    assert_eq!(
        imported.run().metadata.finalized_at_unix_ms,
        Some(imported.run().metadata.finished_at_unix_ms)
    );
    assert!(imported
        .run()
        .runtime_snapshots
        .iter()
        .any(|s| s.at_unix_ms == at && s.blocking_queue_depth == Some(4)));
}

#[tokio::test(flavor = "current_thread")]
async fn record_runtime_snapshot_is_visible_in_shutdown_output() {
    let session = TracingTokioSession::builder("svc")
        .start()
        .expect("start session");
    let at = unix_time_ms();
    session.record_runtime_snapshot(RuntimeSnapshot {
        at_unix_ms: at,
        alive_tasks: Some(7),
        global_queue_depth: Some(6),
        local_queue_depth: Some(5),
        blocking_queue_depth: Some(4),
        remote_schedule_count: Some(3),
    });

    let imported = session.shutdown().await.expect("shutdown");
    assert_eq!(
        imported.run().metadata.finalized_at_unix_ms,
        Some(imported.run().metadata.finished_at_unix_ms)
    );
    assert!(imported
        .run()
        .runtime_snapshots
        .iter()
        .any(|s| s.at_unix_ms == at && s.global_queue_depth == Some(6)));
}

#[tokio::test(flavor = "current_thread")]
async fn record_runtime_snapshot_does_not_alter_tracing_events() {
    let session = TracingTokioSession::builder("svc")
        .start()
        .expect("start session");
    let subscriber = tracing_subscriber::registry().with(session.layer());
    tracing::subscriber::with_default(subscriber, || {
        tracing::info_span!(
            "req",
            tt.kind = "request",
            tt.request_id = "r-manual",
            tt.route = "/manual"
        )
        .in_scope(|| {
            tracing::info_span!(
                "stage",
                tt.kind = "stage",
                tt.request_id = "r-manual",
                tt.stage = "work"
            )
            .in_scope(|| {
                tracing::info_span!(
                    "queue",
                    tt.kind = "queue",
                    tt.request_id = "r-manual",
                    tt.queue = "q",
                    tt.depth_at_start = 1_u64
                )
                .in_scope(|| {});
            });
        });
    });
    session.record_runtime_snapshot(RuntimeSnapshot {
        at_unix_ms: unix_time_ms(),
        alive_tasks: None,
        global_queue_depth: Some(1),
        local_queue_depth: None,
        blocking_queue_depth: Some(1),
        remote_schedule_count: None,
    });
    let run = session.snapshot_run().expect("snapshot").run().clone();
    assert_eq!(run.requests.len(), 1);
    assert_eq!(run.stages.len(), 1);
    assert_eq!(run.queues.len(), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn runtime_snapshot_truncation_propagates_to_imported_run() {
    let session = TracingTokioSession::builder("svc")
        .capture_limits_override(CaptureLimitsOverride {
            max_runtime_snapshots: Some(1),
            ..CaptureLimitsOverride::default()
        })
        .start()
        .expect("start session");
    session.record_runtime_snapshot(RuntimeSnapshot {
        at_unix_ms: unix_time_ms(),
        alive_tasks: Some(1),
        global_queue_depth: Some(1),
        local_queue_depth: Some(1),
        blocking_queue_depth: Some(1),
        remote_schedule_count: Some(1),
    });
    session.record_runtime_snapshot(RuntimeSnapshot {
        at_unix_ms: unix_time_ms().saturating_add(1),
        alive_tasks: Some(2),
        global_queue_depth: Some(2),
        local_queue_depth: Some(2),
        blocking_queue_depth: Some(2),
        remote_schedule_count: Some(2),
    });

    let imported = session.snapshot_run().expect("snapshot run");
    let run = imported.run();
    assert!(run.runtime_snapshots.len() <= 1);
    assert!(run.truncation.dropped_runtime_snapshots > 0);
    assert!(run.truncation.limits_hit);
    let sampler = run
        .metadata
        .effective_tokio_sampler_config
        .expect("sampler metadata");
    assert_eq!(sampler.resolved_runtime_snapshot_retention, 1);
}

#[tokio::test(flavor = "current_thread")]
async fn capture_limits_override_controls_runtime_sampler_and_collector_retention() {
    let session = TracingTokioSession::builder("svc")
        .sampler_interval(Duration::from_millis(1))
        .capture_limits_override(CaptureLimitsOverride {
            max_runtime_snapshots: Some(2),
            ..CaptureLimitsOverride::default()
        })
        .start()
        .expect("start session");

    for i in 0..8_u64 {
        session.record_runtime_snapshot(RuntimeSnapshot {
            at_unix_ms: unix_time_ms().saturating_add(i),
            alive_tasks: Some(i),
            global_queue_depth: Some(i),
            local_queue_depth: Some(i),
            blocking_queue_depth: Some(i),
            remote_schedule_count: Some(i),
        });
    }

    tokio::time::sleep(Duration::from_millis(10)).await;
    let run = session.snapshot_run().expect("snapshot").run().clone();
    assert!(run.runtime_snapshots.len() <= 2);
    assert!(run.truncation.dropped_runtime_snapshots > 0);
    let sampler = run
        .metadata
        .effective_tokio_sampler_config
        .expect("sampler metadata");
    assert_eq!(sampler.resolved_runtime_snapshot_retention, 2);
}
