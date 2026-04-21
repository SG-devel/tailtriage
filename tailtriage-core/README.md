# tailtriage-core

`tailtriage-core` is the **instrumentation foundation** for `tailtriage`.

Use this crate when you want to capture request lifecycle timing and emit one bounded JSON run artifact, without pulling framework or runtime-specific adapters.

## When to use this crate vs others

- Use `tailtriage-core` for direct, explicit request instrumentation.
- Add `tailtriage-tokio` if you also need Tokio runtime-pressure snapshots.
- Add `tailtriage-axum` if you want Axum middleware/extractor helpers.
- Use `tailtriage-controller` if capture must be armed/disarmed repeatedly in a long-lived process.
- Use `tailtriage-cli` to analyze artifacts.

## Installation

```bash
cargo add tailtriage-core
```

## Minimal example

```rust,no_run
use tailtriage_core::{RequestOptions, Tailtriage};

# async fn demo() -> Result<(), Box<dyn std::error::Error>> {
let tailtriage = Tailtriage::builder("checkout-service")
    .output("tailtriage-run.json")
    .build()?;

let started = tailtriage
    .begin_request_with("/checkout", RequestOptions::new().request_id("req-1").kind("http"));
let request = started.handle.clone();

request.queue("ingress").await_on(async {}).await;
request.stage("db").await_on(async { Ok::<(), std::io::Error>(()) }).await?;
started.completion.finish_ok();

tailtriage.shutdown()?;
# Ok(())
# }
```

## Runtime and lifecycle notes

- `CaptureMode` changes only retention defaults.
- `CaptureMode` does **not** auto-start Tokio runtime sampling.
- `queue(...)`, `stage(...)`, and `inflight(...)` never finish a request.
- Every request must be finished exactly once with `finish(...)`, `finish_ok()`, or `finish_result(...)`.
- `shutdown()` flushes the run artifact and does not fabricate missing completions.
- `strict_lifecycle(true)` makes `shutdown()` fail if unfinished requests remain.

## Capture-mode retention defaults

`Light`
- `max_requests = 100_000`
- `max_stages = 200_000`
- `max_queues = 200_000`
- `max_inflight_snapshots = 200_000`
- `max_runtime_snapshots = 100_000`

`Investigation`
- `max_requests = 300_000`
- `max_stages = 600_000`
- `max_queues = 600_000`
- `max_inflight_snapshots = 600_000`
- `max_runtime_snapshots = 300_000`
