# tailtriage-tracing

`tailtriage-tracing` is a narrow tracing intake bridge for completed `tt.*` spans.

It helps existing `tracing` users produce standard `tailtriage_core::Run` artifacts by:
- writing Run JSON on shutdown, and/or
- streaming stable completed-span JSONL as spans close.

It is **not**:
- an observability backend,
- generic tracing log scraping,
- an OTel/OTLP pipeline,
- proof of root cause (output remains triage leads).

## Recommended live session setup

```rust,no_run
use tailtriage_tracing::TracingIntakeSession;
use tracing_subscriber::prelude::*;

# fn main() -> Result<(), Box<dyn std::error::Error>> {
let session = TracingIntakeSession::builder("checkout-service")
    .run_json_path("target/tailtriage-examples/checkout.run.json")
    .completed_span_jsonl_path("target/tailtriage-examples/checkout.spans.jsonl")
    .build()?;

let subscriber = tracing_subscriber::registry().with(session.layer());
tracing::subscriber::with_default(subscriber, || {
    let request = tracing::info_span!(
        "request",
        tt.kind = "request",
        tt.request_id = "req-1",
        tt.route = "/checkout",
        tt.outcome = "ok"
    );
    let _entered = request.enter();
});

session.shutdown()?;
# Ok(())
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
- `max_completed_spans` bounds in-memory completed-span retention.
- Completed-span JSONL streaming happens before in-memory completed-span retention is applied.
- Warnings and lifecycle warnings indicate evidence may be incomplete when limits are hit or writer issues occur.

## Runtime-pressure limitation

Tracing-only intake does not fabricate runtime snapshots. Runtime-pressure evidence still requires runtime snapshots/Tokio sampler coupling.

## Examples

- `tailtriage-tracing/examples/live_session_to_run.rs`
- `tailtriage-tracing/examples/completed_span_jsonl_import.rs`
