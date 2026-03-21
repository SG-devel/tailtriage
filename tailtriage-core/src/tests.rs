use std::future::ready;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::{
    BuildError, CaptureLimits, LocalJsonSink, Outcome, RequestEvent, RequestOptions, Run,
    RunMetadata, RunSink, RuntimeSnapshot, Tailtriage,
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
    let encoded = serde_json::to_string_pretty(&run).expect("serialize");
    assert!(encoded.contains("\"outcome\": \"ok\""));
    let decoded: Run = serde_json::from_str(&encoded).expect("deserialize");
    assert_eq!(decoded, run);
}

#[test]
fn local_json_sink_writes_pretty_json_file() {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("tailtriage_core_run_{nanos}.json"));
    let sink = LocalJsonSink::new(&path);
    sink.write(&sample_run()).expect("write");
    let written = std::fs::read_to_string(&path).expect("read");
    assert!(written.contains("\n  \"metadata\": {\n"));
    std::fs::remove_file(path).expect("cleanup");
}

#[test]
fn builder_rejects_blank_service_name() {
    let err = Tailtriage::builder("   ")
        .build()
        .expect_err("blank should fail");
    assert_eq!(err, BuildError::EmptyServiceName);
}

#[test]
fn builder_rejects_zero_sampling_interval() {
    let err = Tailtriage::builder("svc")
        .sampling(crate::SamplingConfig::runtime(Duration::ZERO))
        .build()
        .expect_err("zero interval should fail");
    assert_eq!(err, BuildError::InvalidRuntimeSamplingInterval);
}

#[test]
fn request_context_records_queue_stage_inflight_and_complete() {
    let tailtriage = Tailtriage::builder("payments").build().expect("build");
    let req = tailtriage.request("/invoice").with_kind("http");
    let id = req.request_id().to_owned();

    futures_executor::block_on(req.queue("q").await_on(ready(())));
    futures_executor::block_on(req.stage("db").await_value(ready(())));
    {
        let _guard = req.inflight("invoice_requests");
    }
    req.complete(Outcome::Ok);

    let run = tailtriage.snapshot();
    assert_eq!(run.requests.len(), 1);
    assert_eq!(run.requests[0].request_id, id);
    assert_eq!(run.requests[0].outcome, Outcome::Ok);
    assert_eq!(run.stages.len(), 1);
    assert_eq!(run.queues.len(), 1);
    assert_eq!(run.inflight.len(), 2);
}

#[test]
fn request_with_uses_caller_request_id() {
    let tailtriage = Tailtriage::builder("payments").build().expect("build");
    let req = tailtriage.request_with("/invoice", RequestOptions::new().request_id("req-custom-1"));
    req.complete(Outcome::Error);
    assert_eq!(tailtriage.snapshot().requests[0].request_id, "req-custom-1");
}

#[test]
fn request_generates_unique_ids() {
    let tailtriage = Tailtriage::builder("payments").build().expect("build");
    let first = tailtriage.request("/invoice");
    let second = tailtriage.request("/invoice");
    assert_ne!(first.request_id(), second.request_id());
}

#[test]
fn run_sugar_completes_request() {
    let tailtriage = Tailtriage::builder("payments").build().expect("build");
    let value = futures_executor::block_on(tailtriage.request("/a").run(Outcome::Ok, ready(7_u32)));
    assert_eq!(value, 7);
    assert_eq!(tailtriage.snapshot().requests[0].outcome, Outcome::Ok);
}

#[test]
fn truncation_still_applies() {
    let tailtriage = Tailtriage::builder("payments")
        .capture_limits(CaptureLimits {
            max_requests: 1,
            max_stages: 1,
            max_queues: 1,
            max_inflight_snapshots: 1,
            max_runtime_snapshots: 1,
        })
        .build()
        .expect("build");

    tailtriage.request("/a").complete(Outcome::Ok);
    tailtriage.request("/b").complete(Outcome::Ok);
    let req = tailtriage.request("/c");
    futures_executor::block_on(req.stage("s1").await_value(ready(())));
    futures_executor::block_on(req.stage("s2").await_value(ready(())));
    futures_executor::block_on(req.queue("q1").await_on(ready(())));
    futures_executor::block_on(req.queue("q2").await_on(ready(())));
    {
        let _g = req.inflight("g");
    }
    {
        let _g = req.inflight("g");
    }
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
    assert_eq!(run.truncation.dropped_requests, 1);
    assert_eq!(run.truncation.dropped_stages, 1);
    assert_eq!(run.truncation.dropped_queues, 1);
    assert_eq!(run.truncation.dropped_inflight_snapshots, 3);
    assert_eq!(run.truncation.dropped_runtime_snapshots, 1);
}

#[test]
fn shutdown_writes_artifact() {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let output = std::env::temp_dir().join(format!("tailtriage_core_shutdown_{nanos}.json"));
    let tailtriage = Tailtriage::builder("payments")
        .output(&output)
        .build()
        .unwrap();
    tailtriage.shutdown().expect("shutdown");
    assert!(std::fs::metadata(&output).is_ok());
    std::fs::remove_file(output).expect("cleanup");
}
