#![cfg(feature = "live")]

mod support;

use std::time::Duration;

use support::equivalence_harness::assert_deterministic_span_import_full_parity;
use tailtriage_analyzer::{analyze_run, AnalyzeOptions};
use tailtriage_core::{RequestOptions, Run, Tailtriage};
use tracing_subscriber::prelude::*;

#[test]
fn deterministic_span_import_matches_native_run_analysis_and_rendering() {
    assert_deterministic_span_import_full_parity();
}

#[test]
fn native_core_and_live_tracing_capture_preserve_authoritative_duration_semantics() {
    let native = native_request_queue_stage_run();
    let live = live_tracing_request_queue_stage_run();

    assert_capture_duration_semantics(&native, "native core capture");
    assert_capture_duration_semantics(&live, "live tracing capture");
}

fn native_request_queue_stage_run() -> Run {
    let tailtriage = Tailtriage::builder("svc")
        .run_id("native-timing-parity")
        .build()
        .expect("native Tailtriage build should succeed");
    let started = tailtriage.begin_request_with(
        "/checkout",
        RequestOptions::new().request_id("req-1").kind("http"),
    );
    let request = started.handle;

    futures_executor::block_on(request.queue("permits").await_on(async {
        std::thread::sleep(Duration::from_millis(1));
    }));
    futures_executor::block_on(request.stage("db").await_value(async {
        std::thread::sleep(Duration::from_millis(1));
    }));
    std::thread::sleep(Duration::from_millis(1));
    started.completion.finish_ok();

    tailtriage.snapshot()
}

fn live_tracing_request_queue_stage_run() -> Run {
    let recorder = tailtriage_tracing::TracingRecorder::builder("svc")
        .run_id("live-timing-parity")
        .build()
        .expect("live tracing recorder build should succeed");
    let subscriber = tracing_subscriber::registry().with(recorder.layer());

    tracing::subscriber::with_default(subscriber, || {
        let request = tracing::info_span!(
            "request",
            tt.kind = "request",
            tt.request_id = "req-1",
            tt.route = "/checkout"
        )
        .entered();

        {
            let queue = tracing::info_span!(
                "queue",
                tt.kind = "queue",
                tt.request_id = "req-1",
                tt.queue = "permits"
            )
            .entered();
            std::thread::sleep(Duration::from_millis(1));
            drop(queue);
        }

        {
            let stage = tracing::info_span!(
                "stage",
                tt.kind = "stage",
                tt.request_id = "req-1",
                tt.stage = "db"
            )
            .entered();
            std::thread::sleep(Duration::from_millis(1));
            drop(stage);
        }

        std::thread::sleep(Duration::from_millis(1));
        drop(request);
    });

    recorder
        .snapshot_run()
        .expect("live tracing recorder snapshot should convert")
        .run()
        .clone()
}

fn assert_capture_duration_semantics(run: &Run, label: &str) {
    assert_eq!(run.requests.len(), 1, "{label} should retain one request");
    assert_eq!(run.stages.len(), 1, "{label} should retain one stage");
    assert_eq!(run.queues.len(), 1, "{label} should retain one queue");

    let request = &run.requests[0];
    let stage = &run.stages[0];
    let queue = &run.queues[0];

    assert!(
        request.latency_us > 0,
        "{label} request latency_us should be positive"
    );
    assert!(
        stage.latency_us > 0,
        "{label} stage latency_us should be positive"
    );
    assert!(
        queue.wait_us <= request.latency_us,
        "{label} queue wait_us should be retained as a duration field"
    );
    assert!(
        request.finished_at_unix_ms >= request.started_at_unix_ms,
        "{label} request finish wall timestamp should not precede start"
    );
    assert!(
        stage.finished_at_unix_ms >= stage.started_at_unix_ms,
        "{label} stage finish wall timestamp should not precede start"
    );
    assert!(
        queue.waited_until_unix_ms >= queue.waited_from_unix_ms,
        "{label} queue finish wall timestamp should not precede start"
    );

    let value = serde_json::to_value(run).expect("run should serialize to JSON");
    assert!(
        value["requests"][0].get("latency_us").is_some(),
        "{label} request latency_us should serialize as an explicit duration field"
    );
    assert!(
        value["stages"][0].get("latency_us").is_some(),
        "{label} stage latency_us should serialize as an explicit duration field"
    );
    assert!(
        value["queues"][0].get("wait_us").is_some(),
        "{label} queue wait_us should serialize as an explicit duration field"
    );

    let mut contradictory = run.clone();
    contradictory.requests[0].started_at_unix_ms = 10_000;
    contradictory.requests[0].finished_at_unix_ms = 10_001;
    let report = analyze_run(&contradictory, AnalyzeOptions::default());
    assert_eq!(
        report.p50_latency_us,
        Some(request.latency_us),
        "{label} analyzer should use explicit latency_us rather than wall timestamp deltas"
    );
}
