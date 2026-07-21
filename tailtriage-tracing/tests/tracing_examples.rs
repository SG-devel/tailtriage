use std::{fs::File, io::Cursor, path::PathBuf};

use tailtriage_analyzer::{analyze_run, AnalyzeOptions};
#[cfg(feature = "live")]
use tailtriage_core::{RequestOptions, Run, Tailtriage};
#[cfg(feature = "live")]
use tailtriage_tracing::TracingSession;
use tailtriage_tracing::{
    import_jsonl_path, import_jsonl_reader, import_jsonl_reader_with_mode, ImportOptions,
    JsonlParseMode,
};
#[cfg(feature = "live")]
use tracing_subscriber::prelude::*;

#[cfg(feature = "live")]
fn native_single_request_run() -> Run {
    let tailtriage = Tailtriage::builder("svc")
        .build()
        .expect("native session should build");
    let started = tailtriage.begin_request_with(
        "/checkout",
        RequestOptions::new().request_id("req-1").kind("http"),
    );
    let request = started.handle;

    futures_executor::block_on(request.queue("admission").await_on(async {
        std::thread::sleep(std::time::Duration::from_millis(1));
    }));
    futures_executor::block_on(request.stage("db").await_value(async {
        std::thread::sleep(std::time::Duration::from_millis(1));
    }));
    std::thread::sleep(std::time::Duration::from_millis(1));
    started.completion.finish_ok();

    tailtriage.snapshot()
}

#[cfg(feature = "live")]
fn live_tracing_single_request_run() -> Run {
    let session = TracingSession::builder("svc")
        .build()
        .expect("live session should build");
    let subscriber = tracing_subscriber::registry().with(session.layer());
    tracing::subscriber::with_default(subscriber, || {
        let request = tracing::info_span!(
            "request",
            tt.kind = "request",
            tt.request_id = "req-1",
            tt.route = "/checkout",
            tt.outcome = "ok"
        );
        let request_guard = request.enter();

        let queue = tracing::info_span!(
            "queue",
            tt.kind = "queue",
            tt.request_id = "req-1",
            tt.queue = "admission",
            tt.depth_at_start = 1_u64
        );
        {
            let _queue_guard = queue.enter();
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        drop(queue);

        let stage = tracing::info_span!(
            "stage",
            tt.kind = "stage",
            tt.request_id = "req-1",
            tt.stage = "db",
            tt.success = true
        );
        {
            let _stage_guard = stage.enter();
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        drop(stage);
        std::thread::sleep(std::time::Duration::from_millis(1));

        drop(request_guard);
        drop(request);
    });

    session
        .snapshot_run()
        .expect("live session snapshot should convert")
        .run()
        .clone()
}

#[cfg(feature = "live")]
fn assert_single_request_timing_semantics(run: &Run) {
    assert_eq!(run.requests.len(), 1);
    assert_eq!(run.queues.len(), 1);
    assert_eq!(run.stages.len(), 1);

    let request = &run.requests[0];
    assert!(request.latency_us > 0);
    assert!(request.finished_at_unix_ms >= request.started_at_unix_ms);

    let stage = &run.stages[0];
    assert!(stage.latency_us > 0);
    assert!(stage.finished_at_unix_ms >= stage.started_at_unix_ms);

    let queue = &run.queues[0];
    assert!(queue.waited_until_unix_ms >= queue.waited_from_unix_ms);

    let json = serde_json::to_value(run).expect("run should serialize");
    assert!(json["requests"][0].get("latency_us").is_some());
    assert!(json["stages"][0].get("latency_us").is_some());
    assert!(json["queues"][0].get("wait_us").is_some());

    let mut duration_authority = run.clone();
    duration_authority.requests[0].started_at_unix_ms = 10;
    duration_authority.requests[0].finished_at_unix_ms = 11;
    duration_authority.requests[0].latency_us = 50_000;
    duration_authority.stages[0].started_at_unix_ms = 10;
    duration_authority.stages[0].finished_at_unix_ms = 11;
    duration_authority.stages[0].latency_us = 40_000;
    duration_authority.queues[0].waited_from_unix_ms = 10;
    duration_authority.queues[0].waited_until_unix_ms = 11;
    duration_authority.queues[0].wait_us = 30_000;

    let report = analyze_run(&duration_authority, AnalyzeOptions::default());
    assert_eq!(report.p50_latency_us, Some(50_000));
    assert_eq!(report.p95_latency_us, Some(50_000));
    assert_eq!(report.p99_latency_us, Some(50_000));
    assert_eq!(report.p95_queue_share_permille, Some(600));
}

#[cfg(feature = "live")]
#[test]
fn native_core_and_live_tracing_capture_preserve_timing_semantics() {
    let native = native_single_request_run();
    let tracing = live_tracing_single_request_run();

    assert_single_request_timing_semantics(&native);
    assert_single_request_timing_semantics(&tracing);
}

#[test]
fn jsonl_fixture_imports_completed_span_shape() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("tracing_spans.jsonl");

    let imported = import_jsonl_path(
        &fixture,
        ImportOptions::new("checkout-service")
            .service_version("example")
            .run_id("fixture-example")
            .strict(true),
    )
    .expect("fixture should import");

    let run = imported.run();
    assert_eq!(run.requests.len(), 1);
    assert_eq!(run.queues.len(), 1);
    assert_eq!(run.stages.len(), 1);
    assert_eq!(run.requests[0].request_id, "req-42");
    assert_eq!(run.requests[0].route, "/checkout");
    assert_eq!(run.requests[0].outcome, "ok");
    assert_eq!(run.queues[0].queue, "db-pool");
    assert_eq!(run.queues[0].depth_at_start, Some(7));
    assert_eq!(run.stages[0].stage, "db.query");
    assert!(run.stages[0].success);
    assert!(imported.warnings().iter().any(|warning| warning
        .message()
        .contains("precise_interval_validation_unavailable")));
}

#[test]
fn jsonl_fixture_reader_and_path_import_parity_on_counts() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("tracing_spans.jsonl");

    let options = ImportOptions::new("checkout-service")
        .service_version("example")
        .run_id("fixture-example")
        .strict(true);

    let from_path =
        import_jsonl_path(&fixture, options.clone()).expect("path import should succeed");

    let file = File::open(&fixture).expect("fixture should open");
    let from_reader = import_jsonl_reader(file, options).expect("reader import should succeed");

    let run_path = from_path.run();
    let run_reader = from_reader.run();
    assert_eq!(run_path.requests.len(), run_reader.requests.len());
    assert_eq!(run_path.queues.len(), run_reader.queues.len());
    assert_eq!(run_path.stages.len(), run_reader.stages.len());
}

