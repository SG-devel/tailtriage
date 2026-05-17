# tailtriage-tracing

`tailtriage-tracing` is a focused intake bridge surface for tracing-shaped span
records that can be converted into `tailtriage_core::Run` inputs.

This crate is intentionally narrow:

- It defines semantic convention keys (`tt.*`) for triage-oriented span fields.
- It defines typed intake records and import option/result types.
- It converts typed `SpanRecord` values with `run_from_span_records`.
- It imports JSONL from readers/paths when records contain completed span timing.
- It provides an in-process `tracing_subscriber::Layer` recorder for completed `tt.*` spans.
- It does **not** implement OpenTelemetry or OTLP.
- It does **not** change `tailtriage-analyzer`.

Both JSONL import and live recorder intake produce standard `tailtriage_core::Run` values for the same analyzer/report workflow.

## JSONL import support in this phase

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

Notes:

- Importer accepts `started_at_unix_ms`/`finished_at_unix_ms` and aliases `start_unix_ms`/`end_unix_ms`.
- In this phase, normalized shape uses **literal dotted keys** inside `fields` (for example `"tt.kind"` and `"tt.request_id"`), not nested objects that require flattening.
- Importer reads `tt.*` fields from `fields`, `span.fields`, or top-level `tt.*` keys when present.
- Scalars can be strings, bools, numbers, or null.
- Empty lines are ignored.
- Malformed JSON line input is an import error in both strict and non-strict mode.
- In non-strict mode, syntactically valid but malformed/incomplete `tt.*` records are skipped with warnings.
- In strict mode, malformed/incomplete `tt.*` records are import errors.

CLI import for the same shape:

```bash
tailtriage import tracing-json spans.jsonl --service checkout --output tailtriage-run.json
tailtriage analyze tailtriage-run.json
```

## tracing-subscriber JSON caveat

Direct `tracing-subscriber` JSON output can vary by formatter configuration. In
this phase, the importer supports:

- normalized completed-span JSONL (shape above), and
- close-event-like records only when they include explicit start/end unix-ms timestamps.

Close-event-like records are supported only when explicit unix-ms start/end timestamps are present; timing is not guessed from line receive time, and broad compatibility with arbitrary tracing JSON is not claimed.

## Intended field shape

Typical span fields are expected to follow this shape:

- request: `tt.kind`, `tt.request_id`, `tt.route`
- stage: `tt.kind`, `tt.request_id`, `tt.stage`
- queue: `tt.kind`, `tt.request_id`, `tt.queue`, `tt.depth_at_start`


## Live tracing recorder

```rust
use tracing_subscriber::prelude::*;
use tailtriage_tracing::TracingRecorder;

let recorder = TracingRecorder::builder("checkout-service")
    .service_version("1.2.3")
    .run_id("run-42")
    .strict(false)
    .build();

let subscriber = tracing_subscriber::registry().with(recorder.layer());
tracing::subscriber::with_default(subscriber, || {
    {
        let request = tracing::info_span!(
            "http.request",
            tt.kind = "request",
            tt.request_id = "req-42",
            tt.route = "/checkout",
            tt.outcome = tracing::field::Empty
        );
        let _entered = request.enter();
        request.record("tt.outcome", "ok");
    }
});

let imported = recorder.shutdown()?;
let run = imported.run();
assert_eq!(run.requests.len(), 1);
# Ok::<(), tailtriage_tracing::ImportError>(())
```

The live recorder is bounded by default (`DEFAULT_MAX_OPEN_SPANS`, `DEFAULT_MAX_COMPLETED_SPANS`), and limits are configurable via `TracingRecorder::builder(...).max_open_spans(...)`, `.max_completed_spans(...)`, or `.limits(RecorderLimits { ... })`.

Use `#[tracing::instrument(fields(...))]` or `.instrument(...)` so span fields attach to async work correctly.
Do not hold a manual entered-span guard across `.await`; async spans may enter/exit many times, and this recorder finalizes completed work on `on_close` (drop), not enter/exit transitions.
Live recorder latency/wait precision uses monotonic elapsed duration (`duration_us`) captured at close time.

Tracing span capture for request/stage/queue evidence works outside Tokio runtimes. Runtime-pressure evidence still requires tailtriage's Tokio sampler or future runtime-metrics import; tracing-only spans cannot infer executor or blocking-pool pressure by themselves.



## Optional Tokio runtime sampler coupling

Enable feature `tokio` to couple tracing request/stage/queue intake with Tokio runtime-pressure sampling in one standard run.

- Tracing-only instrumentation works with or without Tokio and records request/stage/queue evidence.
- Tracing + Tokio sampler records runtime snapshots and sampler metadata for runtime-pressure evidence.
- Tracing spans alone do not infer executor or blocking-pool pressure.
- This crate is not an OpenTelemetry/OTLP implementation and not a tracing backend.

```rust
# #[cfg(feature = "tokio")]
# async fn demo() -> Result<(), Box<dyn std::error::Error>> {
use std::time::Duration;
use tracing_subscriber::prelude::*;
use tailtriage_tracing::TracingTokioSession;

let session = TracingTokioSession::builder("checkout-service")
    .sampler_interval(Duration::from_millis(50))
    .max_runtime_snapshots(256)
    .start()?;

let subscriber = tracing_subscriber::registry().with(session.layer());
tracing::subscriber::with_default(subscriber, || {
    let span = tracing::info_span!(
        "http.request",
        tt.kind = "request",
        tt.request_id = "req-1",
        tt.route = "/checkout"
    );
    let _entered = span.enter();
});

let run = session.shutdown().await?;
assert!(run.run().metadata.effective_tokio_sampler_config.is_some());
# Ok(())
# }
```

## Examples

- `examples/live_recorder.rs`: records one request span, one queue span, and one stage span with `TracingRecorder`, imports a run, and renders analyzer suspects and next checks.
- `examples/tracing_spans.jsonl`: normalized completed-span JSONL fixture importable via `import_jsonl_path` or CLI `tailtriage import tracing-json`.
