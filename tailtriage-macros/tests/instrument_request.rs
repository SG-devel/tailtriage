use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use tailtriage_core::{Config, Tailtriage};
use tailtriage_macros::instrument_request;
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::prelude::*;

#[derive(Default, Clone)]
struct RecordedEvents {
    lines: Arc<Mutex<Vec<String>>>,
}

impl RecordedEvents {
    fn push(&self, value: String) {
        self.lines.lock().expect("event mutex poisoned").push(value);
    }

    fn snapshot(&self) -> Vec<String> {
        self.lines.lock().expect("event mutex poisoned").clone()
    }
}

#[derive(Clone)]
struct CaptureLayer {
    events: RecordedEvents,
}

impl<S> Layer<S> for CaptureLayer
where
    S: Subscriber,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        struct Visit {
            parts: Vec<String>,
        }

        impl tracing::field::Visit for Visit {
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                self.parts.push(format!("{}={value:?}", field.name()));
            }
        }

        let mut visitor = Visit { parts: Vec::new() };
        event.record(&mut visitor);
        self.events.push(visitor.parts.join(" "));
    }
}

#[instrument_request(route = "/invoice", kind = "create_invoice", skip(state))]
async fn ok_handler(state: u32) -> Result<u32, &'static str> {
    let _ = state;
    Ok(42)
}

#[instrument_request(route = "/invoice", kind = "create_invoice", skip(state))]
async fn err_handler(state: u32) -> Result<u32, &'static str> {
    let _ = state;
    Err("boom")
}

#[instrument_request(
    route = "/macro",
    kind = "macro_collector",
    tailtriage = tailtriage,
    request_id = request_id.clone(),
    skip(tailtriage)
)]
async fn collector_handler(
    tailtriage: &Tailtriage,
    request_id: String,
    succeed: bool,
) -> Result<&'static str, &'static str> {
    if succeed {
        Ok("ok")
    } else {
        Err("boom")
    }
}

#[tokio::test]
async fn records_ok_and_error_outcomes() {
    let recorded = RecordedEvents::default();
    let layer = CaptureLayer {
        events: recorded.clone(),
    };

    let subscriber = tracing_subscriber::registry().with(layer);
    let _guard = tracing::subscriber::set_default(subscriber);

    let value = ok_handler(1).await.expect("ok handler should succeed");
    assert_eq!(value, 42);
    let err = err_handler(2).await.expect_err("err handler should fail");
    assert_eq!(err, "boom");

    let events = recorded.snapshot();
    let tail_events: Vec<_> = events
        .iter()
        .filter(|line| line.contains("outcome"))
        .cloned()
        .collect();

    assert_eq!(tail_events.len(), 2, "expected two completion events");
    assert!(tail_events
        .iter()
        .any(|line| line.contains("outcome=\"ok\"")));
    assert!(tail_events
        .iter()
        .any(|line| line.contains("outcome=\"error\"")));
    assert!(tail_events.iter().all(|line| line.contains("duration_us=")));
    assert!(tail_events
        .iter()
        .all(|line| line.contains("route=\"/invoice\"")));
    assert!(tail_events
        .iter()
        .all(|line| line.contains("kind=\"create_invoice\"")));
}

#[tokio::test]
async fn writes_request_events_to_run_json_when_tailtriage_is_provided() {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before epoch")
        .as_nanos();
    let output_path = std::env::temp_dir().join(format!("tailtriage_macro_run_{nanos}.json"));

    let mut config = Config::new("macro-test");
    config.output_path = output_path.clone();

    let tailtriage = Tailtriage::init(config).expect("init should succeed");

    let ok = collector_handler(&tailtriage, "req-ok".to_string(), true)
        .await
        .expect("ok request should succeed");
    assert_eq!(ok, "ok");

    let err = collector_handler(&tailtriage, "req-err".to_string(), false)
        .await
        .expect_err("error request should fail");
    assert_eq!(err, "boom");

    tailtriage.shutdown().expect("flush should succeed");

    let run_json = std::fs::read_to_string(&output_path).expect("run artifact should be readable");
    let run_value: serde_json::Value =
        serde_json::from_str(&run_json).expect("run artifact should be valid json");
    let requests = run_value["requests"]
        .as_array()
        .expect("run artifact requests should be an array");

    assert_eq!(requests.len(), 2);
    assert!(requests.iter().any(|event| event["request_id"] == "req-ok"));
    assert!(requests
        .iter()
        .any(|event| event["request_id"] == "req-err"));
    assert!(requests.iter().any(|event| event["outcome"] == "ok"));
    assert!(requests.iter().any(|event| event["outcome"] == "error"));
}
