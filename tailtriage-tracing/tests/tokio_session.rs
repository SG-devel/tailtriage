#![cfg(feature = "tokio")]

use std::{path::PathBuf, time::Duration};

use tailtriage_core::{unix_time_ms, CaptureLimitsOverride, RuntimeSnapshot};
use tailtriage_tokio::SamplerStartError;
use tailtriage_tracing::tokio::{
    TracingTokioSession, TracingTokioSessionShutdownError, TracingTokioSessionStartError,
};
use tailtriage_tracing::ImportError;
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
async fn tracing_tokio_session_start_rejects_blank_service_name() {
    let err = TracingTokioSession::builder("   ")
        .sampler_interval(Duration::from_millis(1))
        .start()
        .expect_err("blank service name must fail at start");
    assert!(matches!(
        err,
        TracingTokioSessionStartError::Import(ImportError::EmptyServiceName)
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
async fn zero_sampler_interval_is_ignored_when_background_sampler_is_disabled() {
    let session = TracingTokioSession::builder("svc")
        .sampler_interval(Duration::ZERO)
        .disable_background_sampler()
        .start()
        .expect("zero interval should be ignored when sampler is disabled");
    session.record_runtime_snapshot(RuntimeSnapshot {
        at_unix_ms: unix_time_ms(),
        alive_tasks: Some(1),
        global_queue_depth: Some(1),
        local_queue_depth: Some(0),
        blocking_queue_depth: Some(0),
        remote_schedule_count: Some(0),
    });

    let imported = session.shutdown().await.expect("shutdown");
    assert_eq!(imported.run().runtime_snapshots.len(), 1);
    assert!(imported
        .run()
        .metadata
        .lifecycle_warnings
        .iter()
        .any(|warning| warning.starts_with(
            "tailtriage-tracing session ran with background runtime sampling disabled"
        )));
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
    assert_eq!(run.runtime_snapshots.len(), 1);
    assert_eq!(run.truncation.dropped_runtime_snapshots, 1);
    assert!(run.truncation.limits_hit);
}

#[tokio::test(flavor = "current_thread")]
async fn capture_limits_override_controls_sampler_and_collector_retention() {
    let session = TracingTokioSession::builder("svc")
        .capture_limits_override(CaptureLimitsOverride {
            max_runtime_snapshots: Some(2),
            ..CaptureLimitsOverride::default()
        })
        .start()
        .expect("start session");
    for idx in 0_u64..5 {
        session.record_runtime_snapshot(RuntimeSnapshot {
            at_unix_ms: unix_time_ms().saturating_add(idx),
            alive_tasks: Some(idx),
            global_queue_depth: Some(idx),
            local_queue_depth: Some(idx),
            blocking_queue_depth: Some(idx),
            remote_schedule_count: Some(idx),
        });
    }
    let run = session.snapshot_run().expect("snapshot run").run().clone();
    assert!(run.runtime_snapshots.len() <= 2);
    assert!(run.truncation.dropped_runtime_snapshots > 0);
    assert_eq!(
        run.metadata
            .effective_tokio_sampler_config
            .expect("tokio sampler metadata")
            .resolved_runtime_snapshot_retention,
        2
    );
}

fn unique_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("tailtriage-{name}-{}", std::process::id()))
}

#[tokio::test(flavor = "current_thread")]
async fn disabled_sampler_manual_snapshot_shutdown_has_manual_warning() {
    let session = TracingTokioSession::builder("svc")
        .disable_background_sampler()
        .start()
        .expect("start");
    session.record_runtime_snapshot(RuntimeSnapshot {
        at_unix_ms: unix_time_ms(),
        alive_tasks: Some(1),
        global_queue_depth: Some(2),
        local_queue_depth: None,
        blocking_queue_depth: Some(1),
        remote_schedule_count: None,
    });
    let run = session.shutdown().await.expect("shutdown").run().clone();
    assert_eq!(run.runtime_snapshots.len(), 1);
    assert!(run.metadata.effective_tokio_sampler_config.is_none());
    assert!(run.metadata.lifecycle_warnings.iter().any(|warning| {
        warning.contains("background runtime sampling disabled")
            && warning.contains("manually recorded")
    }));
}

#[tokio::test(flavor = "current_thread")]
async fn disabled_sampler_without_manual_snapshot_reports_clear_warning() {
    let session = TracingTokioSession::builder("svc")
        .disable_background_sampler()
        .start()
        .expect("start");
    let run = session.shutdown().await.expect("shutdown").run().clone();
    assert!(run.runtime_snapshots.is_empty());
    assert!(run.metadata.lifecycle_warnings.iter().any(|warning| {
        warning.contains("background runtime sampling disabled")
            && warning.contains("no manual runtime snapshots were recorded")
    }));
}

#[tokio::test(flavor = "current_thread")]
async fn run_json_path_writes_with_simple_relative_filename() {
    struct RemoveFileOnDrop(PathBuf);
    impl Drop for RemoveFileOnDrop {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    let run_path = PathBuf::from(format!(
        "tailtriage-simple-run-json-{}-{}.json",
        std::process::id(),
        unix_time_ms()
    ));
    let _ = std::fs::remove_file(&run_path);
    let _cleanup = RemoveFileOnDrop(run_path.clone());

    let session = TracingTokioSession::builder("svc")
        .run_json_path(&run_path)
        .start()
        .expect("start");
    let subscriber = tracing_subscriber::registry().with(session.layer());
    tracing::subscriber::with_default(subscriber, || {
        tracing::info_span!(
            "req",
            tt.kind = "request",
            tt.request_id = "r-simple",
            tt.route = "/simple"
        )
        .in_scope(|| {});
    });
    session.shutdown().await.expect("shutdown");

    let run: tailtriage_core::Run =
        serde_json::from_slice(&std::fs::read(&run_path).expect("read run")).expect("deserialize");
    assert_eq!(run.requests.len(), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn run_json_path_writes_run_with_request_and_creates_parent() {
    let run_path = unique_path("tokio-session/nested/run.json");
    let _ = std::fs::remove_file(&run_path);
    let _ = std::fs::remove_dir_all(run_path.parent().expect("parent"));
    let session = TracingTokioSession::builder("svc")
        .run_json_path(&run_path)
        .start()
        .expect("start");
    let subscriber = tracing_subscriber::registry().with(session.layer());
    tracing::subscriber::with_default(subscriber, || {
        tracing::info_span!(
            "req",
            tt.kind = "request",
            tt.request_id = "r1",
            tt.route = "/r1"
        )
        .in_scope(|| {});
    });
    session.shutdown().await.expect("shutdown");
    let run: tailtriage_core::Run =
        serde_json::from_slice(&std::fs::read(&run_path).expect("read run")).expect("deserialize");
    assert_eq!(run.requests.len(), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn zero_request_run_json_path_fails_and_does_not_write_file() {
    let run_path = unique_path("tokio-session-empty/run.json");
    let _ = std::fs::remove_file(&run_path);
    let session = TracingTokioSession::builder("svc")
        .run_json_path(&run_path)
        .start()
        .expect("start");
    let err = session.shutdown().await.expect_err("must fail");
    let err_text = err.to_string();
    assert!(matches!(
        err,
        TracingTokioSessionShutdownError::Import(ImportError::ZeroRequestArtifact { .. })
    ));
    assert!(err_text.contains("tracing import produced zero request events"));
    assert!(!run_path.exists());
}

#[tokio::test(flavor = "current_thread")]
async fn zero_request_run_json_path_includes_intake_warnings_when_present() {
    let run_path = unique_path("tokio-session-empty-with-warnings/run.json");
    let _ = std::fs::remove_file(&run_path);
    let session = TracingTokioSession::builder("svc")
        .run_json_path(&run_path)
        .start()
        .expect("start");
    let subscriber = tracing_subscriber::registry().with(session.layer());
    tracing::subscriber::with_default(subscriber, || {
        tracing::info_span!(
            "bad",
            tt.kind = "bogus",
            tt.request_id = "r-bad",
            tt.route = "/bad"
        )
        .in_scope(|| {});
    });
    let err = session.shutdown().await.expect_err("must fail");
    let err_text = err.to_string();
    assert!(matches!(
        err,
        TracingTokioSessionShutdownError::Import(
            ImportError::ZeroRequestArtifactWithWarnings { .. }
        )
    ));
    assert!(err_text.contains("tracing import produced zero request events"));
    assert!(err_text.contains("warnings observed during tracing intake:"));
    assert!(err_text.contains("unknown tt.kind 'bogus'"));
    assert!(!run_path.exists());
}

#[tokio::test(flavor = "current_thread")]
async fn snapshot_run_does_not_write_run_json() {
    let run_path = unique_path("tokio-session-snapshot-only/run.json");
    let _ = std::fs::remove_file(&run_path);
    let session = TracingTokioSession::builder("svc")
        .run_json_path(&run_path)
        .start()
        .expect("start");
    let _ = session.snapshot_run().expect("snapshot run");
    assert!(!run_path.exists());
    let _ = session.shutdown().await;
}
