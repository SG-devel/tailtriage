use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::time::Duration;

use tailtriage_core::{MemorySink, RequestOptions, Run, Tailtriage};
use tailtriage_tracing::TracingRecorder;
use tokio::runtime::Builder;
use tracing::Span;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::Registry;

#[derive(Debug, Clone, PartialEq, Eq)]
struct CanonicalRequestSpec {
    request_id: &'static str,
    queue_delay_ms: u64,
    stage_db_ms: u64,
    stage_render_ms: u64,
}

const ROUTE: &str = "/checkout";
const QUEUE_NAME: &str = "db_pool";
const STAGE_DB: &str = "db.query";
const STAGE_RENDER: &str = "render.response";

const CANONICAL_SCENARIO: [CanonicalRequestSpec; 3] = [
    CanonicalRequestSpec {
        request_id: "req-fast-1",
        queue_delay_ms: 2,
        stage_db_ms: 2,
        stage_render_ms: 2,
    },
    CanonicalRequestSpec {
        request_id: "req-slow-1",
        queue_delay_ms: 8,
        stage_db_ms: 9,
        stage_render_ms: 9,
    },
    CanonicalRequestSpec {
        request_id: "req-fast-2",
        queue_delay_ms: 3,
        stage_db_ms: 2,
        stage_render_ms: 2,
    },
];

#[derive(Debug, Clone)]
struct ArtifactEquivalenceReport {
    native_request_count: usize,
    tracing_request_count: usize,
    matched_routes: BTreeSet<String>,
    matched_stage_names: BTreeSet<String>,
    matched_queue_names: BTreeSet<String>,
    missing_fields: Vec<String>,
    relative_latency_ordering_preserved: bool,
    is_equivalent: bool,
}

impl ArtifactEquivalenceReport {
    fn from_runs(native: &Run, tracing: &Run) -> Self {
        let native_by_request = correlation_shape(native);
        let tracing_by_request = correlation_shape(tracing);

        let native_routes = native
            .requests
            .iter()
            .map(|event| event.route.clone())
            .collect::<BTreeSet<_>>();
        let tracing_routes = tracing
            .requests
            .iter()
            .map(|event| event.route.clone())
            .collect::<BTreeSet<_>>();

        let native_stages = native
            .stages
            .iter()
            .map(|event| event.stage.clone())
            .collect::<BTreeSet<_>>();
        let tracing_stages = tracing
            .stages
            .iter()
            .map(|event| event.stage.clone())
            .collect::<BTreeSet<_>>();

        let native_queues = native
            .queues
            .iter()
            .map(|event| event.queue.clone())
            .collect::<BTreeSet<_>>();
        let tracing_queues = tracing
            .queues
            .iter()
            .map(|event| event.queue.clone())
            .collect::<BTreeSet<_>>();

        let mut missing_fields = Vec::new();
        if native.requests.len() != tracing.requests.len() {
            missing_fields.push(format!(
                "request count differs: native={} tracing={}",
                native.requests.len(),
                tracing.requests.len()
            ));
        }
        if native_routes != tracing_routes {
            missing_fields.push(format!(
                "route set differs: native={native_routes:?} tracing={tracing_routes:?}"
            ));
        }
        if native_stages != tracing_stages {
            missing_fields.push(format!(
                "stage set differs: native={native_stages:?} tracing={tracing_stages:?}"
            ));
        }
        if native_queues != tracing_queues {
            missing_fields.push(format!(
                "queue set differs: native={native_queues:?} tracing={tracing_queues:?}"
            ));
        }
        if native_by_request != tracing_by_request {
            missing_fields.push(format!(
                "request correlation shape differs: native={native_by_request:?} tracing={tracing_by_request:?}"
            ));
        }

        let native_positive_latencies = all_latencies_positive(native);
        let tracing_positive_latencies = all_latencies_positive(tracing);
        if !native_positive_latencies {
            missing_fields.push("native run has non-positive latency".to_owned());
        }
        if !tracing_positive_latencies {
            missing_fields.push("tracing run has non-positive latency".to_owned());
        }

        let relative_latency_ordering_preserved =
            request_latency_ordering(native) == request_latency_ordering(tracing);
        if !relative_latency_ordering_preserved {
            missing_fields.push("relative request latency ordering is not preserved".to_owned());
        }

        let is_equivalent = missing_fields.is_empty();

        Self {
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
            relative_latency_ordering_preserved,
            is_equivalent,
        }
    }
}

