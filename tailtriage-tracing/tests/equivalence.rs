use std::collections::{BTreeMap, BTreeSet};
use std::thread;
use std::time::Duration;

use tailtriage_core::{Outcome, RequestEvent, Run, Tailtriage};
use tailtriage_tracing::TracingRecorder;
use tracing_subscriber::prelude::*;

#[derive(Debug, Clone, PartialEq, Eq)]
struct ArtifactEquivalenceReport {
    native_request_count: usize,
    tracing_request_count: usize,
    matched_routes: BTreeSet<String>,
    matched_stage_names: BTreeSet<String>,
    matched_queue_names: BTreeSet<String>,
    missing_fields: Vec<String>,
    latency_order_preserved: bool,
    is_equivalent: bool,
}

impl ArtifactEquivalenceReport {
    fn failure_message(&self) -> String {
        format!(
            "equivalence failed: native_requests={}, tracing_requests={}, matched_routes={:?}, matched_stages={:?}, matched_queues={:?}, missing_fields={:?}, latency_order_preserved={}",
            self.native_request_count,
            self.tracing_request_count,
            self.matched_routes,
            self.matched_stage_names,
            self.matched_queue_names,
            self.missing_fields,
            self.latency_order_preserved
        )
    }
}

fn canonical_requests() -> Vec<(&'static str, u64, u64, u64)> {
    vec![("req-1", 1, 2, 2), ("req-2", 2, 3, 3), ("req-3", 4, 20, 12)]
}

fn sleep_ms(ms: u64) {
    thread::sleep(Duration::from_millis(ms));
}

fn build_native_run() -> Run {
    let tailtriage = Tailtriage::builder("equivalence-native")
        .build()
        .expect("native builder must succeed");

    for (request_id, queue_ms, stage_a_ms, stage_b_ms) in canonical_requests() {
        let started = tailtriage.begin_request_with(
            "/equivalence",
            tailtriage_core::RequestOptions::new().request_id(request_id),
        );

        futures_executor::block_on(
            started
                .handle
                .queue("db-pool")
                .await_on(async { sleep_ms(queue_ms) }),
        );

        futures_executor::block_on(
            started
                .handle
                .stage("decode")
                .await_value(async { sleep_ms(stage_a_ms) }),
        );

        futures_executor::block_on(
            started
                .handle
                .stage("db.query")
                .await_value(async { sleep_ms(stage_b_ms) }),
        );

        started.completion.finish(Outcome::Ok);
    }

    tailtriage.snapshot()
}

fn build_tracing_run() -> Run {
    let recorder = TracingRecorder::builder("equivalence-tracing")
        .run_id("equivalence-run")
        .build();
    let subscriber = tracing_subscriber::registry().with(recorder.layer());

    tracing::subscriber::with_default(subscriber, || {
        for (request_id, queue_ms, stage_a_ms, stage_b_ms) in canonical_requests() {
            let request_span = tracing::info_span!(
                "request",
                tt.kind = "request",
                tt.request_id = request_id,
                tt.route = "/equivalence"
            );
            let _request_guard = request_span.enter();

            let queue_span = tracing::info_span!(
                "queue",
                tt.kind = "queue",
                tt.request_id = request_id,
                tt.queue = "db-pool"
            );
            {
                let _queue_guard = queue_span.enter();
                sleep_ms(queue_ms);
            }
            drop(queue_span);

            let decode_stage_span = tracing::info_span!(
                "stage_a",
                tt.kind = "stage",
                tt.request_id = request_id,
                tt.stage = "decode",
                tt.success = true
            );
            {
                let _stage_guard = decode_stage_span.enter();
                sleep_ms(stage_a_ms);
            }
            drop(decode_stage_span);

            let query_stage_span = tracing::info_span!(
                "stage_b",
                tt.kind = "stage",
                tt.request_id = request_id,
                tt.stage = "db.query",
                tt.success = true
            );
            {
                let _stage_guard = query_stage_span.enter();
                sleep_ms(stage_b_ms);
            }
            drop(query_stage_span);
        }
    });

    recorder
        .snapshot_run()
        .expect("tracing conversion should succeed")
        .run()
        .clone()
}

fn normalize_run(run: &Run) -> Run {
    let mut normalized = run.clone();
    normalized.metadata.run_id = "normalized-run-id".to_string();
    normalized.metadata.started_at_unix_ms = 0;
    normalized.metadata.finished_at_unix_ms = 0;
    normalized.metadata.finalized_at_unix_ms = None;
    normalized.runtime_snapshots.clear();

    for req in &mut normalized.requests {
        req.started_at_unix_ms = 0;
        req.finished_at_unix_ms = 0;
    }
    for stage in &mut normalized.stages {
        stage.started_at_unix_ms = 0;
        stage.finished_at_unix_ms = 0;
    }
    for queue in &mut normalized.queues {
        queue.waited_from_unix_ms = 0;
        queue.waited_until_unix_ms = 0;
    }

    normalized
        .requests
        .sort_by_key(|req| req.request_id.clone());
    normalized
        .stages
        .sort_by_key(|stage| (stage.request_id.clone(), stage.stage.clone()));
    normalized
        .queues
        .sort_by_key(|queue| (queue.request_id.clone(), queue.queue.clone()));
    normalized
}

