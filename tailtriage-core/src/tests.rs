use std::future::ready;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{
    CaptureLimits, Outcome, RequestEvent, RequestOptions, Run, RunMetadata, RuntimeSnapshot,
    Tailtriage,
};

fn sample_run() -> Run {
    let metadata = RunMetadata {
        run_id: "run_123".to_owned(),
        service_name: "payments".to_owned(),
        service_version: Some("1.2.3".to_owned()),
        started_at_unix_ms: 1_000,
        finished_at_unix_ms: 3_000,
        mode: crate::CaptureMode::Light,
        host: Some("devbox".to_owned()),
        pid: Some(4242),
    };

    let mut run = Run::new(metadata);
    run.requests.push(RequestEvent {
        request_id: "req-1".to_owned(),
        route: "/invoice".to_owned(),
        kind: Some("create_invoice".to_owned()),
        started_at_unix_ms: 1_100,
        finished_at_unix_ms: 1_400,
        latency_us: 300_000,
        outcome: Outcome::Ok,
    });
    run
}

#[test]
fn run_round_trips_with_json() {
    let run = sample_run();
    let encoded = serde_json::to_string_pretty(&run).expect("run should serialize");
    let decoded: Run = serde_json::from_str(&encoded).expect("run should deserialize");
    assert_eq!(decoded, run);
}

#[test]
fn builder_rejects_blank_service_name() {
    let err = Tailtriage::builder(" ").build().err();
    assert_eq!(err, Some(crate::BuildError::EmptyServiceName));
}

#[test]
fn request_records_outcome() {
    let tailtriage = Tailtriage::builder("payments").build().expect("build");
    let request = tailtriage.request("/invoice").with_kind("http");
    let value = futures_executor::block_on(request.run(Outcome::Ok, ready(7_u32)));
    assert_eq!(value, 7);
    let run = tailtriage.snapshot();
    assert_eq!(run.requests[0].outcome, Outcome::Ok);
}

#[test]
fn request_with_uses_provided_id() {
    let tailtriage = Tailtriage::builder("payments").build().expect("build");
    let req = tailtriage.request_with("/invoice", RequestOptions::new().request_id("req-42"));
    req.complete(Outcome::Error);
    let run = tailtriage.snapshot();
    assert_eq!(run.requests[0].request_id, "req-42");
}

#[test]
fn shutdown_writes_current_snapshot() {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before epoch")
        .as_nanos();

    let output_path = std::env::temp_dir().join(format!("tailtriage_core_flush_{nanos}.json"));
    let tailtriage = Tailtriage::builder("payments")
        .output(&output_path)
        .build()
        .expect("build should succeed");
    tailtriage
        .shutdown()
        .expect("shutdown should write run file");
    std::fs::remove_file(output_path).expect("cleanup");
}

#[test]
fn capture_limits_track_truncation() {
    let limits = CaptureLimits {
        max_requests: 1,
        max_stages: 1,
        max_queues: 1,
        max_inflight_snapshots: 1,
        max_runtime_snapshots: 1,
    };
    let tailtriage = Tailtriage::builder("payments")
        .capture_limits(limits)
        .build()
        .expect("build");

    tailtriage.record_request_fields("req-1", "/a", None, (1, 2), 10, "ok");
    tailtriage.record_request_fields("req-2", "/b", None, (1, 2), 10, "ok");
    futures_executor::block_on(tailtriage.stage("req-1", "db").await_value(ready(())));
    futures_executor::block_on(tailtriage.stage("req-1", "cache").await_value(ready(())));
    futures_executor::block_on(tailtriage.queue("req-1", "q").await_on(ready(())));
    futures_executor::block_on(tailtriage.queue("req-1", "q2").await_on(ready(())));
    let guard = tailtriage.inflight("g");
    drop(guard);
    tailtriage.record_runtime_snapshot(RuntimeSnapshot {
        at_unix_ms: 1,
        alive_tasks: Some(1),
        global_queue_depth: None,
        local_queue_depth: None,
        blocking_queue_depth: None,
        remote_schedule_count: None,
    });
    tailtriage.record_runtime_snapshot(RuntimeSnapshot {
        at_unix_ms: 2,
        alive_tasks: Some(2),
        global_queue_depth: None,
        local_queue_depth: None,
        blocking_queue_depth: None,
        remote_schedule_count: None,
    });

    let run = tailtriage.snapshot();
    assert!(run.truncation.is_truncated());
}