impl fmt::Display for ArtifactEquivalenceReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "equivalent={} native_requests={} tracing_requests={} matched_routes={:?} matched_stages={:?} matched_queues={:?} latency_ordering_preserved={} missing={:?}",
            self.is_equivalent,
            self.native_request_count,
            self.tracing_request_count,
            self.matched_routes,
            self.matched_stage_names,
            self.matched_queue_names,
            self.relative_latency_ordering_preserved,
            self.missing_fields
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RequestShape {
    route: String,
    stage_names: BTreeSet<String>,
    queue_names: BTreeSet<String>,
}

fn correlation_shape(run: &Run) -> BTreeMap<String, RequestShape> {
    let mut by_request: BTreeMap<String, RequestShape> = BTreeMap::new();

    for request in &run.requests {
        by_request
            .entry(request.request_id.clone())
            .or_insert(RequestShape {
                route: request.route.clone(),
                stage_names: BTreeSet::new(),
                queue_names: BTreeSet::new(),
            });
    }

    for stage in &run.stages {
        by_request
            .entry(stage.request_id.clone())
            .or_insert(RequestShape {
                route: String::new(),
                stage_names: BTreeSet::new(),
                queue_names: BTreeSet::new(),
            })
            .stage_names
            .insert(stage.stage.clone());
    }

    for queue in &run.queues {
        by_request
            .entry(queue.request_id.clone())
            .or_insert(RequestShape {
                route: String::new(),
                stage_names: BTreeSet::new(),
                queue_names: BTreeSet::new(),
            })
            .queue_names
            .insert(queue.queue.clone());
    }

    by_request
}

fn all_latencies_positive(run: &Run) -> bool {
    run.requests.iter().all(|event| event.latency_us > 0)
        && run.stages.iter().all(|event| event.latency_us > 0)
        && run.queues.iter().all(|event| event.wait_us > 0)
}

fn request_latency_ordering(run: &Run) -> Vec<String> {
    let mut values = run
        .requests
        .iter()
        .map(|event| (event.request_id.clone(), event.latency_us))
        .collect::<Vec<_>>();
    values.sort_by_key(|(_, latency)| *latency);
    values.into_iter().map(|(id, _)| id).collect()
}

fn normalize_run(run: &Run) -> Run {
    let mut normalized = run.clone();
    "normalized".clone_into(&mut normalized.metadata.run_id);
    normalized.metadata.started_at_unix_ms = 0;
    normalized.metadata.finished_at_unix_ms = 0;
    normalized.metadata.finalized_at_unix_ms = None;

    for request in &mut normalized.requests {
        request.started_at_unix_ms = 0;
        request.finished_at_unix_ms = 0;
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
        .sort_by(|a, b| a.request_id.cmp(&b.request_id));
    normalized
        .stages
        .sort_by(|a, b| (&a.request_id, &a.stage).cmp(&(&b.request_id, &b.stage)));
    normalized
        .queues
        .sort_by(|a, b| (&a.request_id, &a.queue).cmp(&(&b.request_id, &b.queue)));

    normalized
}

fn native_run_for_canonical_scenario() -> Run {
    let runtime = Builder::new_current_thread()
        .enable_time()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let tailtriage = Tailtriage::builder("equivalence-native")
            .sink(MemorySink::default())
            .build()
            .expect("native tailtriage builder");

        for request in CANONICAL_SCENARIO {
            let started = tailtriage.begin_request_with(
                ROUTE,
                RequestOptions::new()
                    .request_id(request.request_id)
                    .kind("http"),
            );
            started
                .handle
                .queue(QUEUE_NAME)
                .await_on(tokio::time::sleep(Duration::from_millis(
                    request.queue_delay_ms,
                )))
                .await;

            started
                .handle
                .stage(STAGE_DB)
                .await_value(tokio::time::sleep(Duration::from_millis(
                    request.stage_db_ms,
                )))
                .await;

            started
                .handle
                .stage(STAGE_RENDER)
                .await_value(tokio::time::sleep(Duration::from_millis(
                    request.stage_render_ms,
                )))
                .await;
            started.completion.finish_ok();
        }

        tailtriage.snapshot()
    })
}

