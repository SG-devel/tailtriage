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

### Canonical JSONL shape

Use this normalized completed-span shape as the stable authoring format for tests and integrations:

```json
{"span":{"name":"http.request","started_at_unix_ms":1700000000000,"finished_at_unix_ms":1700000000120,"duration_us":120000,"fields":{"tt.kind":"request","tt.request_id":"req-42","tt.route":"/checkout"}}}
```

Rules:

- `started_at_unix_ms` and `finished_at_unix_ms` are required unix-millisecond timestamps.
- `duration_us` is optional and must be an unsigned integer microseconds value.
- When present, `duration_us` overrides timestamp-derived duration for request latency, stage latency, and queue wait.
- Start/end timestamps are still required even when `duration_us` is present.
- Use literal dotted keys (for example `"tt.kind"`) inside `fields`.

Importer behavior in this phase:

- Importer accepts `started_at_unix_ms`/`finished_at_unix_ms` and aliases `start_unix_ms`/`end_unix_ms`.
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

### tracing-subscriber JSON caveat

Direct `tracing-subscriber` JSON output can vary by formatter configuration. In this phase, tolerant close-event import is best-effort compatibility only, not the preferred/stable authoring format.

For stable ingestion contracts, author canonical normalized completed-span JSONL.

### Field convention

These `tt.*` fields are the stable semantic contract for tracing intake:

| Field | Required for span kind | Expected type | Default | Meaning |
| --- | --- | --- | --- | --- |
| `tt.kind` | request, stage, queue | string (`"request"`, `"stage"`, `"queue"`) | none | Span classification used to map timing/evidence into request, stage, and queue triage records. |
| `tt.request_id` | request, stage, queue | string | none | Correlation key that groups request span plus related stage/queue spans. |
| `tt.route` | request | string | none | Request route or operation name for request-level grouping. |
| `tt.stage` | stage | string | none | Logical downstream stage name (for example `"db"`, `"cache"`, `"http"`). |
| `tt.queue` | queue | string | none | Logical queue name attributed to queue wait evidence. |
| `tt.outcome` | request (optional), stage (optional), queue (optional) | string | none | Outcome label recorded by application code (for example `"ok"`, `"error"`, `"timeout"`). |
| `tt.success` | request (optional), stage (optional), queue (optional) | bool | none | Optional success/failure scalar used for coarse outcome slicing. |
| `tt.depth_at_start` | queue (optional) | unsigned integer (or numeric value convertible to `u64`) | none | Queue depth sampled at queue-span start for queue-pressure context. |

Fields outside this list are ignored by tracing intake unless/ until explicitly documented.

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



### Live recorder tracking rule

A span must declare at least one `tt.*` field at span creation to be tracked by `TracingRecorder`.

- `tt.kind` may be filled later only if some `tt.*` field was declared at creation (for example with `tracing::field::Empty`).
- Record `tt.*` values as typed scalar fields (string/bool/number), not only debug-formatted values.

Minimal example:

```rust
let span = tracing::info_span!(
    "db.stage",
    tt.kind = tracing::field::Empty,
    tt.request_id = "req-42",
    tt.stage = "db"
);
span.record("tt.kind", "stage");
```

Tracing-only imports/recordings provide request/stage/queue evidence and do not fabricate runtime-pressure evidence. Runtime-pressure evidence still requires runtime snapshots (for example via the Tokio sampler).

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
