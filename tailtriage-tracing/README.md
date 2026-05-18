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

## Canonical JSONL shape

Public APIs:

- `import_jsonl_reader(reader, options)`
- `import_jsonl_path(path, options)`

Use this normalized shape for stable integrations and fixtures:

```json
{
  "span": {
    "name": "http.request",
    "started_at_unix_ms": 1700000000000,
    "finished_at_unix_ms": 1700000000120,
    "duration_us": 120000,
    "fields": {
      "tt.kind": "request",
      "tt.request_id": "req-42",
      "tt.route": "/checkout"
    }
  }
}
```

Canonical contract notes:

- `duration_us` is optional and must be an unsigned integer microseconds value.
- When present, `duration_us` overrides derived `(finished-start)` duration for request latency, stage latency, and queue wait.
- `started_at_unix_ms` and `finished_at_unix_ms` are still required even when `duration_us` is present.
- In this phase, normalized shape uses **literal dotted keys** inside `fields` (for example `"tt.kind"` and `"tt.request_id"`), not nested objects that require flattening.
- Importer reads `tt.*` fields from `fields`, `span.fields`, or top-level `tt.*` keys when present.
- Scalars can be strings, bools, numbers, or null.
- Empty lines are ignored.
- Malformed JSON line input is an import error in both strict and non-strict mode.
- In non-strict mode, syntactically valid but malformed/incomplete `tt.*` records are skipped with warnings.
- In strict mode, malformed/incomplete `tt.*` records are import errors.
- Tolerant close-event-like import support is best-effort compatibility for some existing tracing JSONL sources, not the preferred/stable authoring format.

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

Close-event-like records require explicit unix-ms start/end timestamps; timing is not guessed from line receive time, and broad compatibility with arbitrary tracing JSON is not claimed.

## Field convention

`tailtriage-tracing` triage intake uses literal dotted `tt.*` keys for request,
stage, and queue evidence.

| Field | Required for span kind | Expected type | Default | Meaning |
| --- | --- | --- | --- | --- |
| `tt.kind` | request, stage, queue | string (`"request"`, `"stage"`, `"queue"`) | none | Classifies span semantics for triage import. |
| `tt.request_id` | request, stage, queue | string | none | Correlation key joining request + child stage/queue spans in one request timeline. |
| `tt.route` | request | string | empty route | Request route label used in request-level evidence grouping. |
| `tt.stage` | stage | string | none | Stage label used for stage-latency evidence and ranking. |
| `tt.queue` | queue | string | none | Queue label used for queue-wait evidence and ranking. |
| `tt.outcome` | request, stage, queue (optional on all) | string | `"ok"` | Outcome label (`"ok"` or error-like values) used in evidence context. |
| `tt.success` | request, stage, queue (optional on all) | boolean | derived from `tt.outcome` (`true` when `tt.outcome == "ok"`) | Explicit success flag override for success/failure context. |
| `tt.depth_at_start` | queue (optional) | unsigned integer | none | Queue depth at enqueue/start used as supporting queue pressure context. |

Treat fields as typed scalar values (string/bool/number/null), not only
debug-formatted strings.

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

## Live recorder tracking rule

A span must declare at least one `tt.*` field at span creation to be tracked by
the live recorder. If a span has no `tt.*` field at creation, later recordings
are ignored for intake.

`tt.kind` may be filled later only when a `tt.*` field was declared initially
(for example with `tracing::field::Empty`):

```rust
use tracing::field::Empty;

let span = tracing::info_span!(
    "db.query",
    tt.kind = Empty,
    tt.request_id = "req-42",
    tt.stage = "db"
);
span.record("tt.kind", "stage");
```

Record `tt.*` values as typed scalar fields (string/bool/number) rather than
only debug-formatted text.

The live recorder is bounded by default (`DEFAULT_MAX_OPEN_SPANS`, `DEFAULT_MAX_COMPLETED_SPANS`), and limits are configurable via `TracingRecorder::builder(...).max_open_spans(...)`, `.max_completed_spans(...)`, or `.limits(RecorderLimits { ... })`.

Use `#[tracing::instrument(fields(...))]` or `.instrument(...)` so span fields attach to async work correctly.
Do not hold a manual entered-span guard across `.await`; async spans may enter/exit many times, and this recorder finalizes completed work on `on_close` (drop), not enter/exit transitions.
Live recorder latency/wait precision uses monotonic elapsed duration (`duration_us`) captured at close time.

Tracing span capture for request/stage/queue evidence works outside Tokio
runtimes. Tracing-only imports provide request/stage/queue evidence but do not
fabricate runtime-pressure evidence. Runtime-pressure evidence still requires
tailtriage's Tokio sampler or future runtime-metrics import; tracing-only spans
cannot infer executor or blocking-pool pressure by themselves.



## Optional Tokio runtime sampler coupling

Enable the optional `tokio` feature when you want one standard run that combines:

- tracing request/stage/queue evidence, and
- Tokio runtime-pressure snapshots from `tailtriage-tokio::RuntimeSampler`.

This is optional. Base `TracingRecorder` usage stays available without Tokio.

```rust
# #[cfg(feature = "tokio")]
# {
use std::time::Duration;
use tailtriage_tracing::tokio::TracingTokioSession;
use tracing_subscriber::prelude::*;

# #[tokio::main(flavor = "current_thread")]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
let session = TracingTokioSession::builder("checkout-service")
    .service_version("1.2.3")
    .sampler_interval(Duration::from_millis(200))
    .max_runtime_snapshots(2_000)
    .start()?;

let subscriber = tracing_subscriber::registry().with(session.layer());
tracing::subscriber::with_default(subscriber, || {
    tracing::info_span!(
        "http.request",
        tt.kind = "request",
        tt.request_id = "req-42",
        tt.route = "/checkout"
    )
    .in_scope(|| {});
});

let imported = session.shutdown().await?;
assert!(!imported.run().runtime_snapshots.is_empty());
# Ok(())
# }
# }
```

This crate is still not an OTel/OTLP exporter and not a tracing backend.

## Examples

- `examples/live_recorder.rs`: records one request span, one queue span, and one stage span with `TracingRecorder`, imports a run, and renders analyzer suspects and next checks.
- `examples/tracing_spans.jsonl`: normalized completed-span JSONL fixture importable via `import_jsonl_path` or CLI `tailtriage import tracing-json`.
