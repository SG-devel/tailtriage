# tailtriage-tracing

`tailtriage-tracing` is a narrow adoption path for teams that already use Rust `tracing` spans.

It converts tracing-shaped request/stage/queue evidence into standard `tailtriage_core::Run` values for the same analyzer/report workflow used by native capture. It is not a tracing backend, not an observability platform, and does not implement OpenTelemetry or OTLP.

## JSONL import support

Public import APIs:

- `import_jsonl_reader(reader, options)`
- `import_jsonl_path(path, options)`
- `run_from_span_records(records, options)` for typed `SpanRecord -> Run` conversion

Offline workflow:

```bash
tailtriage import tracing-json spans.jsonl --service checkout --output tailtriage-run.json
tailtriage analyze tailtriage-run.json
```

The import command writes pretty Run JSON. Analysis is a separate step.

## tracing-subscriber JSON caveat

`tracing-subscriber` JSON output varies by formatter configuration. This crate supports the documented completed-span JSONL shape and close-event-like records only when explicit unix-ms start/end timestamps are present.

It does not guess timing from line receive time and does not claim compatibility with arbitrary tracing JSON variants.

## Intended field shape

Use normalized completed-span JSONL with literal dotted `tt.*` keys:

```json
{
  "span": {
    "name": "http.request",
    "id": "span-1",
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

Supported timing keys include `started_at_unix_ms` / `finished_at_unix_ms` and aliases `start_unix_ms` / `end_unix_ms`.

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

## Async-span guidance

Use `#[tracing::instrument(fields(...))]` or `.instrument(...)` so span fields attach correctly across async work. Avoid holding a manual entered-span guard across `.await`; completed spans finalize on close (`on_close`), not on enter/exit transitions.

## Runtime-pressure limitation

Tracing-shaped imports can provide request/stage/queue evidence, but runtime-pressure evidence remains Tokio-specific. Tracing-only runs usually have no runtime snapshots, so executor-pressure and blocking-pool suspects can be weaker or absent.

## Examples

- `examples/live_recorder.rs`
- `examples/tracing_spans.jsonl`
