# tailtriage-tracing

`tailtriage-tracing` is a narrow tracing intake bridge for completed `tt.*` spans.

It helps existing `tracing` users produce standard `tailtriage_core::Run` artifacts by:
- writing Run JSON on shutdown, and/or
- streaming stable completed-span JSONL as spans close.

It is **not**:
- an observability backend,
- generic tracing-log JSON scraping,
- an OTel/OTLP pipeline,
- proof of root cause (output remains triage leads).

## Feature flags

- Default (`jsonl`): typed records plus JSONL import APIs.
- `live`: enables `TracingRecorder`, `TailtriageLayer`, and `TracingIntakeSession`.
- `tokio`: enables `TracingTokioSession` (and includes `live`).

CLI offline import workflows only need JSONL import support and do not require the live `tracing_subscriber` layer dependency.

## Recommended live session setup (`live` feature)

```rust,no_run
use tailtriage_tracing::TracingIntakeSession;
use tracing::Instrument as _;
use tracing_subscriber::prelude::*;

# async fn work() {}
# async fn run() -> Result<(), Box<dyn std::error::Error>> {
let session = TracingIntakeSession::builder("checkout-service")
    .run_json_path("target/tailtriage-examples/checkout.run.json")
    .completed_span_jsonl_path("target/tailtriage-examples/checkout.spans.jsonl")
    .build()?;

let subscriber = tracing_subscriber::registry().with(session.layer());
let _guard = tracing::subscriber::set_default(subscriber);
let request = tracing::info_span!(
    "request",
    tt.kind = "request",
    tt.request_id = "req-1",
    tt.route = "/checkout",
    tt.outcome = "ok"
);
work().instrument(request).await;

session.shutdown()?;
# Ok(())
# }
# fn main() -> Result<(), Box<dyn std::error::Error>> {
#   let _ = run();
#   Ok(())
# }
```

## Direct Run JSON path

Use `run_json_path(...)` when you want to skip a separate import step:

```bash
tailtriage analyze target/tailtriage-examples/checkout.run.json
```

## Completed-span JSONL path

Use `completed_span_jsonl_path(...)` when you want an offline import workflow:

```bash
tailtriage import tracing-json target/tailtriage-examples/checkout.spans.jsonl \
  --input-format tailtriage-span-jsonl \
  --service checkout-service \
  --output target/tailtriage-examples/checkout.run.json

tailtriage analyze target/tailtriage-examples/checkout.run.json
```

## Stable JSONL wrapper format

Stable completed-span JSONL records use this wrapper:

```json
{"format":"tailtriage.tracing-span.v1","span":{...}}
```

`format` is a wrapper-level field (not a `SpanRecord` field).

Arbitrary `tracing_subscriber::fmt().json()` log JSON is rejected by import. Import does not guess span timing from line receive time: provide explicit unix-ms start/end timestamps on completed spans.

## `tt.*` field convention

| Span kind | Required fields | Optional fields |
| --- | --- | --- |
| request | `tt.kind="request"`, `tt.request_id`, `tt.route` | `tt.outcome` |
| stage | `tt.kind="stage"`, `tt.request_id`, `tt.stage` | `tt.success` |
| queue | `tt.kind="queue"`, `tt.request_id`, `tt.queue` | `tt.depth_at_start` |

## Strict vs non-strict

- Strict mode: malformed/incomplete `tt.*` span records fail import/session conversion.
- Non-strict mode: malformed/incomplete records are warned and skipped where implemented.

## Retention and drop behavior

- `max_open_spans` bounds in-flight span tracking.
- Completed-span JSONL streaming happens before in-memory capture-limit retention is applied.
- Warnings and lifecycle warnings indicate evidence may be incomplete when limits are hit or writer issues occur.

## Runtime-pressure limitation

Tracing intake import and native capture share the same CaptureMode/CaptureLimits semantics for request/stage/queue evidence retention. Offline tracing JSONL import does not fabricate runtime snapshots. Runtime-pressure evidence still requires runtime snapshots/Tokio sampler coupling.
Persisted Run JSON artifacts intended for `tailtriage analyze` require at least one completed request span event. Library snapshots taken before completed requests may still be zero-request for inspection.

For `TracingTokioSession`, runtime snapshot retention also uses the same core capture-limit model:

- configure retention with `mode(...)`, `capture_limits(...)`, or `capture_limits_override(...)`
- there is no tracing-specific `.max_runtime_snapshots(...)` session builder method
- tracing-only runs still do not fabricate runtime snapshots

## Examples

- `tailtriage-tracing/examples/live_session_to_run.rs`
- `tailtriage-tracing/examples/completed_span_jsonl_import.rs`