fn tracing_run_for_canonical_scenario() -> Run {
    let runtime = Builder::new_current_thread()
        .enable_time()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let recorder = TracingRecorder::builder("equivalence-tracing").build();
        let subscriber = Registry::default().with(recorder.layer());

        tracing::subscriber::with_default(subscriber, || {
            for request in CANONICAL_SCENARIO {
                let request_span = tracing::info_span!(
                    "request",
                    tt.kind = "request",
                    tt.request_id = request.request_id,
                    tt.route = ROUTE,
                    tt.outcome = "ok"
                );
                let _request_guard = request_span.enter();

                let queue_span = tracing::info_span!(
                    "queue",
                    tt.kind = "queue",
                    tt.request_id = request.request_id,
                    tt.queue = QUEUE_NAME
                );
                with_span_sleep(&queue_span, request.queue_delay_ms);

                let stage_db_span = tracing::info_span!(
                    "stage_db",
                    tt.kind = "stage",
                    tt.request_id = request.request_id,
                    tt.stage = STAGE_DB,
                    tt.success = true
                );
                with_span_sleep(&stage_db_span, request.stage_db_ms);

                let stage_render_span = tracing::info_span!(
                    "stage_render",
                    tt.kind = "stage",
                    tt.request_id = request.request_id,
                    tt.stage = STAGE_RENDER,
                    tt.success = true
                );
                with_span_sleep(&stage_render_span, request.stage_render_ms);
            }
        });

        recorder
            .shutdown()
            .expect("tracing recorder import")
            .run()
            .clone()
    })
}

fn with_span_sleep(span: &Span, delay_ms: u64) {
    let _guard = span.enter();
    std::thread::sleep(Duration::from_millis(delay_ms));
}

#[test]
fn native_and_tracing_artifacts_are_equivalent_after_normalization() {
    let native = native_run_for_canonical_scenario();
    let tracing = tracing_run_for_canonical_scenario();

    assert!(native.runtime_snapshots.is_empty());
    assert!(tracing.runtime_snapshots.is_empty());

    let report =
        ArtifactEquivalenceReport::from_runs(&normalize_run(&native), &normalize_run(&tracing));

    assert!(report.is_equivalent, "{report}");
}

#[test]
fn equivalence_report_failure_message_is_actionable() {
    let native = native_run_for_canonical_scenario();
    let mut tracing = tracing_run_for_canonical_scenario();
    tracing.queues.clear();

    let report =
        ArtifactEquivalenceReport::from_runs(&normalize_run(&native), &normalize_run(&tracing));

    assert!(!report.is_equivalent);
    let message = report.to_string();
    assert!(message.contains("queue set differs"), "{message}");
    assert!(
        message.contains("request correlation shape differs"),
        "{message}"
    );
}

#[test]
fn normalization_returns_new_run_and_does_not_mutate_original() {
    let run = native_run_for_canonical_scenario();
    let original = run.clone();

    let normalized = normalize_run(&run);

    assert_eq!(run, original);
    assert_eq!(normalized.metadata.run_id, "normalized");
    assert_eq!(normalized.metadata.started_at_unix_ms, 0);
    assert_eq!(normalized.metadata.finished_at_unix_ms, 0);
}
