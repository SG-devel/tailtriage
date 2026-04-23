# tailtriage-core

`tailtriage-core` is the framework-agnostic capture foundation for `tailtriage`.

Use it when you want explicit request lifecycle instrumentation and bounded JSON artifacts without controller, Axum, or Tokio runtime-sampler APIs unless you add them separately.

## What this crate does

`tailtriage-core` owns capture-side lifecycle semantics:

- request admission
- queue/stage/inflight instrumentation
- explicit request completion
- bounded in-memory retention
- JSON run artifact writing

The artifact produced here is analyzed by `tailtriage-cli`.

## Crate selection

Use `tailtriage-core` when you want the smallest framework-agnostic capture surface.

Use `tailtriage` when you want the recommended default entry point: an aggregator/re-export crate with optional integrations behind features.

## Installation

```bash
cargo add tailtriage-core
```

## Quick start

```rust,no_run
use tailtriage_core::Tailtriage;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let run = Tailtriage::builder("checkout-service")
        .output("tailtriage-run.json")
        .build()?;

    let started = run.begin_request("/checkout");
    started.completion.finish_ok();

    run.shutdown()?;
    Ok(())
}
```

## Request lifecycle

`begin_request(...)` / `begin_request_with(...)` returns `StartedRequest` with:

- `started.handle` for queue/stage/inflight instrumentation
- `started.completion` for explicit finish

```rust,no_run
use tailtriage_core::{RequestOptions, Tailtriage};

async fn demo() -> Result<(), Box<dyn std::error::Error>> {
    let run = Tailtriage::builder("checkout-service")
        .output("tailtriage-run.json")
        .build()?;

    let started = run.begin_request_with(
        "/checkout",
        RequestOptions::new().request_id("req-1").kind("http"),
    );
    let req = started.handle.clone();

    req.queue("ingress").await_on(async {}).await;
    req.stage("db")
        .await_on(async { Ok::<(), std::io::Error>(()) })
        .await?;

    started.completion.finish_ok();
    run.shutdown()?;
    Ok(())
}
```

### Two easy-to-miss helpers

For infallible async work, `StageTimer::await_value(...)` avoids a dummy `Result`:

```rust,no_run
# use tailtriage_core::Tailtriage;
# async fn demo(run: Tailtriage) {
# let req = run.begin_request("/x").handle;
let value = req.stage("cache").await_value(async { 42 }).await;
# let _ = value;
# }
```

When queue depth is known at enqueue time, `QueueTimer::with_depth_at_start(...)` records it directly:

```rust,no_run
# use tailtriage_core::Tailtriage;
# async fn demo(run: Tailtriage) {
# let req = run.begin_request("/x").handle;
req.queue("ingress")
    .with_depth_at_start(12)
    .await_on(async {})
    .await;
# }
```

## Lifecycle contract

- `queue(...)`, `stage(...)`, and `inflight(...)` do **not** finish requests.
- Every admitted request must be finished exactly once.
- Dropping a completion token does **not** auto-finish.
- Non-strict lifecycle: `shutdown()` writes the artifact and records unfinished-request warnings/metadata.
- `strict_lifecycle(true)`: unfinished requests cause `shutdown()` to return an error and no artifact is written.

Finalization timestamps:

- Active `snapshot()` output is not finalized (`metadata.finalized_at_unix_ms == None`).
- `shutdown()` writes final artifacts with both:
  - `metadata.finished_at_unix_ms` set to shutdown time
  - `metadata.finalized_at_unix_ms` set to that same timestamp
- Older artifacts may deserialize with `metadata.finalized_at_unix_ms == None`.

## Capture modes

Modes change retention defaults only. They do not change lifecycle semantics and do **not** auto-start runtime sampling.

- `CaptureMode::Light`
- `CaptureMode::Investigation`

Override limits with:

- `capture_limits(...)` (full override)
- `capture_limits_override(...)` (field-level override)

## What this crate does not do

This crate does not provide:

- repeated arm/disarm controller windows
- Tokio runtime sampling
- Axum middleware/extractors
- analysis/report generation

Use sibling crates for those surfaces: `tailtriage-controller`, `tailtriage-tokio`, `tailtriage-axum`, and `tailtriage-cli`.