#[test]
fn imported_fixture_run_is_analyzable_and_has_no_runtime_snapshots() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("tracing_spans.jsonl");
    let imported = import_jsonl_path(&fixture, ImportOptions::new("checkout-service"))
        .expect("fixture import should succeed");

    let run = imported.run();
    assert_eq!(run.requests.len(), 1);
    assert_eq!(run.queues.len(), 1);
    assert_eq!(run.stages.len(), 1);
    assert!(
        run.runtime_snapshots.is_empty(),
        "tracing-only import must not fabricate runtime snapshots"
    );
    let report = analyze_run(run, AnalyzeOptions::default());
    assert_eq!(report.request_count, 1);
}

#[test]
fn stable_wrapper_duration_us_is_authoritative_when_wall_timestamps_disagree() {
    let input = r#"{"format":"tailtriage.tracing-span.v1","span":{"name":"request","started_at_unix_ms":1700000000000,"started_at_run_us":0,"finished_at_unix_ms":1700000000001,"finished_at_run_us":1000,"duration_us":50000,"fields":{"tt.kind":"request","tt.request_id":"req-1","tt.route":"/checkout","tt.outcome":"ok"}}}"#;

    let imported = import_jsonl_reader_with_mode(
        Cursor::new(input),
        ImportOptions::new("svc"),
        JsonlParseMode::TailtriageWrapperOnly,
    )
    .expect("non-strict import should retain mismatched duration");

    assert_eq!(imported.run().requests.len(), 1);
    assert_eq!(imported.run().requests[0].latency_us, 50_000);
    assert!(imported
        .warnings()
        .iter()
        .any(|warning| warning.message().contains("duration_mismatch")));

    let err = import_jsonl_reader_with_mode(
        Cursor::new(input),
        ImportOptions::new("svc").strict(true),
        JsonlParseMode::TailtriageWrapperOnly,
    )
    .expect_err("strict import should reject mismatched duration");
    assert!(err.to_string().contains("duration_mismatch"));
}

#[test]
fn compatible_reader_api_is_reachable_from_crate_root() {
    let input = r#"{"span":{"name":"req","started_at_unix_ms":1,"finished_at_unix_ms":2,"fields":{"tt.kind":"request","tt.request_id":"r1","tt.route":"/a"}}}"#;

    let imported = import_jsonl_reader_with_mode(
        std::io::Cursor::new(input),
        ImportOptions::new("svc"),
        JsonlParseMode::Compatible,
    )
    .expect("compatible import should work");

    assert_eq!(imported.run().requests.len(), 1);
}
