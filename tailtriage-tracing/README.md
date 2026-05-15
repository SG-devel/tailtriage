# tailtriage-tracing

`tailtriage-tracing` is a focused intake bridge for tracing-shaped span
records that can be converted into `tailtriage_core::Run` inputs.

This crate provides:

- semantic convention keys (`tt.*`) for triage-oriented span fields,
- typed intake records and import option/result types,
- JSONL import helpers,
- a live `tracing_subscriber::Layer` recorder for completed spans.

This crate does **not** add OTel/OTLP export and does **not** change analyzer behavior.

## Live recorder example

```rust
use tracing::info_span;
use tracing_subscriber::prelude::*;
use tailtriage_tracing::TracingRecorder;

let recorder = TracingRecorder::builder("checkout-service")
    .service_version("1.2.3")
    .run_id("demo-run")
    .strict(false)
    .build();

let subscriber = tracing_subscriber::registry().with(recorder.layer());
tracing::subscriber::with_default(subscriber, || {
    let request = info_span!(
        "http.request",
        tt.kind = "request",
        tt.request_id = "req-42",
        tt.route = "/checkout",
    );
    let _request_guard = request.enter();

    let stage = info_span!(
        "db.stage",
        tt.kind = "stage",
        tt.request_id = "req-42",
        tt.stage = "db",
        tt.success = true,
    );
    let _stage_guard = stage.enter();
});

let imported = recorder.snapshot_run()?;
assert_eq!(imported.run().requests.len(), 1);
# Ok::<(), tailtriage_tracing::ImportError>(())
```

## Async span guidance

For async code, prefer `#[tracing::instrument(fields(...))]` or future instrumentation with
`.instrument(...)` so spans close at the right lifecycle boundaries. Avoid holding a manual
entered-span guard across `.await` points.

## Runtime-pressure evidence note

Tracing spans can be captured in non-Tokio runtimes for request/stage/queue triage evidence.
Runtime-pressure evidence (executor pressure and blocking-pool pressure) still requires
`tailtriage` runtime sampling (or future runtime metrics import) to be present in the run.

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
