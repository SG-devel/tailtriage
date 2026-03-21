use std::future::ready;
use std::time::Duration;

use crate::{
    CaptureLimits, InitError, RequestEvent, Run, RunMetadata, RuntimeSnapshot, Tailtriage,
};

#[test]
fn empty_service_name_is_rejected() {
    let err = Tailtriage::builder(" ")
        .build()
        .expect_err("blank service_name should fail");
    assert_eq!(err, InitError::EmptyServiceName);
}

#[test]
fn builder_sets_basic_fields() {
    let tailtriage = Tailtriage::builder("payments")
        .investigation()
        .service_version("1.2.3")
        .run_id("run-42")
        .output(std::env::temp_dir().join("tailtriage_builder_sets_basic_fields.json"))
        .build()
        .expect("build should succeed");

    let run = tailtriage.snapshot();
    assert_eq!(run.metadata.service_name, "payments");
    assert_eq!(run.metadata.service_version.as_deref(), Some("1.2.3"));
    assert_eq!(run.metadata.run_id, "run-42");
}

#[test]
fn request_context_records_request_stage_queue_and_inflight() {
    let tailtriage = Tailtriage::builder("payments")
        .output(std::env::temp_dir().join("tailtriage_request_context_records.json"))
        .build()
        .expect("build should succeed");

    let request = tailtriage
        .request("/invoice")
        .request_id("req-42")
        .kind("http")
        .start();

    {
        let _inflight = request.inflight("invoice_inflight");
        futures_executor::block_on(
            request
                .queue("invoice_queue")
                .with_depth_at_start(8)
                .await_on(ready(())),
        );
        futures_executor::block_on(request.stage("db").await_value(ready(())));
    }

    request.finish("ok");

    let snapshot = tailtriage.snapshot();
    assert_eq!(snapshot.requests.len(), 1);
    assert_eq!(snapshot.requests[0].request_id, "req-42");
    assert_eq!(snapshot.requests[0].kind.as_deref(), Some("http"));
    assert_eq!(snapshot.stages.len(), 1);
    assert_eq!(snapshot.queues.len(), 1);
    assert_eq!(snapshot.inflight.len(), 2);
}

#[test]
fn request_context_run_is_sugar_over_finish() {
    let tailtriage = Tailtriage::builder("payments")
        .output(std::env::temp_dir().join("tailtriage_request_context_run_sugar.json"))
        .build()
        .expect("build should succeed");

    let value = futures_executor::block_on(
        tailtriage
            .request("/checkout")
            .request_id("req-77")
            .start()
            .run("ok", ready(7_u32)),
    );
    assert_eq!(value, 7);
    assert_eq!(tailtriage.snapshot().requests[0].request_id, "req-77");
}

#[test]
fn runtime_sampling_interval_round_trip() {
    let tailtriage = Tailtriage::builder("payments")
        .runtime_sampling_interval(Duration::from_millis(25))
        .output(std::env::temp_dir().join("tailtriage_runtime_sampling_interval.json"))
        .build()
        .expect("build should succeed");

    assert_eq!(
        tailtriage.runtime_sampling_interval(),
        Some(Duration::from_millis(25))
    );
}

#[test]
fn capture_limits_truncate_sections() {
    let limits = CaptureLimits {
        max_requests: 1,
        max_stages: 1,
        max_queues: 1,
        max_inflight_snapshots: 1,
        max_runtime_snapshots: 1,
    };

    let tailtriage = Tailtriage::builder("payments")
        .capture_limits(limits)
        .output(std::env::temp_dir().join("tailtriage_capture_limits_truncate_sections.json"))
        .build()
        .expect("build should succeed");

    let first = tailtriage.request("/invoice").request_id("req-1").start();
    futures_executor::block_on(first.stage("db").await_value(ready(())));
    futures_executor::block_on(first.queue("q").await_on(ready(())));
    let inflight = first.inflight("g");
    drop(inflight);
    first.finish("ok");

    let second = tailtriage.request("/invoice").request_id("req-2").start();
    futures_executor::block_on(second.stage("db").await_value(ready(())));
    futures_executor::block_on(second.queue("q").await_on(ready(())));
    second.finish("ok");

    tailtriage.record_runtime_snapshot(RuntimeSnapshot {
        at_unix_ms: 1,
        alive_tasks: None,
        global_queue_depth: None,
        local_queue_depth: None,
        blocking_queue_depth: None,
        remote_schedule_count: None,
    });
    tailtriage.record_runtime_snapshot(RuntimeSnapshot {
        at_unix_ms: 2,
        alive_tasks: None,
        global_queue_depth: None,
        local_queue_depth: None,
        blocking_queue_depth: None,
        remote_schedule_count: None,
    });

    let run = tailtriage.snapshot();
    assert_eq!(run.requests.len(), 1);
    assert_eq!(run.stages.len(), 1);
    assert_eq!(run.queues.len(), 1);
    assert_eq!(run.runtime_snapshots.len(), 1);
    assert!(run.truncation.is_truncated());
}

#[test]
fn shutdown_writes_run_file() {
    let output_path = std::env::temp_dir().join("tailtriage_shutdown_writes_run_file.json");
    let tailtriage = Tailtriage::builder("payments")
        .output(output_path.clone())
        .build()
        .expect("build should succeed");

    tailtriage
        .shutdown()
        .expect("shutdown should write run file");
    assert!(output_path.exists());
}

#[test]
fn request_event_shape_stays_compatible() {
    let event = RequestEvent {
        request_id: "req-1".to_string(),
        route: "/r".to_string(),
        kind: Some("http".to_string()),
        started_at_unix_ms: 1,
        finished_at_unix_ms: 2,
        latency_us: 3,
        outcome: "ok".to_string(),
    };

    assert_eq!(event.route, "/r");
    assert_eq!(event.kind.as_deref(), Some("http"));
}

#[test]
fn run_metadata_shape_stays_compatible() {
    let metadata = RunMetadata {
        run_id: "run-1".to_string(),
        service_name: "svc".to_string(),
        service_version: None,
        started_at_unix_ms: 1,
        finished_at_unix_ms: 2,
        mode: crate::CaptureMode::Light,
        host: None,
        pid: None,
    };
    let run = Run::new(metadata);
    assert!(run.requests.is_empty());
}
