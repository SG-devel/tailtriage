use std::future::ready;
#[cfg(debug_assertions)]
use std::panic::AssertUnwindSafe;
use std::sync::{Arc, Mutex};

use crate::{
    BuildError, CaptureLimits, CaptureLimitsOverride, CaptureMode, EffectiveTokioSamplerConfig,
    Outcome, RequestOptions, RuntimeSamplerRegistrationError, SinkError, Tailtriage,
};

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
    let started = tailtriage.begin_request_with(
        "/invoice",
        RequestOptions::new().request_id("req-42").kind("http"),
    );
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
fn duplicate_explicit_request_ids_are_tracked_and_finished_independently() {
    let tailtriage = build_for_test("payments", "tailtriage-core-duplicate-explicit-id.json");
    let first = tailtriage.begin_request_with(
        "/invoice",
        RequestOptions::new().request_id("req-duplicate"),
    );
    let second = tailtriage.begin_request_with(
        "/invoice",
        RequestOptions::new().request_id("req-duplicate"),
    );

    first.completion.finish_ok();
    second.completion.finish_ok();

    let snapshot = tailtriage.snapshot();
    assert_eq!(snapshot.requests.len(), 2);
    assert_eq!(snapshot.requests[0].request_id, "req-duplicate");
    assert_eq!(snapshot.requests[1].request_id, "req-duplicate");
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
fn unfinished_requests_still_trigger_strict_lifecycle_errors() {
    let sink = Arc::new(RecordingSink::default());
    let tailtriage = Tailtriage::builder("payments")
        .sink(Arc::clone(&sink))
        .strict_lifecycle(true)
        .build()
        .expect("build should succeed");

    let started = tailtriage.begin_request("/invoice");
    std::mem::forget(started.completion);

    let err = tailtriage
        .shutdown()
        .expect_err("unfinished request should fail strict lifecycle mode");
    assert!(matches!(
        err,
        SinkError::Lifecycle {
            unfinished_count: 1
        }
    ));
    let run = sink.run.lock().expect("sink lock should succeed");
    assert!(
        run.is_none(),
        "strict lifecycle error should prevent sink write"
    );
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
    assert_eq!(run.metadata.mode, CaptureMode::Light);
    assert_eq!(
        run.metadata
            .effective_core_config
            .expect("effective core config should be present for new runs")
            .capture_limits,
        CaptureMode::Light.core_defaults()
    );
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
fn mode_defaults_differ_between_light_and_investigation() {
    assert_ne!(
        CaptureMode::Light.core_defaults(),
        CaptureMode::Investigation.core_defaults()
    );
}

#[test]
fn capture_limits_remains_full_override() {
    let full_override = CaptureLimits {
        max_requests: 10,
        max_stages: 20,
        max_queues: 30,
        max_inflight_snapshots: 40,
        max_runtime_snapshots: 50,
    };
    let tailtriage = Tailtriage::builder("payments")
        .investigation()
        .capture_limits_override(CaptureLimitsOverride {
            max_requests: Some(999),
            ..CaptureLimitsOverride::default()
        })
        .capture_limits(full_override)
        .build()
        .expect("build should succeed");

    assert_eq!(
        tailtriage.effective_core_config().capture_limits,
        full_override
    );
}

#[test]
fn capture_limits_override_applies_selected_fields_on_mode_defaults() {
    let tailtriage = Tailtriage::builder("payments")
        .investigation()
        .capture_limits_override(CaptureLimitsOverride {
            max_requests: Some(12_345),
            max_runtime_snapshots: Some(88),
            ..CaptureLimitsOverride::default()
        })
        .build()
        .expect("build should succeed");

    let mut expected = CaptureMode::Investigation.core_defaults();
    expected.max_requests = 12_345;
    expected.max_runtime_snapshots = 88;
    assert_eq!(tailtriage.effective_core_config().capture_limits, expected);
}

#[test]
fn strict_lifecycle_is_unchanged_by_mode() {
    let light = Tailtriage::builder("payments")
        .light()
        .strict_lifecycle(true)
        .build()
        .expect("build should succeed");
    let investigation = Tailtriage::builder("payments")
        .investigation()
        .strict_lifecycle(true)
        .build()
        .expect("build should succeed");

    assert!(light.effective_core_config().strict_lifecycle);
    assert!(investigation.effective_core_config().strict_lifecycle);
}

#[test]
fn selected_mode_and_effective_config_are_preserved_in_metadata() {
    let tailtriage = Tailtriage::builder("payments")
        .investigation()
        .capture_limits_override(CaptureLimitsOverride {
            max_queues: Some(7),
            ..CaptureLimitsOverride::default()
        })
        .build()
        .expect("build should succeed");

    let snapshot = tailtriage.snapshot();
    assert_eq!(tailtriage.selected_mode(), CaptureMode::Investigation);
    assert_eq!(snapshot.metadata.mode, CaptureMode::Investigation);
    assert_eq!(
        snapshot.metadata.effective_core_config,
        Some(tailtriage.effective_core_config())
    );
    assert_eq!(
        snapshot
            .metadata
            .effective_core_config
            .expect("effective core config should be present for new runs")
            .capture_limits
            .max_queues,
        7
    );
}

#[test]
fn runtime_sampler_registration_is_single_start_and_metadata_cannot_be_overwritten() {
    let tailtriage = Tailtriage::builder("payments")
        .build()
        .expect("build should succeed");
    let first = EffectiveTokioSamplerConfig {
        inherited_mode: CaptureMode::Light,
        explicit_mode_override: None,
        resolved_mode: CaptureMode::Light,
        resolved_sampler_cadence_ms: 500,
        resolved_runtime_snapshot_retention: 5_000,
    };
    let second = EffectiveTokioSamplerConfig {
        inherited_mode: CaptureMode::Light,
        explicit_mode_override: Some(CaptureMode::Investigation),
        resolved_mode: CaptureMode::Investigation,
        resolved_sampler_cadence_ms: 100,
        resolved_runtime_snapshot_retention: 50_000,
    };

    tailtriage
        .register_tokio_runtime_sampler(first)
        .expect("first sampler registration should succeed");
    let err = tailtriage
        .register_tokio_runtime_sampler(second)
        .expect_err("duplicate registration should fail");
    assert_eq!(err, RuntimeSamplerRegistrationError::DuplicateStart);

    let snapshot = tailtriage.snapshot();
    assert_eq!(
        snapshot.metadata.effective_tokio_sampler_config,
        Some(first)
    );
}

#[test]
fn limit_hit_flag_is_set_when_truncation_occurs() {
    let tailtriage = Tailtriage::builder("payments")
        .capture_limits(CaptureLimits {
            max_requests: 1,
            max_stages: 1,
            max_queues: 1,
            max_inflight_snapshots: 1,
            max_runtime_snapshots: 1,
        })
        .build()
        .expect("build should succeed");

    tailtriage.begin_request("/a").completion.finish_ok();
    tailtriage.begin_request("/b").completion.finish_ok();

    let snapshot = tailtriage.snapshot();
    assert!(snapshot.truncation.limits_hit);
    assert!(snapshot.truncation.dropped_requests > 0);
}

#[test]
fn saturation_preserves_exact_drop_counts_across_sections() {
    let tailtriage = Tailtriage::builder("payments")
        .capture_limits(CaptureLimits {
            max_requests: 1,
            max_stages: 1,
            max_queues: 1,
            max_inflight_snapshots: 1,
            max_runtime_snapshots: 1,
        })
        .build()
        .expect("build should succeed");

    let first = tailtriage.begin_request("/invoice");
    futures_executor::block_on(first.handle.stage("db").await_value(ready(())));
    futures_executor::block_on(first.handle.stage("cache").await_value(ready(())));
    futures_executor::block_on(first.handle.stage("serialize").await_value(ready(())));
    futures_executor::block_on(first.handle.queue("permit").await_on(ready(())));
    futures_executor::block_on(first.handle.queue("backend").await_on(ready(())));
    futures_executor::block_on(first.handle.queue("egress").await_on(ready(())));
    {
        let _inflight = first.handle.inflight("requests");
    }
    {
        let _inflight = first.handle.inflight("requests");
    }
    {
        let _inflight = first.handle.inflight("requests");
    }
    first.completion.finish_ok();

    tailtriage.begin_request("/invoice").completion.finish_ok();
    tailtriage.begin_request("/invoice").completion.finish_ok();

    for i in 1..=3 {
        tailtriage.record_runtime_snapshot(crate::RuntimeSnapshot {
            at_unix_ms: crate::unix_time_ms(),
            alive_tasks: Some(i),
            global_queue_depth: Some(i),
            local_queue_depth: None,
            blocking_queue_depth: None,
            remote_schedule_count: None,
        });
    }

    let snapshot = tailtriage.snapshot();
    assert!(snapshot.truncation.limits_hit);
    assert_eq!(snapshot.requests.len(), 1);
    assert_eq!(snapshot.stages.len(), 1);
    assert_eq!(snapshot.queues.len(), 1);
    assert_eq!(snapshot.inflight.len(), 1);
    assert_eq!(snapshot.runtime_snapshots.len(), 1);
    assert_eq!(snapshot.truncation.dropped_requests, 2);
    assert_eq!(snapshot.truncation.dropped_stages, 2);
    assert_eq!(snapshot.truncation.dropped_queues, 2);
    assert_eq!(snapshot.truncation.dropped_inflight_snapshots, 5);
    assert_eq!(snapshot.truncation.dropped_runtime_snapshots, 2);
}

#[test]
fn shutdown_artifact_includes_post_saturation_drops() {
    let sink = Arc::new(RecordingSink::default());
    let tailtriage = Tailtriage::builder("payments")
        .capture_limits(CaptureLimits {
            max_requests: 1,
            max_stages: 1,
            max_queues: 1,
            max_inflight_snapshots: 1,
            max_runtime_snapshots: 1,
        })
        .sink(sink.clone())
        .build()
        .expect("build should succeed");

    tailtriage.begin_request("/a").completion.finish_ok();
    tailtriage.begin_request("/b").completion.finish_ok();
    tailtriage.begin_request("/c").completion.finish_ok();

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
    tailtriage.record_runtime_snapshot(crate::RuntimeSnapshot {
        at_unix_ms: crate::unix_time_ms(),
        alive_tasks: Some(3),
        global_queue_depth: Some(3),
        local_queue_depth: None,
        blocking_queue_depth: None,
        remote_schedule_count: None,
    });

    tailtriage.shutdown().expect("shutdown should succeed");
    let artifact = sink
        .run
        .lock()
        .expect("lock should succeed")
        .clone()
        .expect("run should be captured");

    assert!(artifact.truncation.limits_hit);
    assert_eq!(artifact.truncation.dropped_requests, 2);
    assert_eq!(artifact.truncation.dropped_runtime_snapshots, 2);
}

#[test]
fn unsaturated_runs_keep_zero_truncation_counters() {
    let tailtriage = Tailtriage::builder("payments")
        .capture_limits(CaptureLimits {
            max_requests: 10,
            max_stages: 10,
            max_queues: 10,
            max_inflight_snapshots: 10,
            max_runtime_snapshots: 10,
        })
        .build()
        .expect("build should succeed");

    let started = tailtriage.begin_request("/ok");
    futures_executor::block_on(started.handle.stage("db").await_value(ready(())));
    futures_executor::block_on(started.handle.queue("permit").await_on(ready(())));
    {
        let _guard = started.handle.inflight("requests");
    }
    started.completion.finish_ok();
    tailtriage.record_runtime_snapshot(crate::RuntimeSnapshot {
        at_unix_ms: crate::unix_time_ms(),
        alive_tasks: Some(1),
        global_queue_depth: Some(1),
        local_queue_depth: None,
        blocking_queue_depth: None,
        remote_schedule_count: None,
    });

    let snapshot = tailtriage.snapshot();
    assert!(!snapshot.truncation.is_truncated());
    assert!(!snapshot.truncation.limits_hit);
    assert_eq!(snapshot.truncation.dropped_requests, 0);
    assert_eq!(snapshot.truncation.dropped_stages, 0);
    assert_eq!(snapshot.truncation.dropped_queues, 0);
    assert_eq!(snapshot.truncation.dropped_inflight_snapshots, 0);
    assert_eq!(snapshot.truncation.dropped_runtime_snapshots, 0);
}

#[test]
fn mode_does_not_change_event_types_or_lifecycle_shape() {
    let light = Tailtriage::builder("payments")
        .light()
        .build()
        .expect("build should succeed");
    let investigation = Tailtriage::builder("payments")
        .investigation()
        .build()
        .expect("build should succeed");

    let started_light = light.begin_request("/same");
    futures_executor::block_on(started_light.handle.queue("q").await_on(ready(())));
    futures_executor::block_on(started_light.handle.stage("s").await_value(ready(())));
    started_light.completion.finish_ok();

    let started_investigation = investigation.begin_request("/same");
    futures_executor::block_on(started_investigation.handle.queue("q").await_on(ready(())));
    futures_executor::block_on(
        started_investigation
            .handle
            .stage("s")
            .await_value(ready(())),
    );
    started_investigation.completion.finish_ok();

    let light_snapshot = light.snapshot();
    let investigation_snapshot = investigation.snapshot();
    assert_eq!(light_snapshot.requests.len(), 1);
    assert_eq!(light_snapshot.queues.len(), 1);
    assert_eq!(light_snapshot.stages.len(), 1);
    assert_eq!(investigation_snapshot.requests.len(), 1);
    assert_eq!(investigation_snapshot.queues.len(), 1);
    assert_eq!(investigation_snapshot.stages.len(), 1);
}

#[test]
fn legacy_artifact_without_effective_core_config_deserializes_as_unknown() {
    let legacy = serde_json::json!({
        "schema_version": crate::SCHEMA_VERSION,
        "metadata": {
            "run_id": "run-legacy",
            "service_name": "payments",
            "service_version": null,
            "started_at_unix_ms": 1,
            "finished_at_unix_ms": 2,
            "mode": "investigation",
            "host": null,
            "pid": 123,
            "lifecycle_warnings": [],
            "unfinished_requests": {
                "count": 0,
                "sample": []
            }
        },
        "requests": [],
        "stages": [],
        "queues": [],
        "inflight": [],
        "runtime_snapshots": [],
        "truncation": {
            "dropped_requests": 0,
            "dropped_stages": 0,
            "dropped_queues": 0,
            "dropped_inflight_snapshots": 0,
            "dropped_runtime_snapshots": 0
        }
    });

    let parsed: crate::Run = serde_json::from_value(legacy).expect("legacy run should parse");
    assert_eq!(parsed.metadata.mode, CaptureMode::Investigation);
    assert!(parsed.metadata.effective_core_config.is_none());
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

#[test]
fn owned_started_request_maps_result_to_request_outcome() {
    let tailtriage = Arc::new(build_for_test(
        "payments",
        "tailtriage-core-owned-started.json",
    ));

    let ok_value = tailtriage
        .begin_request_owned("/owned-result-ok")
        .completion
        .finish_result(Ok::<u8, &'static str>(9))
        .expect("ok result should remain ok");
    assert_eq!(ok_value, 9);

    let err = tailtriage
        .begin_request_owned("/owned-result-err")
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
