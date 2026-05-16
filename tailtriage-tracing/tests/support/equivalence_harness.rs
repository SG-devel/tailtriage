use std::collections::{BTreeMap, BTreeSet};

use std::time::Duration;
use tailtriage_core::{RequestOptions, Run, Tailtriage};
use tailtriage_tracing::TracingRecorder;
use tracing_subscriber::prelude::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactEquivalenceReport {
    pub native_request_count: usize,
    pub tracing_request_count: usize,
    pub native_routes: BTreeSet<String>,
    pub tracing_routes: BTreeSet<String>,
    pub native_stage_names: BTreeSet<String>,
    pub tracing_stage_names: BTreeSet<String>,
    pub native_queue_names: BTreeSet<String>,
    pub tracing_queue_names: BTreeSet<String>,
    pub correlation_shapes_match: bool,
    pub latency_signals_match: bool,
    pub missing_fields: Vec<String>,
    pub is_equivalent: bool,
}

impl ArtifactEquivalenceReport {
    pub fn failure_message(&self) -> String {
        format!(
            "is_equivalent={} native_requests={} tracing_requests={} native_routes={:?} tracing_routes={:?} native_stages={:?} tracing_stages={:?} native_queues={:?} tracing_queues={:?} correlation_shapes_match={} latency_signals_match={} missing_fields={:?}",
            self.is_equivalent,
            self.native_request_count,
            self.tracing_request_count,
            self.native_routes,
            self.tracing_routes,
            self.native_stage_names,
            self.tracing_stage_names,
            self.native_queue_names,
            self.tracing_queue_names,
            self.correlation_shapes_match,
            self.latency_signals_match,
            self.missing_fields,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedRun {
    pub requests: Vec<NormalizedRequest>,
    pub stages: Vec<NormalizedStage>,
    pub queues: Vec<NormalizedQueue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Ord, PartialOrd)]
pub struct NormalizedRequest {
    pub request_id: String,
    pub route: String,
    pub kind: Option<String>,
    pub outcome: String,
    pub latency_positive: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Ord, PartialOrd)]
pub struct NormalizedStage {
    pub request_id: String,
    pub stage: String,
    pub success: bool,
    pub latency_positive: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Ord, PartialOrd)]
pub struct NormalizedQueue {
    pub request_id: String,
    pub queue: String,
    pub depth_at_start: Option<u64>,
    pub latency_positive: bool,
}

pub fn normalize_run(run: &Run) -> NormalizedRun {
    let mut requests = run
        .requests
        .iter()
        .map(|request| NormalizedRequest {
            request_id: request.request_id.clone(),
            route: request.route.clone(),
            kind: request.kind.clone(),
            outcome: request.outcome.clone(),
            latency_positive: request.latency_us > 0,
        })
        .collect::<Vec<_>>();
    requests.sort();

    let mut stages = run
        .stages
        .iter()
        .map(|stage| NormalizedStage {
            request_id: stage.request_id.clone(),
            stage: stage.stage.clone(),
            success: stage.success,
            latency_positive: stage.latency_us > 0,
        })
        .collect::<Vec<_>>();
    stages.sort();

    let mut queues = run
        .queues
        .iter()
        .map(|queue| NormalizedQueue {
            request_id: queue.request_id.clone(),
            queue: queue.queue.clone(),
            depth_at_start: queue.depth_at_start,
            latency_positive: queue.wait_us > 0,
        })
        .collect::<Vec<_>>();
    queues.sort();

    NormalizedRun {
        requests,
        stages,
        queues,
    }
}

pub fn equivalence_report(native: &Run, tracing: &Run) -> ArtifactEquivalenceReport {
    let native_norm = normalize_run(native);
    let tracing_norm = normalize_run(tracing);

    let native_routes = native_norm
        .requests
        .iter()
        .map(|r| r.route.clone())
        .collect::<BTreeSet<_>>();
    let tracing_routes = tracing_norm
        .requests
        .iter()
        .map(|r| r.route.clone())
        .collect::<BTreeSet<_>>();

    let native_stage_names = native_norm
        .stages
        .iter()
        .map(|s| s.stage.clone())
        .collect::<BTreeSet<_>>();
    let tracing_stage_names = tracing_norm
        .stages
        .iter()
        .map(|s| s.stage.clone())
        .collect::<BTreeSet<_>>();

    let native_queue_names = native_norm
        .queues
        .iter()
        .map(|q| q.queue.clone())
        .collect::<BTreeSet<_>>();
    let tracing_queue_names = tracing_norm
        .queues
        .iter()
        .map(|q| q.queue.clone())
        .collect::<BTreeSet<_>>();

    let native_shape = build_shape_map(&native_norm);
    let tracing_shape = build_shape_map(&tracing_norm);
    let correlation_shapes_match = native_shape == tracing_shape;

    let latency_signals_match = all_positive(&native_norm, &tracing_norm)
        && slowest_request_id(native) == slowest_request_id(tracing);

    let mut missing_fields = Vec::new();
    if native.requests.len() != tracing.requests.len() {
        missing_fields.push("request count mismatch".to_string());
    }
    if native_routes != tracing_routes {
        missing_fields.push("route set mismatch".to_string());
    }
    if native_stage_names != tracing_stage_names {
        missing_fields.push("stage name set mismatch".to_string());
    }
    if native_queue_names != tracing_queue_names {
        missing_fields.push("queue name set mismatch".to_string());
    }
    if !correlation_shapes_match {
        missing_fields.push("request/stage/queue correlation shape mismatch".to_string());
    }
    if !latency_signals_match {
        missing_fields.push("latency positivity or relative ordering mismatch".to_string());
    }

    let is_equivalent = missing_fields.is_empty();

    ArtifactEquivalenceReport {
        native_request_count: native.requests.len(),
        tracing_request_count: tracing.requests.len(),
        native_routes,
        tracing_routes,
        native_stage_names,
        tracing_stage_names,
        native_queue_names,
        tracing_queue_names,
        correlation_shapes_match,
        latency_signals_match,
        missing_fields,
        is_equivalent,
    }
}

