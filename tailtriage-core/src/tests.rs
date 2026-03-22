use std::future::ready;
use std::panic::AssertUnwindSafe;
use std::sync::{Arc, Mutex};

use crate::{BuildError, CaptureLimits, Outcome, RequestOptions, Tailtriage};

#[derive(Debug, Default)]
struct RecordingSink {
    run: Mutex<Option<crate::Run>>,
}

impl crate::RunSink for Arc<RecordingSink> {
    fn write(&self, run: &crate::Run) -> Result<(), crate::SinkError> {
        let mut guard = self.run.lock().expect("lock should succeed");
        *guard = Some(run.clone());
        Ok(())
    }
}

fn build_for_test(name: &str, filename: &str) -> Tailtriage {
    Tailtriage::builder(name)
        .output(std::env::temp_dir().join(filename))
        .build()
        .expect("build should succeed")
}

#[test]
fn rejects_blank_service_name() {
    let err = Tailtriage::builder("   ")
        .build()
        .expect_err("blank service_name should fail");
    assert_eq!(err, BuildError::EmptyServiceName);
}

#[test]
fn request_context_records_request_event() {
    let tailtriage = build_for_test("payments", "tailtriage-core-request.json");
    let request = tailtriage
        .request_with("/invoice", RequestOptions::new().request_id("req-42"))
        .with_kind("http");
    assert_eq!(request.route(), "/invoice");
    assert_eq!(request.kind(), Some("http"));
    futures_executor::block_on(request.stage("db").await_value(ready(())));
    request.finish_ok();

    let snapshot = tailtriage.snapshot();
    assert_eq!(snapshot.requests.len(), 1);
    assert_eq!(snapshot.requests[0].request_id, "req-42");
    assert_eq!(snapshot.requests[0].route, "/invoice");
    assert_eq!(snapshot.requests[0].kind.as_deref(), Some("http"));
    assert_eq!(snapshot.requests[0].outcome, "ok");
    assert_eq!(snapshot.stages.len(), 1);
}

#[test]
fn generated_request_ids_are_unique() {
    let tailtriage = build_for_test("payments", "tailtriage-core-generated-id.json");
    let first = tailtriage.request("/invoice");
    let second = tailtriage.request("/invoice");
    assert_ne!(first.request_id(), second.request_id());
    first.finish_ok();
    second.finish_ok();
}

#[test]
fn queue_stage_and_inflight_are_recorded() {
    let tailtriage = build_for_test("payments", "tailtriage-core-timers.json");
    let request = tailtriage.request_with("/invoice", RequestOptions::new().request_id("req-9"));
    {
        let _inflight = request.inflight("invoice_inflight");
        futures_executor::block_on(request.queue("permit").await_on(ready(())));
        let _: Result<(), ()> =
            futures_executor::block_on(request.stage("persist").await_on(ready(Ok(()))));
    }
    request.finish_ok();

    let snapshot = tailtriage.snapshot();
    assert_eq!(snapshot.inflight.len(), 2);
    assert_eq!(snapshot.queues.len(), 1);
    assert_eq!(snapshot.stages.len(), 1);
}

#[test]
fn shutdown_writes_artifact() {
    let output = std::env::temp_dir().join("tailtriage-core-shutdown.json");
    let tailtriage = Tailtriage::builder("payments")
        .output(&output)
        .build()
        .expect("build should succeed");

    let request = tailtriage.request("/health");
    request.finish_ok();
    tailtriage.shutdown().expect("shutdown should succeed");

    let bytes = std::fs::read(output).expect("artifact should exist");
    let run: crate::Run = serde_json::from_slice(&bytes).expect("artifact should deserialize");
    assert_eq!(run.requests.len(), 1);
}

