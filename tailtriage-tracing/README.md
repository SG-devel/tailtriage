# tailtriage-tracing

`tailtriage-tracing` is a narrow tracing intake surface for teams that already use Rust `tracing` spans.

It converts tracing-shaped request/stage/queue evidence into standard `tailtriage_core::Run` values that feed the same analyzer/report workflow as native capture.

It is not a tracing backend, not an observability platform, and does not implement OpenTelemetry or OTLP.

## JSONL import support

Public import APIs:

- `import_jsonl_reader(reader, options)`
- `import_jsonl_path(path, options)`

Offline CLI path:

```bash
tailtriage import tracing-json spans.jsonl --service checkout --output tailtriage-run.json
tailtriage analyze tailtriage-run.json
```

The import command writes pretty Run JSON (not Report JSON). Warnings are emitted to stderr as `warning: ...`.

## tracing-subscriber JSON caveat

`tracing-subscriber` JSON output formats vary by formatter options. This crate supports:

- normalized completed-span JSONL (documented below)
- close-event-like rows only when explicit unix-ms start/end timestamps are present

It does not guess timing from line receive time, and it does not claim arbitrary tracing JSON compatibility.

## Intended field shape

Use literal dotted `tt.*` keys in the span `fields` object:

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

- normalized shape uses literal dotted keys (`"tt.kind"`), not nested key flattening
- aliases `start_unix_ms`/`end_unix_ms` are accepted for start/end timing
- empty lines are ignored
- malformed JSON lines are import errors

## Live tracing recorder

Use `TracingRecorder` for in-memory recording of completed `tt.*` spans:

```rust
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
let run = imported.run();
# let _ = run;
# Ok::<(), tailtriage_tracing::ImportError>(())
```

## Async-span guidance

Prefer `#[tracing::instrument(fields(...))]` or `.instrument(...)` for async work so fields remain attached correctly.

Do not hold a manual entered-span guard across `.await`; async spans can enter/exit repeatedly, and this recorder finalizes completed spans on close (`on_close`), not enter/exit transitions.

## Runtime-pressure limitation

Tracing-only runs can still provide request/stage/queue evidence.

Runtime-pressure evidence remains Tokio-specific. Without runtime snapshots from tailtriage Tokio sampling, executor-pressure and blocking-pool suspects are usually weaker or absent.

## Examples

- `examples/live_recorder.rs`
- `examples/tracing_spans.jsonl`
