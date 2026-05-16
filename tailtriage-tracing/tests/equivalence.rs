use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::time::Duration;

use tailtriage_core::{MemorySink, RequestOptions, Run, Tailtriage};
use tailtriage_tracing::TracingRecorder;
use tracing_subscriber::prelude::*;

#[derive(Debug, Clone, PartialEq, Eq)]
struct NormalizedRequest {
    request_id: String,
    route: String,
    outcome: String,
    positive_latency: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NormalizedStage {
    request_id: String,
    stage: String,
    success: bool,
    positive_latency: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NormalizedQueue {
    request_id: String,
    queue: String,
    positive_latency: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NormalizedRun {
    requests: Vec<NormalizedRequest>,
    stages: Vec<NormalizedStage>,
    queues: Vec<NormalizedQueue>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ArtifactEquivalenceReport {
    native_request_count: usize,
    tracing_request_count: usize,
    matched_routes: BTreeSet<String>,
    matched_stage_names: BTreeSet<String>,
    matched_queue_names: BTreeSet<String>,
    missing_fields: Vec<String>,
    slow_request_ordering_preserved: bool,
    is_equivalent: bool,
}

impl ArtifactEquivalenceReport {
    fn failure_message(&self) -> String {
        let mut message = String::new();
        let _ = write!(
            &mut message,
            "artifact equivalence failed: native_requests={}, tracing_requests={}, matched_routes={:?}, matched_stages={:?}, matched_queues={:?}, slow_ordering_preserved={}, missing_fields={:?}",
            self.native_request_count,
            self.tracing_request_count,
            self.matched_routes,
            self.matched_stage_names,
            self.matched_queue_names,
            self.slow_request_ordering_preserved,
            self.missing_fields
        );
        message
    }
}

fn normalize_run(run: &Run) -> NormalizedRun {
    let mut requests = run
        .requests
        .iter()
        .map(|request| NormalizedRequest {
            request_id: request.request_id.clone(),
            route: request.route.clone(),
            outcome: request.outcome.clone(),
            positive_latency: request.latency_us > 0,
        })
        .collect::<Vec<_>>();
    requests.sort_by_key(|request| request.request_id.clone());

    let mut stages = run
        .stages
        .iter()
        .map(|stage| NormalizedStage {
            request_id: stage.request_id.clone(),
            stage: stage.stage.clone(),
            success: stage.success,
            positive_latency: stage.latency_us > 0,
        })
        .collect::<Vec<_>>();
    stages.sort_by_key(|stage| (stage.request_id.clone(), stage.stage.clone()));

    let mut queues = run
        .queues
        .iter()
        .map(|queue| NormalizedQueue {
            request_id: queue.request_id.clone(),
            queue: queue.queue.clone(),
            positive_latency: queue.wait_us > 0,
        })
        .collect::<Vec<_>>();
    queues.sort_by_key(|queue| (queue.request_id.clone(), queue.queue.clone()));

    NormalizedRun {
        requests,
        stages,
        queues,
    }
}

fn slowest_request_by_latency(run: &Run) -> Option<&str> {
    run.requests
        .iter()
        .max_by_key(|request| request.latency_us)
        .map(|request| request.request_id.as_str())
}

fn build_equivalence_report(native: &Run, tracing: &Run) -> ArtifactEquivalenceReport {
    let native_normalized = normalize_run(native);
    let tracing_normalized = normalize_run(tracing);

    let native_routes = native_normalized
        .requests
        .iter()
        .map(|request| request.route.clone())
        .collect::<BTreeSet<_>>();
    let tracing_routes = tracing_normalized
        .requests
        .iter()
        .map(|request| request.route.clone())
        .collect::<BTreeSet<_>>();

    let native_stages = native_normalized
        .stages
        .iter()
        .map(|stage| stage.stage.clone())
        .collect::<BTreeSet<_>>();
    let tracing_stages = tracing_normalized
        .stages
        .iter()
        .map(|stage| stage.stage.clone())
        .collect::<BTreeSet<_>>();

    let native_queues = native_normalized
        .queues
        .iter()
        .map(|queue| queue.queue.clone())
        .collect::<BTreeSet<_>>();
    let tracing_queues = tracing_normalized
        .queues
        .iter()
        .map(|queue| queue.queue.clone())
        .collect::<BTreeSet<_>>();

    let native_shape = shape_map(&native_normalized);
    let tracing_shape = shape_map(&tracing_normalized);

    let all_positive = native_normalized
        .requests
        .iter()
        .all(|event| event.positive_latency)
        && native_normalized
            .stages
            .iter()
            .all(|event| event.positive_latency)
        && native_normalized
            .queues
            .iter()
            .all(|event| event.positive_latency)
        && tracing_normalized
            .requests
            .iter()
            .all(|event| event.positive_latency)
        && tracing_normalized
            .stages
            .iter()
            .all(|event| event.positive_latency)
        && tracing_normalized
            .queues
            .iter()
            .all(|event| event.positive_latency);

    let slow_ordering_preserved =
        slowest_request_by_latency(native) == slowest_request_by_latency(tracing);

    let mut missing_fields = Vec::new();
    if native_shape != tracing_shape {
        missing_fields.push("request/stage/queue shape mismatch".to_string());
    }
    if !all_positive {
        missing_fields.push("non-positive latency observed".to_string());
    }
    if !slow_ordering_preserved {
        missing_fields.push("slow request ordering mismatch".to_string());
    }

    let is_equivalent = native.requests.len() == tracing.requests.len()
        && native.runtime_snapshots.is_empty()
        && tracing.runtime_snapshots.is_empty()
        && native_routes == tracing_routes
        && native_stages == tracing_stages
        && native_queues == tracing_queues
        && native_shape == tracing_shape
        && all_positive
        && slow_ordering_preserved;

    ArtifactEquivalenceReport {
        native_request_count: native.requests.len(),
        tracing_request_count: tracing.requests.len(),
        matched_routes: native_routes
            .intersection(&tracing_routes)
            .cloned()
            .collect(),
        matched_stage_names: native_stages
            .intersection(&tracing_stages)
            .cloned()
            .collect(),
        matched_queue_names: native_queues
            .intersection(&tracing_queues)
            .cloned()
            .collect(),
        missing_fields,
        slow_request_ordering_preserved: slow_ordering_preserved,
        is_equivalent,
    }
}

fn shape_map(run: &NormalizedRun) -> BTreeMap<String, (BTreeSet<String>, BTreeSet<String>)> {
    let mut by_request = BTreeMap::<String, (BTreeSet<String>, BTreeSet<String>)>::new();
    for stage in &run.stages {
        by_request
            .entry(stage.request_id.clone())
            .or_default()
            .0
            .insert(stage.stage.clone());
    }
    for queue in &run.queues {
        by_request
            .entry(queue.request_id.clone())
            .or_default()
            .1
            .insert(queue.queue.clone());
    }
    by_request
}

async fn run_native_scenario() -> Run {
    let sink = MemorySink::new();
    let tailtriage = Tailtriage::builder("equivalence-svc")
        .sink(sink.clone())
        .build()
        .expect("native capture should build");

    for (request_id, queue_delay_ms, stage2_delay_ms) in [
        ("req-fast-1", 2_u64, 3_u64),
        ("req-slow", 6_u64, 15_u64),
        ("req-fast-2", 2_u64, 4_u64),
    ] {
        let started = tailtriage
            .begin_request_with("/canonical", RequestOptions::new().request_id(request_id));
        started
            .handle
            .queue("ingress_queue")
            .await_on(async move {
                tokio::time::sleep(Duration::from_millis(queue_delay_ms)).await;
            })
            .await;
        started
            .handle
            .stage("stage_parse")
            .await_value(async {
                tokio::time::sleep(Duration::from_millis(3)).await;
            })
            .await;
        started
            .handle
            .stage("stage_downstream")
            .await_value(async move {
                tokio::time::sleep(Duration::from_millis(stage2_delay_ms)).await;
            })
            .await;
        started.completion.finish_ok();
    }

    tailtriage
        .shutdown()
        .expect("native shutdown should succeed");
    sink.last_run().expect("native run should be available")
}

async fn run_tracing_scenario() -> Run {
    let recorder = TracingRecorder::builder("equivalence-svc")
        .run_id("trace-run")
        .strict(true)
        .build();
    let subscriber = tracing_subscriber::registry().with(recorder.layer());
    let _subscriber_guard = tracing::subscriber::set_default(subscriber);

    for (request_id, queue_delay_ms, stage2_delay_ms) in [
        ("req-fast-1", 2_u64, 3_u64),
        ("req-slow", 6_u64, 15_u64),
        ("req-fast-2", 2_u64, 4_u64),
    ] {
        let request_span = tracing::info_span!(
            "request",
            tt.kind = "request",
            tt.request_id = request_id,
            tt.route = "/canonical",
            tt.outcome = "ok"
        );
        let _request_guard = request_span.enter();

        {
            let queue_span = tracing::info_span!(
                "queue",
                tt.kind = "queue",
                tt.request_id = request_id,
                tt.queue = "ingress_queue"
            );
            let _queue_guard = queue_span.enter();
            tokio::time::sleep(Duration::from_millis(queue_delay_ms)).await;
        }
        {
            let stage_one = tracing::info_span!(
                "stage",
                tt.kind = "stage",
                tt.request_id = request_id,
                tt.stage = "stage_parse",
                tt.success = true
            );
            let _stage_guard = stage_one.enter();
            tokio::time::sleep(Duration::from_millis(3)).await;
        }
        {
            let stage_two = tracing::info_span!(
                "stage",
                tt.kind = "stage",
                tt.request_id = request_id,
                tt.stage = "stage_downstream",
                tt.success = true
            );
            let _stage_guard = stage_two.enter();
            tokio::time::sleep(Duration::from_millis(stage2_delay_ms)).await;
        }
    }

    recorder
        .shutdown()
        .expect("tracing conversion should succeed")
        .run()
        .clone()
}

#[tokio::test(flavor = "current_thread")]
async fn native_and_tracing_runs_are_semantically_equivalent() {
    let native = run_native_scenario().await;
    let tracing = run_tracing_scenario().await;

    let report = build_equivalence_report(&native, &tracing);
    assert!(report.is_equivalent, "{}", report.failure_message());
}

#[test]
fn report_failure_message_is_actionable() {
    let report = ArtifactEquivalenceReport {
        native_request_count: 3,
        tracing_request_count: 2,
        matched_routes: BTreeSet::new(),
        matched_stage_names: BTreeSet::new(),
        matched_queue_names: BTreeSet::new(),
        missing_fields: vec!["request/stage/queue shape mismatch".to_string()],
        slow_request_ordering_preserved: false,
        is_equivalent: false,
    };

    let message = report.failure_message();
    assert!(message.contains("native_requests=3"));
    assert!(message.contains("tracing_requests=2"));
    assert!(message.contains("shape mismatch"));
}

#[tokio::test(flavor = "current_thread")]
async fn normalization_does_not_mutate_original_runs() {
    let native = run_native_scenario().await;
    let before = native.clone();

    let _normalized = normalize_run(&native);

    assert_eq!(native, before);
}