fn build_shape_map(run: &NormalizedRun) -> BTreeMap<String, (BTreeSet<String>, BTreeSet<String>)> {
    let mut map = BTreeMap::new();
    for request in &run.requests {
        map.insert(
            request.request_id.clone(),
            (BTreeSet::new(), BTreeSet::new()),
        );
    }
    for stage in &run.stages {
        if let Some((stages, _)) = map.get_mut(&stage.request_id) {
            stages.insert(stage.stage.clone());
        }
    }
    for queue in &run.queues {
        if let Some((_, queues)) = map.get_mut(&queue.request_id) {
            queues.insert(queue.queue.clone());
        }
    }
    map
}

fn all_positive(native: &NormalizedRun, tracing: &NormalizedRun) -> bool {
    native.requests.iter().all(|r| r.latency_positive)
        && tracing.requests.iter().all(|r| r.latency_positive)
        && native.stages.iter().all(|s| s.latency_positive)
        && tracing.stages.iter().all(|s| s.latency_positive)
        && native.queues.iter().all(|q| q.latency_positive)
        && tracing.queues.iter().all(|q| q.latency_positive)
}

fn slowest_request_id(run: &Run) -> Option<String> {
    run.requests
        .iter()
        .max_by_key(|r| r.latency_us)
        .map(|r| r.request_id.clone())
}

pub async fn run_native_scenario() -> Run {
    let tailtriage = Tailtriage::builder("equivalence-service")
        .build()
        .expect("native tailtriage should build");

    run_scenario_native(&tailtriage).await;
    tailtriage.snapshot()
}

async fn run_scenario_native(tailtriage: &Tailtriage) {
    for (request_id, extra_ms) in [("req-1", 0_u64), ("req-2", 0_u64), ("req-3", 8_u64)] {
        let started = tailtriage
            .begin_request_with("/checkout", RequestOptions::new().request_id(request_id));
        started
            .handle
            .queue("db-pool")
            .with_depth_at_start(3)
            .await_on(async {
                std::thread::sleep(Duration::from_millis(1));
            })
            .await;
        started
            .handle
            .stage("db.query")
            .await_value(async {
                std::thread::sleep(Duration::from_millis(2));
            })
            .await;
        started
            .handle
            .stage("render")
            .await_value(async {
                std::thread::sleep(Duration::from_millis(1 + extra_ms));
            })
            .await;
        started.completion.finish_ok();
    }
}

pub async fn run_tracing_scenario() -> Run {
    let recorder = TracingRecorder::builder("equivalence-service")
        .run_id("tracing-eq")
        .strict(true)
        .build();
    let subscriber = tracing_subscriber::registry().with(recorder.layer());

    tracing::subscriber::with_default(subscriber, || {
        for (request_id, extra_ms) in [("req-1", 0_u64), ("req-2", 0_u64), ("req-3", 8_u64)] {
            let request = tracing::info_span!(
                "request",
                tt.kind = "request",
                tt.request_id = request_id,
                tt.route = "/checkout",
                tt.outcome = "ok"
            );
            let _request_guard = request.enter();

            let queue = tracing::info_span!(
                "queue",
                tt.kind = "queue",
                tt.request_id = request_id,
                tt.queue = "db-pool",
                tt.depth_at_start = 3_u64
            );
            {
                let _queue_guard = queue.enter();
                std::thread::sleep(Duration::from_millis(1));
            }

            let stage_1 = tracing::info_span!(
                "stage_1",
                tt.kind = "stage",
                tt.request_id = request_id,
                tt.stage = "db.query",
                tt.success = true
            );
            {
                let _stage_guard = stage_1.enter();
                std::thread::sleep(Duration::from_millis(2));
            }

            let stage_2 = tracing::info_span!(
                "stage_2",
                tt.kind = "stage",
                tt.request_id = request_id,
                tt.stage = "render",
                tt.success = true
            );
            {
                let _stage_guard = stage_2.enter();
                std::thread::sleep(Duration::from_millis(1 + extra_ms));
            }
        }
    });

    recorder
        .snapshot_run()
        .expect("tracing run should import")
        .run()
        .clone()
}
