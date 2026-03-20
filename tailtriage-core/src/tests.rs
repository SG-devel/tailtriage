use std::future::ready;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{
    CaptureMode, Config, InFlightSnapshot, InitError, LocalJsonSink, QueueEvent, RequestEvent,
    RequestMeta, Run, RunMetadata, RunSink, RuntimeSnapshot, StageEvent, Tailtriage,
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
    assert!(
        written.contains("\n  \"metadata\": {\n"),
        "expected pretty JSON formatting"
    );

    let decoded: Run = serde_json::from_str(&written).expect("written JSON should parse");
    assert_eq!(decoded, run);

    std::fs::remove_file(path).expect("temp run file should be removable");
}

#[test]
fn init_rejects_blank_service_name() {
    let mut config = Config::new("payments");
    config.service_name = "   ".to_owned();

    let err = Tailtriage::init(config).expect_err("blank service_name should fail");
    assert_eq!(err, InitError::EmptyServiceName);
}

#[test]
fn request_records_timing_and_outcome() {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before epoch")
        .as_nanos();

    let mut config = Config::new("payments");
    config.output_path = std::env::temp_dir().join(format!("tailtriage_core_scope_{nanos}.json"));

    let tailtriage = Tailtriage::init(config).expect("init should succeed");
    let mut request = RequestMeta::new("req-42", "/invoice");
    request.kind = Some("create_invoice".to_owned());

    let result = futures_executor::block_on(tailtriage.request(request, "ok", ready(7_u32)));
    assert_eq!(result, 7);

    let snapshot = tailtriage.snapshot();
    assert_eq!(snapshot.requests.len(), 1);

    let event = &snapshot.requests[0];
    assert_eq!(event.request_id, "req-42");
    assert_eq!(event.route, "/invoice");
    assert_eq!(event.kind.as_deref(), Some("create_invoice"));
    assert_eq!(event.outcome, "ok");
    assert!(event.finished_at_unix_ms >= event.started_at_unix_ms);
}

#[test]
fn request_meta_for_route_generates_traceable_unique_ids() {
    let first = RequestMeta::for_route("/invoice");
    let second = RequestMeta::for_route("/invoice");

    assert_eq!(first.route, "/invoice");
    assert_eq!(second.route, "/invoice");
    assert_ne!(first.request_id, second.request_id);
    assert!(first.request_id.starts_with("_invoice-"));
    assert!(second.request_id.starts_with("_invoice-"));
}

#[test]
fn request_with_for_route_and_kind_records_expected_fields() {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before epoch")
        .as_nanos();

    let mut config = Config::new("payments");
    config.output_path = std::env::temp_dir().join(format!("tailtriage_core_helper_{nanos}.json"));

    let tailtriage = Tailtriage::init(config).expect("init should succeed");
    let meta = RequestMeta::for_route("/invoice").with_kind("create_invoice");
    let result = futures_executor::block_on(tailtriage.request(meta, "ok", ready(9_u32)));
    assert_eq!(result, 9);

    let snapshot = tailtriage.snapshot();
    assert_eq!(snapshot.requests.len(), 1);
    let event = &snapshot.requests[0];
    assert_eq!(event.route, "/invoice");
    assert_eq!(event.kind.as_deref(), Some("create_invoice"));
    assert_eq!(event.outcome, "ok");
    assert!(event.request_id.starts_with("_invoice-"));
}

#[test]
fn request_with_for_route_records_route_and_outcome_without_kind() {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before epoch")
        .as_nanos();

    let mut config = Config::new("payments");
    config.output_path =
        std::env::temp_dir().join(format!("tailtriage_core_route_only_{nanos}.json"));

    let tailtriage = Tailtriage::init(config).expect("init should succeed");
    let result = futures_executor::block_on(tailtriage.request(
        RequestMeta::for_route("/invoice"),
        "error",
        ready(13_u32),
    ));
    assert_eq!(result, 13);

    let snapshot = tailtriage.snapshot();
    assert_eq!(snapshot.requests.len(), 1);
    let event = &snapshot.requests[0];
    assert_eq!(event.route, "/invoice");
    assert_eq!(event.kind, None);
    assert_eq!(event.outcome, "error");
    assert!(event.request_id.starts_with("_invoice-"));
}

