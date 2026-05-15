# tailtriage-tracing

`tailtriage-tracing` is a focused intake bridge for tracing-shaped span data that
can be converted into `tailtriage_core::Run` triage inputs.

This crate provides:

- semantic convention keys (`tt.*`) for triage-oriented span fields
- typed intake records and import option/result types
- `run_from_span_records` for converting `SpanRecord` values into `Run`
- JSONL import helpers for completed spans
- a live `tracing_subscriber::Layer` recorder for completed spans

This crate does **not** provide OTel/OTLP integration and does **not** change
`tailtriage-analyzer` behavior.

## Live tracing recorder

Public APIs:

- `TracingRecorder`
- `TracingRecorderBuilder`
- `TailtriageLayer`

Minimal example:

```rust
use tracing_subscriber::{layer::SubscriberExt, Registry};
use tailtriage_tracing::TracingRecorder;

let recorder = TracingRecorder::builder("checkout-service")
    .service_version("1.2.3")
    .build();

let subscriber = Registry::default().with(recorder.layer());
tracing::subscriber::with_default(subscriber, || {
    let request = tracing::info_span!(
        "http.request",
        tt.kind = "request",
        tt.request_id = "req-42",
        tt.route = "/checkout"
    );
    let _entered = request.enter();
});

let imported = recorder.snapshot_run()?;
assert_eq!(imported.run().requests.len(), 1);
# Ok::<(), tailtriage_tracing::ImportError>(())
```

Async instrumentation note:

- Prefer `#[tracing::instrument(fields(...))]` or `.instrument(...)`.
- Do not hold a manual entered-span guard across `.await`.

Runtime-pressure evidence note:

- tracing spans work outside Tokio for request/stage/queue evidence.
- Runtime-pressure evidence still requires tailtriage Tokio runtime sampling or
  future runtime metrics import.

## JSONL import support

Public APIs:

- `import_jsonl_reader(reader, options)`
- `import_jsonl_path(path, options)`

Supported stable contract (recommended for tests and integrations):

```json
{
  "span": {
    "name": "http.request",
    "id": "span-1",
    "parent_id": "root-1",
    "started_at_unix_ms": 1700000000000,
    "finished_at_unix_ms": 1700000000120,
    "fields": {
      "tt.kind": "request",
      "tt.request_id": "req-42",
      "tt.route": "/checkout"
    }
  }
}
```
