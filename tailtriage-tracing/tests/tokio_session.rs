#![cfg(feature = "tokio")]

use std::time::Duration;

use tailtriage_core::{unix_time_ms, RuntimeSnapshot};
use tailtriage_tracing::{ImportError, TracingSession};
use tracing_subscriber::prelude::*;

async fn wait_for_runtime_snapshot(session: &TracingSession) {
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
    let session = TracingSession::builder("svc")
        .sampler_interval(Duration::from_millis(1))
        .build()
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
    let err = TracingSession::builder("svc")
        .sampler_interval(Duration::from_millis(1))
        .build()
        .expect_err("must fail outside tokio runtime");
    assert!(matches!(
        err,
        ImportError::Io {
            operation: "start tracing Tokio runtime sampler",
            ..
        }
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn tracing_tokio_session_start_rejects_blank_service_name() {
    let err = TracingSession::builder("   ")
        .sampler_interval(Duration::from_millis(1))
        .build()
        .expect_err("blank service name must fail at start");
    assert!(matches!(err, ImportError::EmptyServiceName));
}

#[tokio::test(flavor = "current_thread")]
async fn zero_sampler_interval_fails_clearly() {
    let err = TracingSession::builder("svc")
        .sampler_interval(Duration::ZERO)
        .build()
        .expect_err("zero interval must fail");
    assert!(matches!(
        err,
        ImportError::InvalidConfiguration { option: "sampler_interval", ref reason }
            if reason == "sampler interval must be greater than zero"
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn zero_sampler_interval_fails_even_with_manual_runtime_snapshots() {
    let err = TracingSession::builder("svc")
        .manual_runtime_snapshots()
        .sampler_interval(Duration::ZERO)
        .build()
        .expect_err("zero interval must fail regardless of call order");
    assert!(matches!(
        err,
        ImportError::InvalidConfiguration { option: "sampler_interval", ref reason }
            if reason == "sampler interval must be greater than zero"
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn a1_bare_tokio_feature_session_rejects_manual_runtime_recording() {
    let session = TracingSession::builder("svc")
        .build()
        .expect("start session");
    let err = session
        .record_runtime_snapshot(RuntimeSnapshot {
            at_unix_ms: unix_time_ms(),
            at_run_us: None,
            alive_tasks: Some(10),
            global_queue_depth: Some(11),
            local_queue_depth: Some(12),
            blocking_queue_depth: Some(13),
            remote_schedule_count: Some(14),
        })
        .expect_err("runtime collection not enabled");
    assert!(matches!(
        err,
        ImportError::InvalidConfiguration { option: "runtime_snapshots", ref reason }
            if reason.contains("runtime collection is not enabled")
    ));
    let imported = session.shutdown().await.expect("shutdown");
    assert!(imported.run().runtime_snapshots.is_empty());
    assert!(imported.run().metadata.lifecycle_warnings.is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn a2_manual_runtime_snapshots_retains_snapshot_without_sampler() {
    let session = TracingSession::builder("svc")
        .manual_runtime_snapshots()
        .build()
        .expect("start session");
    session
        .record_runtime_snapshot(RuntimeSnapshot {
            at_unix_ms: 42,
            at_run_us: Some(7),
            alive_tasks: Some(1),
            global_queue_depth: Some(2),
            local_queue_depth: Some(3),
            blocking_queue_depth: Some(4),
            remote_schedule_count: Some(5),
        })
        .expect("record manual snapshot");
    let imported = session.shutdown().await.expect("shutdown");
    assert_eq!(imported.run().runtime_snapshots.len(), 1);
    assert_eq!(imported.run().runtime_snapshots[0].at_unix_ms, 42);
    assert!(imported
        .run()
        .metadata
        .effective_tokio_sampler_config
        .is_none());
    assert!(imported
        .run()
        .metadata
        .lifecycle_warnings
        .iter()
        .any(|warning| {
            warning.contains("background runtime sampling disabled")
                && warning.contains("manually recorded")
        }));
}

#[tokio::test(flavor = "current_thread")]
async fn a3_sampler_interval_starts_background_and_retains_manual_snapshot() {
    let session = TracingSession::builder("svc")
        .sampler_interval(Duration::from_millis(1))
        .build()
        .expect("start session");
    session
        .record_runtime_snapshot(RuntimeSnapshot {
            at_unix_ms: 123_456,
            at_run_us: None,
            alive_tasks: Some(99),
            global_queue_depth: Some(98),
            local_queue_depth: Some(97),
            blocking_queue_depth: Some(96),
            remote_schedule_count: Some(95),
        })
        .expect("record manual snapshot");
    wait_for_runtime_snapshot(&session).await;
    let imported = session.shutdown().await.expect("shutdown");
    assert!(!imported.run().runtime_snapshots.is_empty());
    assert!(imported
        .run()
        .runtime_snapshots
        .iter()
        .any(|snapshot| snapshot.at_unix_ms == 123_456 && snapshot.alive_tasks == Some(99)));
    assert!(imported
        .run()
        .metadata
        .effective_tokio_sampler_config
        .is_some());
    assert!(!imported
        .run()
        .metadata
        .lifecycle_warnings
        .iter()
        .any(|warning| { warning.contains("background runtime sampling disabled") }));
}

#[tokio::test(flavor = "current_thread")]
async fn a5_ordinary_live_session_captures_request_stage_queue_without_runtime_config() {
    let session = TracingSession::builder("svc")
        .build()
        .expect("start session");
    let subscriber = tracing_subscriber::registry().with(session.layer());
    tracing::subscriber::with_default(subscriber, || {
        tracing::info_span!(
            "req",
            tt.kind = "request",
            tt.request_id = "r-a5",
            tt.route = "/a5",
            tt.outcome = "ok"
        )
        .in_scope(|| {
            tracing::info_span!(
                "stage",
                tt.kind = "stage",
                tt.request_id = "r-a5",
                tt.stage = "work",
                tt.success = true
            )
            .in_scope(|| {});
            tracing::info_span!(
                "queue",
                tt.kind = "queue",
                tt.request_id = "r-a5",
                tt.queue = "permits",
                tt.depth_at_start = 3_u64
            )
            .in_scope(|| {});
        });
    });
    let imported = session.shutdown().await.expect("shutdown");
    assert_eq!(imported.run().requests.len(), 1);
    assert_eq!(imported.run().stages.len(), 1);
    assert_eq!(imported.run().queues.len(), 1);
    assert!(imported.run().runtime_snapshots.is_empty());
    assert!(imported.run().metadata.lifecycle_warnings.is_empty());
}
