# tailtriage-tracing

`tailtriage-tracing` is a focused intake bridge for teams that already instrument with Rust `tracing` spans.

It converts tracing-shaped request/stage/queue evidence into standard `tailtriage_core::Run` values for the same analyzer/report workflow used by native `tailtriage` capture.

This crate is intentionally narrow:

- semantic `tt.*` conventions for triage-oriented span fields
- `SpanRecord -> Run` conversion via `run_from_span_records`
- JSONL import via `import_jsonl_reader` and `import_jsonl_path`
- live in-memory recording with `TracingRecorder`
- no OpenTelemetry/OTLP implementation
- no tracing backend behavior
- no analyzer semantic changes

## JSONL import support

Public APIs:

- `import_jsonl_reader(reader, options)`
- `import_jsonl_path(path, options)`

Normalized completed-span JSONL contract:

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

- Use literal dotted `tt.*` keys in `fields`.
- Import reads `tt.*` fields from `fields`, `span.fields`, or top-level `tt.*` keys when present.
- Empty lines are ignored.
- Malformed JSON lines are import errors.

## tracing-subscriber JSON caveat

`tracing-subscriber` JSON output varies by formatter settings. This importer supports:

- normalized completed-span JSONL in the documented shape
- close-event-like records only when explicit unix-ms start/end timestamps are present

It does not guess timing from line receive time.

## Intended field shape

Typical span fields:

- request: `tt.kind`, `tt.request_id`, `tt.route`
- stage: `tt.kind`, `tt.request_id`, `tt.stage`
- queue: `tt.kind`, `tt.request_id`, `tt.queue`, `tt.depth_at_start`

## Live tracing recorder

```rust
use tailtriage_analyzer::{analyze_run, AnalyzeOptions};
use tailtriage_tracing::TracingRecorder;
use tracing_subscriber::prelude::*;

let recorder = TracingRecorder::builder("checkout-service").build();
let subscriber = tracing_subscriber::registry().with(recorder.layer());

tracing::subscriber::with_default(subscriber, || {
    let request = tracing::info_span!(
        "http.request",
        tt.kind = "request",
        tt.request_id = "req-1",
        tt.route = "/checkout"
    );
    let _entered = request.enter();
});

let imported = recorder.shutdown()?;
let report = analyze_run(imported.run(), AnalyzeOptions::default());
# let _ = report;
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Examples

- `examples/live_recorder.rs`
- `examples/tracing_spans.jsonl`

## Async span guidance

Use `#[tracing::instrument(fields(...))]` or `.instrument(...)` so fields attach to async work correctly.

Do not hold a manual entered-span guard across `.await`; async spans can enter/exit repeatedly, while this recorder finalizes completed spans on close.

## Runtime-pressure limitation

Tracing-only imports are useful for request/stage/queue triage evidence, but they usually do not include Tokio runtime snapshots.

Without runtime snapshots, executor-pressure and blocking-pool suspects may be weaker or absent. For runtime-pressure evidence, use tailtriage Tokio runtime sampling.
