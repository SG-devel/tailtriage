# tailtriage-tracing

`tailtriage-tracing` is a focused intake bridge surface for tracing-shaped completed spans that are assembled into standard `tailtriage_core::Run` artifacts through core-owned completed-run assembly.

This crate is intentionally narrow:

- It defines semantic convention keys (`tt.*`) for triage-oriented span fields.
- It defines typed intake records and import option/result types.
- It converts typed `SpanRecord` values with `run_from_span_records`.
- It imports JSONL from readers/paths when records contain completed span timing (`import_jsonl_reader` / `import_jsonl_path`).
- It provides an in-process `tracing_subscriber::Layer` recorder (`TracingRecorder`) for completed `tt.*` spans.
- It optionally couples live tracing intake with Tokio runtime snapshots via `tokio::TracingTokioSession`.
- It does **not** implement OpenTelemetry or OTLP.
- It does **not** change `tailtriage-analyzer`.

JSONL import, typed `SpanRecord` import, and live recorder intake all produce standard `tailtriage_core::Run` values for the same analyzer/report workflow via core-owned completed-run assembly. Completed tracing import output follows the same bounded retention/truncation semantics as core-built runs.

## JSONL import support in this phase

Public APIs:

- `import_jsonl_reader(reader, options)`
- `import_jsonl_path(path, options)`

### Canonical JSONL shape (stable authoring contract)

Recommended normalized line shape for tests and integrations:

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
      "tt.route": "/checkout",
      "tt.outcome": "ok"
    }
  }
}
```

Notes:

- Importer accepts `started_at_unix_ms`/`finished_at_unix_ms` and aliases `start_unix_ms`/`end_unix_ms`.
- `duration_us` is optional and must be an unsigned microseconds value. When present, it overrides derived `(finished-start)` duration for request latency, stage latency, and queue wait.
- Start/end unix-ms timestamps are still required even when `duration_us` is present.
- In this phase, normalized shape uses **literal dotted keys** inside `fields` (for example `"tt.kind"` and `"tt.request_id"`), not nested objects that require flattening.
- Importer reads `tt.*` fields from `fields`, `span.fields`, or top-level `tt.*` keys when present.
- Scalars can be strings, bools, numbers, or null.
- Empty lines are ignored.
- Malformed JSON line input is an import error in both strict and non-strict mode.
- In non-strict mode, syntactically valid but malformed/incomplete `tt.*` records are skipped with warnings.
- In strict mode, malformed/incomplete `tt.*` records are import errors.
- Tolerant import of close-event-like records is best-effort compatibility only; it is not the preferred/stable authoring format.

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

Use literal dotted `tt.*` keys inside the span `fields` object. The table below
describes the stable field contract used by import and live recording.

| Field | Required for span kind | Expected type | Default | Meaning |
| --- | --- | --- | --- | --- |
| `tt.kind` | request, stage, queue | string (`"request"`, `"stage"`, `"queue"`) | none | Identifies the triage span kind. |
| `tt.request_id` | request, stage, queue | string | none | Correlates request, stage, and queue spans into one request flow. |
| `tt.route` | request | string | none | Logical request route/name used for request-level grouping. |
| `tt.stage` | stage | string | none | Stage label for downstream-stage evidence. |
| `tt.queue` | queue | string | none | Queue label for queue-wait evidence. |
| `tt.outcome` | request (optional) | string | `ok` (with aggregate importer warning) | Optional completion outcome label (for example `ok`/`error`). |
| `tt.success` | stage (optional) | bool | `true` (with aggregate importer warning) | Optional normalized success flag. |
| `tt.depth_at_start` | queue (optional) | unsigned integer | omitted when unknown (no warning) | Queue depth snapshot when queued work started waiting. |


## Live tracing recorder

```rust
use tracing::Instrument;
use tracing_subscriber::prelude::*;
use tailtriage_tracing::TracingRecorder;

let recorder = TracingRecorder::builder("checkout-service")
    .service_version("1.2.3")
    .run_id("run-42")
    .strict(false)
    .build();

let subscriber = tracing_subscriber::registry().with(recorder.layer());
tracing::subscriber::with_default(subscriber, || {
    futures_executor::block_on(async {
        let request = tracing::info_span!(
            "http.request",
            tt.kind = "request",
            tt.request_id = "req-42",
            tt.route = "/checkout",
            tt.outcome = "ok"
        );
        async {}.instrument(request).await;
    });
});

let imported = recorder.snapshot_run()?;
let imported = recorder.shutdown()?;
let run = imported.run();
assert_eq!(run.requests.len(), 1);
# Ok::<(), tailtriage_tracing::ImportError>(())
```

## Live recorder tracking rule

For live recording, a span is tracked only when **at least one `tt.*` field is
declared at span creation time**.

- If a span has no `tt.*` field at creation, later `record(...)` calls are ignored
  by this recorder.
- `tt.kind` may be filled later only when a `tt.*` field was declared initially
  (for example with `tracing::field::Empty`).
- Record `tt.*` values as typed scalar fields (string/bool/number), not only
  debug-formatted values.

Minimal pattern:

```rust
let span = tracing::info_span!(
    "db.stage",
    tt.kind = tracing::field::Empty,
    tt.request_id = "req-42",
    tt.stage = "postgres.query"
);
span.record("tt.kind", "stage");
```

The live recorder is bounded by default (`DEFAULT_MAX_OPEN_SPANS`, `DEFAULT_MAX_COMPLETED_SPANS`), and limits are configurable via `TracingRecorder::builder(...).max_open_spans(...)`, `.max_completed_spans(...)`, or `.limits(RecorderLimits { ... })`. In non-strict mode, retention drops are reported as import warnings and `run.truncation.limits_hit = true`; in strict mode, retention drops fail import with a strict violation so dropped `tt.*` evidence is not silently accepted.

Use `#[tracing::instrument(fields(...))]` or `.instrument(...)` so span fields attach to async work correctly.
Do not hold a manual entered-span guard across `.await`; async spans may enter/exit many times, and this recorder finalizes completed work on `on_close` (drop), not enter/exit transitions.
Live recorder latency/wait precision uses monotonic elapsed duration (`duration_us`) captured at close time.

Tracing-only import/recording provides request, stage, and queue evidence. It
does not fabricate executor-pressure or blocking-pool-pressure evidence.
Runtime-pressure evidence still requires tailtriage runtime snapshots (for
example the Tokio sampler).



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