fn request_latency_rank(run: &Run) -> Vec<String> {
    let mut ordered: Vec<&RequestEvent> = run.requests.iter().collect();
    ordered.sort_by_key(|req| req.latency_us);
    ordered
        .into_iter()
        .map(|req| req.request_id.clone())
        .collect::<Vec<_>>()
}

fn compare_runs(native: &Run, tracing: &Run) -> ArtifactEquivalenceReport {
    let native_norm = normalize_run(native);
    let tracing_norm = normalize_run(tracing);

    let native_routes: BTreeSet<String> = native_norm
        .requests
        .iter()
        .map(|r| r.route.clone())
        .collect();
    let tracing_routes: BTreeSet<String> = tracing_norm
        .requests
        .iter()
        .map(|r| r.route.clone())
        .collect();

    let native_stages: BTreeSet<String> =
        native_norm.stages.iter().map(|s| s.stage.clone()).collect();
    let tracing_stages: BTreeSet<String> = tracing_norm
        .stages
        .iter()
        .map(|s| s.stage.clone())
        .collect();

    let native_queues: BTreeSet<String> =
        native_norm.queues.iter().map(|q| q.queue.clone()).collect();
    let tracing_queues: BTreeSet<String> = tracing_norm
        .queues
        .iter()
        .map(|q| q.queue.clone())
        .collect();

    let matched_routes = native_routes
        .intersection(&tracing_routes)
        .cloned()
        .collect();
    let matched_stage_names = native_stages
        .intersection(&tracing_stages)
        .cloned()
        .collect();
    let matched_queue_names = native_queues
        .intersection(&tracing_queues)
        .cloned()
        .collect();

    let mut missing_fields = Vec::new();
    if native_norm.requests.iter().any(|r| r.latency_us == 0)
        || tracing_norm.requests.iter().any(|r| r.latency_us == 0)
    {
        missing_fields.push("request latency must be positive".to_string());
    }
    if native_norm.stages.iter().any(|s| s.latency_us == 0)
        || tracing_norm.stages.iter().any(|s| s.latency_us == 0)
    {
        missing_fields.push("stage latency must be positive".to_string());
    }
    if native_norm.queues.iter().any(|q| q.wait_us == 0)
        || tracing_norm.queues.iter().any(|q| q.wait_us == 0)
    {
        missing_fields.push("queue latency must be positive".to_string());
    }

    let native_shape: BTreeMap<String, (BTreeSet<String>, BTreeSet<String>)> =
        request_shape(&native_norm);
    let tracing_shape: BTreeMap<String, (BTreeSet<String>, BTreeSet<String>)> =
        request_shape(&tracing_norm);

    if native_shape != tracing_shape {
        missing_fields.push("request/stage/queue correlation shape differs".to_string());
    }

    let latency_order_preserved =
        request_latency_rank(&native_norm) == request_latency_rank(&tracing_norm);

    let is_equivalent = native_norm.requests.len() == tracing_norm.requests.len()
        && native_routes == tracing_routes
        && native_stages == tracing_stages
        && native_queues == tracing_queues
        && native_shape == tracing_shape
        && missing_fields.is_empty()
        && latency_order_preserved;

    ArtifactEquivalenceReport {
        native_request_count: native_norm.requests.len(),
        tracing_request_count: tracing_norm.requests.len(),
        matched_routes,
        matched_stage_names,
        matched_queue_names,
        missing_fields,
        latency_order_preserved,
        is_equivalent,
    }
}

fn request_shape(run: &Run) -> BTreeMap<String, (BTreeSet<String>, BTreeSet<String>)> {
    let mut shape: BTreeMap<String, (BTreeSet<String>, BTreeSet<String>)> = BTreeMap::new();
    for req in &run.requests {
        shape.entry(req.request_id.clone()).or_default();
    }
    for stage in &run.stages {
        shape
            .entry(stage.request_id.clone())
            .or_default()
            .0
            .insert(stage.stage.clone());
    }
    for queue in &run.queues {
        shape
            .entry(queue.request_id.clone())
            .or_default()
            .1
            .insert(queue.queue.clone());
    }
    shape
}

#[test]
fn native_and_tracing_artifacts_are_equivalent() {
    let native = build_native_run();
    let tracing = build_tracing_run();

    let report = compare_runs(&native, &tracing);
    assert!(report.is_equivalent, "{}", report.failure_message());
}

#[test]
fn equivalence_report_failure_message_is_actionable() {
    let native = build_native_run();
    let mut tracing = build_tracing_run();
    tracing.queues.clear();

    let report = compare_runs(&native, &tracing);
    assert!(!report.is_equivalent);
    let message = report.failure_message();
    assert!(message.contains("missing_fields"));
    assert!(message.contains("matched_queues"));
    assert!(message.contains("latency_order_preserved"));
}

#[test]
fn normalization_does_not_mutate_original_runs() {
    let native = build_native_run();
    let original = native.clone();

    let normalized = normalize_run(&native);

    assert_eq!(native, original);
    assert_ne!(normalized.metadata.run_id, native.metadata.run_id);
}
