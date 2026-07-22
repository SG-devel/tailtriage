#![cfg(feature = "tokio")]

use std::time::Duration;

use tailtriage_core::{unix_time_ms, RuntimeSnapshot};
use tailtriage_tracing::{ImportError, TracingSession};
use tracing_subscriber::prelude::*;

async fn wait_for_runtime_snapshot(session: &TracingSession) {
    let deadline = tokio::time::Instant::now() + Duration::from_millis(500);
    loop {
        let imported = session.snapshot_run().expect("snapshot run");
        assert!(imported.run().metadata.finalized_at_unix_ms.is_none());
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
async fn tracing_session_snapshot_is_unfinalized() {
    let session = TracingSession::builder("svc")
        .build()
        .expect("start session");
    let imported = session.snapshot_run().expect("snapshot run");
    let run = imported.run();
    assert_eq!(run.schema_version, tailtriage_core::SCHEMA_VERSION);
    assert!(run.metadata.finalized_at_unix_ms.is_none());
    session.shutdown().await.expect("shutdown session");
}

#[tokio::test(flavor = "current_thread")]
async fn tracing_session_shutdown_output_is_finalized() {
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
    let before_shutdown = unix_time_ms();
    let imported = session.shutdown().await.expect("shutdown session");
    let after_shutdown = unix_time_ms();
    let run = imported.run();
    assert!(!run.requests.is_empty());
    assert!(!run.stages.is_empty());
    assert!(!run.queues.is_empty());
    assert_eq!(run.queues[0].depth_at_start, Some(2));
    assert!(!run.runtime_snapshots.is_empty());
    assert!(run.metadata.effective_tokio_sampler_config.is_some());
    assert_eq!(run.schema_version, tailtriage_core::SCHEMA_VERSION);
    let finalized = run
        .metadata
        .finalized_at_unix_ms
        .expect("shutdown output is finalized");
    assert!(before_shutdown <= finalized);
    assert!(finalized <= after_shutdown);
    assert!(finalized >= run.metadata.started_at_unix_ms);
    let max_evidence_end = run
        .requests
        .iter()
        .map(|request| request.finished_at_unix_ms)
        .chain(run.stages.iter().map(|stage| stage.finished_at_unix_ms))
        .chain(run.queues.iter().map(|queue| queue.waited_until_unix_ms))
        .chain(
            run.runtime_snapshots
                .iter()
                .map(|snapshot| snapshot.at_unix_ms),
        )
        .max()
        .expect("retained evidence end");
    assert!(finalized >= max_evidence_end);
    let serialized = serde_json::to_value(run).expect("serialize run");
    assert!(serialized["metadata"].get("finished_at_unix_ms").is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn shutdown_freezes_completed_spans_after_sampler_shutdown_boundary() {
    let session = TracingSession::builder("svc")
        .sampler_interval(Duration::from_secs(60))
        .build()
        .expect("start session");
    let subscriber = tracing_subscriber::registry().with(session.layer());
    let span = tracing::subscriber::with_default(subscriber, || {
        tracing::info_span!(
            "req",
            tt.kind = "request",
            tt.request_id = "completed-during-shutdown",
            tt.route = "/shutdown"
        )
    });

    let shutdown_task = tokio::spawn(async move { session.shutdown().await });
    tokio::task::yield_now().await;
    drop(span);

    let imported = shutdown_task
        .await
        .expect("shutdown task completes")
        .expect("shutdown session");
    let run = imported.run();
    assert!(run
        .requests
        .iter()
        .any(|request| request.request_id == "completed-during-shutdown"));
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
    let snapshot = session.snapshot_run().expect("snapshot");
    assert_eq!(
        snapshot.run().schema_version,
        tailtriage_core::SCHEMA_VERSION
    );
    assert_eq!(snapshot.run().runtime_snapshots.len(), 1);
    assert_eq!(snapshot.run().runtime_snapshots[0].at_unix_ms, 42);
    assert_eq!(snapshot.run().metadata.started_at_unix_ms, 42);
    assert!(snapshot.run().metadata.finalized_at_unix_ms.is_none());

    let before_shutdown = unix_time_ms();
    let imported = session.shutdown().await.expect("shutdown");
    let after_shutdown = unix_time_ms();
    assert_eq!(imported.run().runtime_snapshots.len(), 1);
    assert_eq!(imported.run().runtime_snapshots[0].at_unix_ms, 42);
    let finalized = imported
        .run()
        .metadata
        .finalized_at_unix_ms
        .expect("shutdown finalizes run");
    assert!(before_shutdown <= finalized);
    assert!(finalized <= after_shutdown);
    assert!(finalized >= imported.run().metadata.started_at_unix_ms);
    assert!(finalized >= 42);
    let serialized = serde_json::to_value(imported.run()).expect("serialize run");
    assert!(serialized["metadata"].get("finished_at_unix_ms").is_none());
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

#[tokio::test(flavor = "current_thread")]
async fn active_sampler_shutdown_stops_before_outputs_and_keeps_runtime_run_only() {
    let dir = tempfile::tempdir().expect("tempdir");
    let run_path = dir.path().join("run.json");
    let spans_path = dir.path().join("completed.jsonl");

    let session = TracingSession::builder("svc")
        .sampler_interval(Duration::from_millis(1))
        .run_json_path(&run_path)
        .completed_span_jsonl_path(&spans_path)
        .build()
        .expect("start session");

    let subscriber = tracing_subscriber::registry().with(session.layer());
    tracing::subscriber::with_default(subscriber, || {
        tracing::info_span!(
            "request-source",
            tt.kind = "request",
            tt.request_id = "sampler-order-r1",
            tt.route = "/sampler-order",
            custom_source = "retained"
        )
        .in_scope(|| {});
    });

    wait_for_runtime_snapshot(&session).await;
    let before_shutdown = session.snapshot_run().expect("pre-shutdown snapshot");
    assert_eq!(before_shutdown.run().requests.len(), 1);
    assert!(!before_shutdown.run().runtime_snapshots.is_empty());
    assert!(!run_path.exists());
    assert!(!spans_path.exists());

    let returned = session.shutdown().await.expect("shutdown");
    let returned_run = returned.run();
    let returned_runtime_count = returned_run.runtime_snapshots.len();
    assert_eq!(returned_run.requests.len(), 1);
    assert_eq!(returned_run.requests[0].request_id, "sampler-order-r1");
    assert_eq!(returned_run.requests[0].route, "/sampler-order");
    assert!(returned_runtime_count >= 1);
    assert!(returned_run
        .metadata
        .effective_tokio_sampler_config
        .is_some());

    let written_run_text = std::fs::read_to_string(&run_path).expect("read run json");
    let written_run: tailtriage_core::Run =
        serde_json::from_str(&written_run_text).expect("decode run json");
    assert_eq!(&written_run, returned_run);
    assert_eq!(written_run.runtime_snapshots.len(), returned_runtime_count);
    assert_eq!(written_run.requests.len(), 1);

    let spans_text = std::fs::read_to_string(&spans_path).expect("read completed jsonl");
    let lines: Vec<_> = spans_text.lines().collect();
    assert_eq!(lines.len(), 1);
    let value: serde_json::Value = serde_json::from_str(lines[0]).expect("decode span jsonl");
    assert_eq!(value["format"], "tailtriage.tracing-span.v1");
    assert_eq!(value["span"]["name"], "request-source");
    assert_eq!(value["span"]["fields"]["tt.kind"], "request");
    assert_eq!(value["span"]["fields"]["tt.request_id"], "sampler-order-r1");
    assert_eq!(value["span"]["fields"]["custom_source"], "retained");
    assert!(value.get("runtime_snapshots").is_none());
    assert!(value["span"].get("runtime_snapshots").is_none());

    tokio::time::sleep(Duration::from_millis(20)).await;
    let run_after = std::fs::read_to_string(&run_path).expect("read run json after wait");
    let spans_after = std::fs::read_to_string(&spans_path).expect("read spans after wait");
    let decoded_after: tailtriage_core::Run =
        serde_json::from_str(&run_after).expect("decode run json after wait");
    assert_eq!(run_after, written_run_text);
    assert_eq!(spans_after, spans_text);
    assert_eq!(
        decoded_after.runtime_snapshots.len(),
        returned_runtime_count
    );
    assert_eq!(&decoded_after, returned_run);
}