#[test]
fn capture_limits_apply_to_all_sections() {
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
        .expect("build should succeed");

    let first = tailtriage.request_with("/invoice", RequestOptions::new().request_id("req-1"));
    futures_executor::block_on(first.stage("db").await_value(ready(())));
    futures_executor::block_on(first.queue("q").await_on(ready(())));
    {
        let _guard = first.inflight("g");
    }
    first.finish_ok();

    let second = tailtriage.request_with("/invoice", RequestOptions::new().request_id("req-2"));
    futures_executor::block_on(second.stage("db").await_value(ready(())));
    futures_executor::block_on(second.queue("q").await_on(ready(())));
    second.finish_ok();
    tailtriage.record_runtime_snapshot(crate::RuntimeSnapshot {
        at_unix_ms: crate::unix_time_ms(),
        alive_tasks: Some(1),
        global_queue_depth: Some(1),
        local_queue_depth: None,
        blocking_queue_depth: None,
        remote_schedule_count: None,
    });
    tailtriage.record_runtime_snapshot(crate::RuntimeSnapshot {
        at_unix_ms: crate::unix_time_ms(),
        alive_tasks: Some(2),
        global_queue_depth: Some(2),
        local_queue_depth: None,
        blocking_queue_depth: None,
        remote_schedule_count: None,
    });

    let snapshot = tailtriage.snapshot();
    assert_eq!(snapshot.requests.len(), 1);
    assert_eq!(snapshot.stages.len(), 1);
    assert_eq!(snapshot.queues.len(), 1);
    assert_eq!(snapshot.inflight.len(), 1);
    assert_eq!(snapshot.runtime_snapshots.len(), 1);
}

#[test]
fn finish_records_outcome() {
    let tailtriage = build_for_test("payments", "tailtriage-core-finish.json");
    tailtriage.request("/finish").finish(Outcome::Ok);

    let snapshot = tailtriage.snapshot();
    assert_eq!(snapshot.requests.len(), 1);
    assert_eq!(snapshot.requests[0].outcome, "ok");
}

#[test]
fn finish_ok_records_ok_outcome() {
    let tailtriage = build_for_test("payments", "tailtriage-core-finish-ok.json");

    tailtriage.request("/finish-ok").finish_ok();

    let snapshot = tailtriage.snapshot();
    assert_eq!(snapshot.requests.len(), 1);
    assert_eq!(snapshot.requests[0].outcome, "ok");
}

#[test]
fn finish_result_maps_result_to_request_outcome() {
    let tailtriage = build_for_test("payments", "tailtriage-core-finish-result.json");

    let ok_value = tailtriage
        .request("/finish-result-ok")
        .finish_result(Ok::<u8, &'static str>(3))
        .expect("ok result should remain ok");
    assert_eq!(ok_value, 3);

    let err = tailtriage
        .request("/finish-result-err")
        .finish_result::<u8, _>(Err("boom"))
        .expect_err("err result should remain err");
    assert_eq!(err, "boom");

    let snapshot = tailtriage.snapshot();
    assert_eq!(snapshot.requests.len(), 2);
    assert_eq!(snapshot.requests[0].outcome, "ok");
    assert_eq!(snapshot.requests[1].outcome, "error");
}

async fn stage_in_helper_layer(
    request: &crate::RequestContext<'_>,
    stage_name: &str,
) -> Result<(), &'static str> {
    request
        .stage(stage_name)
        .await_on(ready(Ok::<(), &'static str>(())))
        .await
}

#[test]
fn request_context_supports_fractured_code_usage() {
    let tailtriage = build_for_test("payments", "tailtriage-core-fractured.json");
    let request = tailtriage
        .request_with(
            "/fractured",
            RequestOptions::new().request_id("req-fractured"),
        )
        .with_kind("http");

    futures_executor::block_on(request.queue("q").await_on(ready(())));
    futures_executor::block_on(stage_in_helper_layer(&request, "layer_a"))
        .expect("helper stage should succeed");
    futures_executor::block_on(stage_in_helper_layer(&request, "layer_b"))
        .expect("helper stage should succeed");
    request.finish_ok();

    let snapshot = tailtriage.snapshot();
    assert_eq!(snapshot.requests.len(), 1);
    assert_eq!(snapshot.stages.len(), 2);
    assert_eq!(snapshot.queues.len(), 1);
}

#[test]
fn custom_sink_receives_shutdown_run() {
    let sink = Arc::new(RecordingSink::default());
    let tailtriage = Tailtriage::builder("payments")
        .sink(Arc::clone(&sink))
        .build()
        .expect("build should succeed");

    tailtriage.request("/sink-test").finish_ok();
    tailtriage.shutdown().expect("shutdown should succeed");

    let stored = sink
        .run
        .lock()
        .expect("lock should succeed")
        .clone()
        .expect("sink should receive run");
    assert_eq!(stored.requests.len(), 1);
}

#[cfg(debug_assertions)]
#[test]
fn dropping_unfinished_request_panics_in_debug() {
    let tailtriage = build_for_test("payments", "tailtriage-core-drop-unfinished.json");
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let _request = tailtriage.request("/unfinished");
    }));
    assert!(result.is_err(), "unfinished request should panic in debug");
}
