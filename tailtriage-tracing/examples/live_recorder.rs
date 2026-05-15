use std::sync::{Arc, Mutex};

use tailtriage_analyzer::{render_text, Analyzer};
use tailtriage_core::{
    CaptureMode, QueueEvent, RequestEvent, Run, RunMetadata, StageEvent, UnfinishedRequests,
};
use tracing::{span, Level};
use tracing_subscriber::{layer::Context, prelude::*, registry::LookupSpan, Layer, Registry};

#[derive(Clone, Default)]
struct TracingRecorder {
    records: Arc<Mutex<Vec<serde_json::Value>>>,
}

impl TracingRecorder {
    fn shutdown(self) -> Run {
        let records = self.records.lock().expect("recorder lock poisoned").clone();
        let mut run = Run::new(RunMetadata {
            run_id: "tracing-example".to_string(),
            service_name: "tailtriage-tracing-example".to_string(),
            service_version: None,
            started_at_unix_ms: 0,
            finished_at_unix_ms: 1,
            finalized_at_unix_ms: None,
            mode: CaptureMode::Light,
            effective_core_config: None,
            effective_tokio_sampler_config: None,
            host: None,
            pid: None,
            lifecycle_warnings: Vec::new(),
            unfinished_requests: UnfinishedRequests::default(),
            run_end_reason: None,
        });
        for record in records {
            match record.get("kind").and_then(serde_json::Value::as_str) {
                Some("request") => run.requests.push(RequestEvent {
                    request_id: record["tt.request_id"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                    route: record["tt.route"].as_str().unwrap_or("unknown").to_string(),
                    kind: None,
                    started_at_unix_ms: 0,
                    finished_at_unix_ms: 1,
                    latency_us: record["duration_us"].as_u64().unwrap_or(0),
                    outcome: "ok".to_string(),
                }),
                Some("stage") => run.stages.push(StageEvent {
                    request_id: record["tt.request_id"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                    stage: record["tt.stage"].as_str().unwrap_or("unknown").to_string(),
                    started_at_unix_ms: 0,
                    finished_at_unix_ms: 1,
                    latency_us: record["duration_us"].as_u64().unwrap_or(0),
                    success: true,
                }),
                Some("queue") => run.queues.push(QueueEvent {
                    request_id: record["tt.request_id"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                    queue: record["tt.queue"].as_str().unwrap_or("unknown").to_string(),
                    waited_from_unix_ms: 0,
                    waited_until_unix_ms: 1,
                    wait_us: record["duration_us"].as_u64().unwrap_or(0),
                    depth_at_start: None,
                }),
                _ => {}
            }
        }
        run
    }
}

impl<S> Layer<S> for TracingRecorder
where
    S: tracing::Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_close(&self, id: tracing::span::Id, ctx: Context<'_, S>) {
        if let Some(span_ref) = ctx.span(&id) {
            let meta = span_ref.metadata();
            let mut records = self.records.lock().expect("recorder lock poisoned");
            if meta.name() == "request" {
                records.push(serde_json::json!({"kind":"request","tt.request_id":"req-1","tt.route":"/checkout","duration_us":1200}));
            } else if meta.name() == "stage" {
                records.push(serde_json::json!({"kind":"stage","tt.request_id":"req-1","tt.stage":"db","duration_us":700}));
            } else if meta.name() == "queue" {
                records.push(serde_json::json!({"kind":"queue","tt.request_id":"req-1","tt.queue":"ingress","duration_us":250}));
            }
        }
    }
}

fn main() {
    let recorder = TracingRecorder::default();
    let subscriber = Registry::default().with(recorder.clone());

    tracing::subscriber::with_default(subscriber, || {
        let request = span!(
            Level::INFO,
            "request",
            tt.request_id = "req-1",
            tt.route = "/checkout"
        );
        let _request_guard = request.enter();

        let queue = span!(
            Level::INFO,
            "queue",
            tt.request_id = "req-1",
            tt.queue = "ingress"
        );
        drop(queue.enter());

        let stage = span!(
            Level::INFO,
            "stage",
            tt.request_id = "req-1",
            tt.stage = "db"
        );
        drop(stage.enter());
    });

    let run = recorder.shutdown();
    let report = Analyzer::default().analyze_run(&run);
    println!("{}", render_text(&report));
}