#[test]
fn flush_writes_current_snapshot() {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before epoch")
        .as_nanos();

    let output_path = std::env::temp_dir().join(format!("tailtriage_core_flush_{nanos}.json"));
    let mut config = Config::new("payments");
    config.output_path = output_path.clone();

    let tailtriage = Tailtriage::init(config).expect("init should succeed");
    tailtriage.flush().expect("flush should write run file");

    let bytes = std::fs::metadata(&output_path)
        .expect("flush output should exist")
        .len();
    assert!(bytes > 0);

    std::fs::remove_file(output_path).expect("temp run file should be removable");
}

#[test]
fn inflight_guard_records_increment_and_decrement() {
    let mut config = Config::new("payments");
    config.output_path = std::env::temp_dir().join("tailtriage_core_inflight_test.json");

    let tailtriage = Tailtriage::init(config).expect("init should succeed");

    {
        let _guard = tailtriage.inflight("invoice_requests");
        let snapshot = tailtriage.snapshot();
        assert_eq!(snapshot.inflight.len(), 1);
        assert_eq!(snapshot.inflight[0].gauge, "invoice_requests");
        assert_eq!(snapshot.inflight[0].count, 1);
    }

    let snapshot = tailtriage.snapshot();
    assert_eq!(snapshot.inflight.len(), 2);
    assert_eq!(snapshot.inflight[1].gauge, "invoice_requests");
    assert_eq!(snapshot.inflight[1].count, 0);
}

#[test]
fn stage_wrapper_records_stage_event() {
    let mut config = Config::new("payments");
    config.output_path = std::env::temp_dir().join("tailtriage_core_stage_test.json");

    let tailtriage = Tailtriage::init(config).expect("init should succeed");

    let result = futures_executor::block_on(
        tailtriage
            .stage("req-22", "fetch_customer")
            .await_value(ready(11_u32)),
    );
    assert_eq!(result, 11);

    let snapshot = tailtriage.snapshot();
    assert_eq!(snapshot.stages.len(), 1);
    let event = &snapshot.stages[0];
    assert_eq!(event.request_id, "req-22");
    assert_eq!(event.stage, "fetch_customer");
    assert!(event.success);
    assert!(event.finished_at_unix_ms >= event.started_at_unix_ms);
}

#[test]
fn stage_wrapper_records_success_for_ok_result() {
    let mut config = Config::new("payments");
    config.output_path = std::env::temp_dir().join("tailtriage_core_stage_ok_test.json");

    let tailtriage = Tailtriage::init(config).expect("init should succeed");

    let result = futures_executor::block_on(
        tailtriage
            .stage("req-33", "persist_invoice")
            .await_on(ready::<Result<u32, &'static str>>(Ok(17_u32))),
    );
    assert_eq!(result, Ok(17));

    let snapshot = tailtriage.snapshot();
    assert_eq!(snapshot.stages.len(), 1);
    let event = &snapshot.stages[0];
    assert_eq!(event.request_id, "req-33");
    assert_eq!(event.stage, "persist_invoice");
    assert!(event.success);
}

#[test]
fn stage_wrapper_records_failure_for_err_result() {
    let mut config = Config::new("payments");
    config.output_path = std::env::temp_dir().join("tailtriage_core_stage_err_test.json");

    let tailtriage = Tailtriage::init(config).expect("init should succeed");

    let result = futures_executor::block_on(
        tailtriage
            .stage("req-34", "persist_invoice")
            .await_on(ready::<Result<u32, &'static str>>(Err("boom"))),
    );
    assert_eq!(result, Err("boom"));

    let snapshot = tailtriage.snapshot();
    assert_eq!(snapshot.stages.len(), 1);
    let event = &snapshot.stages[0];
    assert_eq!(event.request_id, "req-34");
    assert_eq!(event.stage, "persist_invoice");
    assert!(!event.success);
}

#[test]
fn queue_wrapper_records_wait_event() {
    let mut config = Config::new("payments");
    config.output_path = std::env::temp_dir().join("tailtriage_core_queue_test.json");

    let tailtriage = Tailtriage::init(config).expect("init should succeed");

    let result = futures_executor::block_on(
        tailtriage
            .queue("req-22", "invoice_worker")
            .with_depth_at_start(3)
            .await_on(ready(11_u32)),
    );
    assert_eq!(result, 11);

    let snapshot = tailtriage.snapshot();
    assert_eq!(snapshot.queues.len(), 1);
    let event = &snapshot.queues[0];
    assert_eq!(event.request_id, "req-22");
    assert_eq!(event.queue, "invoice_worker");
    assert_eq!(event.depth_at_start, Some(3));
    assert!(event.waited_until_unix_ms >= event.waited_from_unix_ms);
}
