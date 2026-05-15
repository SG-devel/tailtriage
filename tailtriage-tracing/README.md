# tailtriage-tracing

`tailtriage-tracing` is a focused intake bridge surface for tracing-shaped span
records that can be converted into `tailtriage_core::Run` inputs.

This crate is intentionally narrow:

- It defines semantic convention keys (`tt.*`) for triage-oriented span fields.
- It defines typed intake records and import option/result types.
- It provides a live `tracing_subscriber::Layer` recorder for completed spans.
- It converts typed `SpanRecord` values with `run_from_span_records`.
- It imports JSONL from readers/paths when records contain completed span timing.
- It does **not** implement OpenTelemetry or OTLP.
- It does **not** change `tailtriage-analyzer`.

## Live recorder example

```rust
use tracing_subscriber::{layer::SubscriberExt, Registry};
use tailtriage_tracing::TracingRecorder;

let recorder = TracingRecorder::builder("checkout-service")
    .service_version("1.2.3")
    .run_id("run-42")
    .strict(false)
    .build();

let subscriber = Registry::default().with(recorder.layer());
let _guard = tracing::subscriber::set_default(subscriber);

let span = tracing::info_span!(
    "http.request",
    "tt.kind" = "request",
    "tt.request_id" = "req-42",
    "tt.route" = "/checkout",
    "tt.outcome" = "ok"
);
let _entered = span.enter();

drop(_entered);
drop(span); // close span

let imported = recorder.snapshot_run()?;
assert_eq!(imported.run().requests.len(), 1);
# Ok::<(), tailtriage_tracing::ImportError>(())
```

Notes:

- For async code, prefer `#[tracing::instrument(fields(...))]` or `.instrument(...)` so span lifetime tracks future execution.
- Do not hold manually entered-span guards across `.await`.
- Request/stage/queue span capture works outside Tokio, but runtime-pressure evidence still requires tailtriage's Tokio sampler (or a future compatible runtime-metrics import path).

## JSONL import support in this phase

Public APIs:

- `import_jsonl_reader(reader, options)`
- `import_jsonl_path(path, options)`
