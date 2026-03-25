use std::future::ready;
#[cfg(debug_assertions)]
use std::panic::AssertUnwindSafe;
use std::sync::{Arc, Mutex};

use crate::{BuildError, CaptureLimits, Outcome, RequestOptions, SinkError, Tailtriage};

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
fn started_request_records_request_event() {
    let tailtriage = build_for_test("payments", "tailtriage-core-request.json");
    let started = tailtriage
        .begin_request_with(
            "/invoice",
            RequestOptions::new().request_id("req-42").kind("http"),
        )
        .with_kind("http");
    let request = started.handle;
    assert_eq!(request.route(), "/invoice");
    assert_eq!(request.kind(), Some("http"));
    futures_executor::block_on(request.stage("db").await_value(ready(())));
    started.completion.finish_ok();

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
    let first = tailtriage.begin_request("/invoice");
    let second = tailtriage.begin_request("/invoice");
    assert_ne!(first.handle.request_id(), second.handle.request_id());
    first.completion.finish_ok();
    second.completion.finish_ok();
}

#[test]
fn queue_stage_and_inflight_are_recorded() {
    let tailtriage = build_for_test("payments", "tailtriage-core-timers.json");
    let started =
        tailtriage.begin_request_with("/invoice", RequestOptions::new().request_id("req-9"));
    let request = started.handle;
    {
        let _inflight = request.inflight("invoice_inflight");
        futures_executor::block_on(request.queue("permit").await_on(ready(())));
        let _: Result<(), ()> =
            futures_executor::block_on(request.stage("persist").await_on(ready(Ok(()))));
    }
    started.completion.finish_ok();

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

    tailtriage.begin_request("/health").completion.finish_ok();
    tailtriage.shutdown().expect("shutdown should succeed");

    let bytes = std::fs::read(output).expect("artifact should exist");
    let run: crate::Run = serde_json::from_slice(&bytes).expect("artifact should deserialize");
    assert_eq!(run.schema_version, crate::SCHEMA_VERSION);
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

    let first =
        tailtriage.begin_request_with("/invoice", RequestOptions::new().request_id("req-1"));
    futures_executor::block_on(first.handle.stage("db").await_value(ready(())));
    futures_executor::block_on(first.handle.queue("q").await_on(ready(())));
    {
        let _guard = first.handle.inflight("g");
    }
    first.completion.finish_ok();

    let second =
        tailtriage.begin_request_with("/invoice", RequestOptions::new().request_id("req-2"));
    futures_executor::block_on(second.handle.stage("db").await_value(ready(())));
    futures_executor::block_on(second.handle.queue("q").await_on(ready(())));
    second.completion.finish_ok();
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
    tailtriage
        .begin_request("/finish")
        .completion
        .finish(Outcome::Ok);

    let snapshot = tailtriage.snapshot();
    assert_eq!(snapshot.requests.len(), 1);
    assert_eq!(snapshot.requests[0].outcome, "ok");
}

#[test]
fn finish_result_maps_result_to_request_outcome() {
    let tailtriage = build_for_test("payments", "tailtriage-core-finish-result.json");

    let ok_value = tailtriage
        .begin_request("/finish-result-ok")
        .completion
        .finish_result(Ok::<u8, &'static str>(3))
        .expect("ok result should remain ok");
    assert_eq!(ok_value, 3);

    let err = tailtriage
        .begin_request("/finish-result-err")
        .completion
        .finish_result::<u8, _>(Err("boom"))
        .expect_err("err result should remain err");
    assert_eq!(err, "boom");

    let snapshot = tailtriage.snapshot();
    assert_eq!(snapshot.requests.len(), 2);
    assert_eq!(snapshot.requests[0].outcome, "ok");
    assert_eq!(snapshot.requests[1].outcome, "error");
}

async fn stage_in_helper_layer(
    request: &crate::RequestHandle<'_>,
    stage_name: &str,
) -> Result<(), &'static str> {
    request
        .stage(stage_name)
        .await_on(ready(Ok::<(), &'static str>(())))
        .await
}

#[test]
fn request_handle_supports_fractured_code_usage() {
    let tailtriage = build_for_test("payments", "tailtriage-core-fractured.json");
    let started = tailtriage.begin_request_with(
        "/fractured",
        RequestOptions::new()
            .request_id("req-fractured")
            .kind("http"),
    );
    let request = started.handle.clone();

    futures_executor::block_on(request.queue("q").await_on(ready(())));
    futures_executor::block_on(stage_in_helper_layer(&request, "layer_a"))
        .expect("helper stage should succeed");
    futures_executor::block_on(stage_in_helper_layer(&request, "layer_b"))
        .expect("helper stage should succeed");
    started.completion.finish_ok();

    let snapshot = tailtriage.snapshot();
    assert_eq!(snapshot.requests.len(), 1);
    assert_eq!(snapshot.stages.len(), 2);
    assert_eq!(snapshot.queues.len(), 1);
}

#[test]
fn shutdown_warns_with_unfinished_requests() {
    let tailtriage = build_for_test("payments", "tailtriage-core-unfinished.json");
    let started = tailtriage.begin_request("/unfinished");
    std::mem::forget(started.completion);

    tailtriage.shutdown().expect("shutdown should succeed");
    let snapshot = tailtriage.snapshot();

    assert_eq!(snapshot.requests.len(), 0);
    assert_eq!(snapshot.metadata.unfinished_requests.count, 1);
    assert_eq!(snapshot.metadata.unfinished_requests.sample.len(), 1);
    assert!(!snapshot.metadata.lifecycle_warnings.is_empty());
}

#[test]
fn strict_lifecycle_fails_shutdown_with_unfinished_requests() {
    let tailtriage = Tailtriage::builder("payments")
        .strict_lifecycle(true)
        .build()
        .expect("build should succeed");
    let started = tailtriage.begin_request("/unfinished");
    std::mem::forget(started.completion);

    let error = tailtriage.shutdown().expect_err("strict mode should fail");
    assert!(matches!(
        error,
        SinkError::Lifecycle {
            unfinished_count: 1
        }
    ));
}

#[test]
fn custom_sink_receives_shutdown_run() {
    let sink = Arc::new(RecordingSink::default());
    let tailtriage = Tailtriage::builder("payments")
        .sink(Arc::clone(&sink))
        .build()
        .expect("build should succeed");

    tailtriage
        .begin_request("/sink-test")
        .completion
        .finish_ok();
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
fn dropping_unfinished_completion_panics_in_debug() {
    let tailtriage = build_for_test("payments", "tailtriage-core-drop-unfinished.json");
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let _started = tailtriage.begin_request("/unfinished");
    }));
    assert!(
        result.is_err(),
        "unfinished completion should panic in debug"
    );
}
