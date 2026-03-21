use std::future::ready;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{
    CaptureLimits, CaptureMode, InFlightSnapshot, InitError, LocalJsonSink, QueueEvent,
    RequestEvent, Run, RunMetadata, RunSink, RuntimeSnapshot, StageEvent, Tailtriage,
};

fn sample_run() -> Run {
    let metadata = RunMetadata {
        run_id: "run_123".to_owned(),
        service_name: "payments".to_owned(),
        service_version: Some("1.2.3".to_owned()),
        started_at_unix_ms: 1_000,
        finished_at_unix_ms: 3_000,
        mode: CaptureMode::Light,
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
        outcome: "ok".to_owned(),
    });
    run.stages.push(StageEvent {
        request_id: "req-1".to_owned(),
        stage: "persist_invoice".to_owned(),
        started_at_unix_ms: 1_220,
        finished_at_unix_ms: 1_350,
        latency_us: 130_000,
        success: true,
    });
    run.queues.push(QueueEvent {
        request_id: "req-1".to_owned(),
        queue: "invoice_worker".to_owned(),
        waited_from_unix_ms: 1_105,
        waited_until_unix_ms: 1_120,
        wait_us: 15_000,
        depth_at_start: Some(7),
    });
    run.inflight.push(InFlightSnapshot {
        gauge: "invoice_requests".to_owned(),
        at_unix_ms: 1_200,
        count: 42,
    });
    run.runtime_snapshots.push(RuntimeSnapshot {
        at_unix_ms: 1_250,
        alive_tasks: Some(130),
        global_queue_depth: Some(18),
        local_queue_depth: Some(12),
        blocking_queue_depth: Some(4),
        remote_schedule_count: Some(44),
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
fn init_rejects_blank_service_name() {
    let err = Tailtriage::builder("   ")
        .build()
        .expect_err("blank service_name should fail");
    assert_eq!(err, InitError::EmptyServiceName);
}

#[test]
fn request_context_records_timing_and_outcome() {
    let tailtriage = Tailtriage::builder("payments")
        .build()
        .expect("init should succeed");

    let ctx = tailtriage
        .begin_request("/invoice")
        .with_request_id("req-42")
        .kind("create_invoice");
    futures_executor::block_on(ctx.run("ok", ready(())));

    let snapshot = tailtriage.snapshot();
    assert_eq!(snapshot.requests.len(), 1);
    let event = &snapshot.requests[0];
    assert_eq!(event.request_id, "req-42");
    assert_eq!(event.kind.as_deref(), Some("create_invoice"));
    assert_eq!(event.outcome, "ok");
}

#[test]
fn shutdown_writes_current_snapshot() {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before epoch")
        .as_nanos();
    let output_path = std::env::temp_dir().join(format!("tailtriage_core_shutdown_{nanos}.json"));

    let tailtriage = Tailtriage::builder("payments")
        .output(output_path.clone())
        .build()
        .expect("init should succeed");
    tailtriage
        .shutdown()
        .expect("shutdown should write run file");

    let bytes = std::fs::metadata(&output_path)
        .expect("shutdown output should exist")
        .len();
    assert!(bytes > 0);
    std::fs::remove_file(output_path).expect("temp run file should be removable");
}

#[test]
fn queue_stage_and_inflight_attach_to_request_context() {
    let tailtriage = Tailtriage::builder("payments")
        .build()
        .expect("init should succeed");

    let ctx = tailtriage
        .begin_request("/invoice")
        .with_request_id("req-1");
    {
        let _guard = ctx.inflight("invoice_requests");
        futures_executor::block_on(ctx.queue("q").with_depth_at_start(3).await_on(ready(())));
        let result = futures_executor::block_on(
            ctx.stage("db")
                .await_on(ready::<Result<u32, &'static str>>(Ok(17))),
        );
        assert_eq!(result, Ok(17));
    }
    ctx.complete("ok");

    let run = tailtriage.snapshot();
    assert_eq!(run.inflight.len(), 2);
    assert_eq!(run.queues.len(), 1);
    assert_eq!(run.stages.len(), 1);
    assert_eq!(run.requests.len(), 1);
}

#[test]
fn capture_limits_drop_events_and_track_truncation_counters() {
    let tailtriage = Tailtriage::builder("payments")
        .capture_limits(CaptureLimits {
            max_requests: 1,
            max_stages: 1,
            max_queues: 1,
            max_inflight_snapshots: 1,
            max_runtime_snapshots: 1,
        })
        .build()
        .expect("init should succeed");

    let a = tailtriage.begin_request("/a").with_request_id("req-1");
    futures_executor::block_on(a.stage("db").await_value(ready(())));
    futures_executor::block_on(a.queue("q").await_on(ready(())));
    {
        let _guard = a.inflight("g");
    }
    a.complete("ok");

    let b = tailtriage.begin_request("/b").with_request_id("req-2");
    futures_executor::block_on(b.stage("cache").await_value(ready(())));
    futures_executor::block_on(b.queue("q2").await_on(ready(())));
    {
        let _guard = b.inflight("g");
    }
    b.complete("ok");

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
    assert_eq!(run.requests.len(), 1);
    assert_eq!(run.stages.len(), 1);
    assert_eq!(run.queues.len(), 1);
    assert_eq!(run.inflight.len(), 1);
    assert_eq!(run.runtime_snapshots.len(), 1);
}

#[test]
fn local_json_sink_writes_pretty_json_file() {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before epoch")
        .as_nanos();

    let path = std::env::temp_dir().join(format!("tailtriage_core_run_{nanos}.json"));
    let sink = LocalJsonSink::new(&path);

    let run = sample_run();
    sink.write(&run).expect("sink should write run JSON");

    let written = std::fs::read_to_string(&path).expect("written file should exist");
    assert!(written.contains("\n  \"metadata\": {\n"));

    let decoded: Run = serde_json::from_str(&written).expect("written JSON should parse");
    assert_eq!(decoded, run);

    std::fs::remove_file(path).expect("temp run file should be removable");
}
