use std::future::ready;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::{
    BuildError, CaptureLimits, Outcome, RequestOptions, Run, RunMetadata, RunSink, RuntimeSnapshot,
    SamplingConfig, SinkError, Tailtriage,
};

fn sample_run() -> Run {
    Run::new(RunMetadata {
        run_id: "run_123".to_owned(),
        service_name: "payments".to_owned(),
        service_version: Some("1.2.3".to_owned()),
        started_at_unix_ms: 1_000,
        finished_at_unix_ms: 3_000,
        mode: crate::CaptureMode::Light,
        host: Some("devbox".to_owned()),
        pid: Some(4242),
    })
}

#[test]
fn run_round_trips_with_json() {
    let run = sample_run();
    let encoded = serde_json::to_string_pretty(&run).expect("serialize");
    let decoded: Run = serde_json::from_str(&encoded).expect("deserialize");
    assert_eq!(decoded, run);
}

#[test]
fn builder_rejects_blank_service_name() {
    let err = Tailtriage::builder("   ").build().err();
    assert_eq!(err, Some(BuildError::EmptyServiceName));
}

#[test]
fn builder_rejects_zero_runtime_interval() {
    let err = Tailtriage::builder("payments")
        .sampling(SamplingConfig::runtime(Duration::ZERO))
        .build()
        .err();
    assert_eq!(err, Some(BuildError::InvalidRuntimeSamplingInterval));
}

#[test]
fn request_generates_unique_ids() {
    let collector = Tailtriage::builder("payments").build().expect("build");
    let first = collector.request("/invoice");
    let second = collector.request("/invoice");
    assert_ne!(first.request_id(), second.request_id());
}

#[test]
fn request_with_uses_caller_request_id() {
    let collector = Tailtriage::builder("payments").build().expect("build");
    let req = collector.request_with("/invoice", RequestOptions::new().request_id("req-42"));
    assert_eq!(req.request_id(), "req-42");
}

#[test]
fn context_records_kind_queue_stage_inflight_and_complete() {
    let collector = Tailtriage::builder("payments").build().expect("build");
    let req = collector.request("/invoice").with_kind("http");
    assert_eq!(req.kind(), Some("http"));

    futures_executor::block_on(req.queue("queue").await_on(ready(())));
    futures_executor::block_on(req.stage("db").await_value(ready(())));
    {
        let _guard = req.inflight("requests");
    }
    req.complete(Outcome::Ok);

    let run = collector.snapshot();
    assert_eq!(run.requests[0].outcome, "ok");
    assert_eq!(run.queues.len(), 1);
    assert_eq!(run.stages.len(), 1);
    assert_eq!(run.inflight.len(), 2);
}

#[test]
fn run_helper_completes_with_outcome() {
    let collector = Tailtriage::builder("payments").build().expect("build");
    let value = futures_executor::block_on(
        collector
            .request("/invoice")
            .run(Outcome::Error, ready(7_u32)),
    );
    assert_eq!(value, 7);
    assert_eq!(collector.snapshot().requests[0].outcome, "error");
}

#[test]
fn truncation_still_applies() {
    let collector = Tailtriage::builder("payments")
        .capture_limits(CaptureLimits {
            max_requests: 1,
            max_stages: 1,
            max_queues: 1,
            max_inflight_snapshots: 1,
            max_runtime_snapshots: 1,
        })
        .build()
        .expect("build");

    futures_executor::block_on(collector.request("/a").run(Outcome::Ok, ready(())));
    futures_executor::block_on(collector.request("/b").run(Outcome::Ok, ready(())));

    let req = collector.request("/c");
    futures_executor::block_on(req.stage("s1").await_value(ready(())));
    futures_executor::block_on(req.stage("s2").await_value(ready(())));
    futures_executor::block_on(req.queue("q1").await_on(ready(())));
    futures_executor::block_on(req.queue("q2").await_on(ready(())));
    {
        let _guard = req.inflight("g");
    }
    {
        let _guard = req.inflight("g");
    }

    collector.record_runtime_snapshot(RuntimeSnapshot {
        at_unix_ms: 1,
        alive_tasks: Some(1),
        global_queue_depth: None,
        local_queue_depth: None,
        blocking_queue_depth: None,
        remote_schedule_count: None,
    });
    collector.record_runtime_snapshot(RuntimeSnapshot {
        at_unix_ms: 2,
        alive_tasks: Some(2),
        global_queue_depth: None,
        local_queue_depth: None,
        blocking_queue_depth: None,
        remote_schedule_count: None,
    });

    let run = collector.snapshot();
    assert_eq!(run.requests.len(), 1);
    assert_eq!(run.stages.len(), 1);
    assert_eq!(run.queues.len(), 1);
    assert_eq!(run.inflight.len(), 1);
    assert_eq!(run.runtime_snapshots.len(), 1);
}

#[test]
fn shutdown_writes_artifact() {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before epoch")
        .as_nanos();
    let output_path = std::env::temp_dir().join(format!("tailtriage_core_shutdown_{nanos}.json"));

    let collector = Tailtriage::builder("payments")
        .output(&output_path)
        .build()
        .expect("build");
    collector.shutdown().expect("shutdown should write");

    let bytes = std::fs::metadata(&output_path).expect("exists").len();
    assert!(bytes > 0);
    std::fs::remove_file(output_path).expect("cleanup");
}

#[derive(Debug)]
struct InMemorySink;

impl RunSink for InMemorySink {
    fn write(&self, _run: &Run) -> Result<(), SinkError> {
        Ok(())
    }
}

#[test]
fn custom_sink_can_be_configured() {
    let collector = Tailtriage::builder("payments")
        .sink(InMemorySink)
        .build()
        .expect("build");
    collector.shutdown().expect("shutdown with custom sink");
}
