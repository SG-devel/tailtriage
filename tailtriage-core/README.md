# tailtriage-core

`tailtriage-core` is the framework-agnostic instrumentation foundation for `tailtriage`.

Use it when you want explicit control over request lifecycle instrumentation and bounded JSON run artifacts without bringing in controller, Axum, or Tokio runtime-sampler APIs unless you choose them separately.

## What this crate does

`tailtriage-core` owns the capture-side model and lifecycle semantics:

- request admission
- queue/stage/inflight instrumentation
- explicit request completion
- bounded in-memory retention
- JSON run artifact writing

The artifact produced here is what downstream tools such as `tailtriage-cli` analyze.

## When to choose this crate

Choose `tailtriage-core` when:

- you want framework-agnostic instrumentation
- you want the smallest capture-side surface
- you want to add optional crates explicitly instead of starting with the default crate

Choose `tailtriage` instead when you want the recommended all-in-one entry point.

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

## What the capture lifecycle looks like

A request starts with `begin_request(...)` or `begin_request_with(...)`.

That returns a `StartedRequest` with two parts:

- `started.handle` for queue/stage/inflight instrumentation
- `started.completion` for explicit request completion

Example:

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

## What goes into the artifact

A run artifact can contain:

- request events
- queue events
- stage events
- in-flight snapshots
- runtime snapshots
- truncation counters and lifecycle warnings
- effective configuration metadata for the run

Runtime snapshots are part of the core artifact schema, but they are only populated when a runtime integration such as `tailtriage-tokio` records them.

## Lifecycle contract

These semantics are important:

- `queue(...)`, `stage(...)`, and `inflight(...)` do **not** finish requests
- every admitted request must be finished exactly once
- dropping a completion token does **not** auto-finish the request
- `shutdown()` writes the artifact and does **not** fabricate missing completions
- `strict_lifecycle(true)` makes `shutdown()` fail if unfinished requests remain

## Capture modes

`tailtriage-core` supports two capture modes:

- `CaptureMode::Light`
- `CaptureMode::Investigation`

These modes change **retention defaults only**. They do not change request lifecycle semantics, and they do **not** auto-start runtime sampling.

### Default limits by mode

`Light` defaults:

- `max_requests = 100_000`
- `max_stages = 200_000`
- `max_queues = 200_000`
- `max_inflight_snapshots = 200_000`
- `max_runtime_snapshots = 100_000`

`Investigation` defaults:

- `max_requests = 300_000`
- `max_stages = 600_000`
- `max_queues = 600_000`
- `max_inflight_snapshots = 600_000`
- `max_runtime_snapshots = 300_000`

You can override these limits with either:

- `capture_limits(...)` for a full override
- `capture_limits_override(...)` for field-level overrides on top of mode defaults

## Minimal configuration examples

### Light mode

```rust,no_run
use tailtriage_core::Tailtriage;

let run = Tailtriage::builder("checkout-service")
    .light()
    .output("tailtriage-run.json")
    .build()?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Investigation mode with one field override

```rust,no_run
use tailtriage_core::{CaptureLimitsOverride, Tailtriage};

let run = Tailtriage::builder("checkout-service")
    .investigation()
    .capture_limits_override(CaptureLimitsOverride {
        max_requests: Some(500_000),
        ..CaptureLimitsOverride::default()
    })
    .output("tailtriage-run.json")
    .build()?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

## What this crate does not do by itself

`tailtriage-core` does not provide:

- controller-style repeated arm/disarm windows
- Tokio runtime sampling
- Axum middleware or extractors
- analysis or report generation

Use sibling crates when you need those surfaces:

- `tailtriage-controller`
- `tailtriage-tokio`
- `tailtriage-axum`
- `tailtriage-cli`

## Related crates

- `tailtriage`: recommended all-in-one entry point
- `tailtriage-controller`: repeated bounded windows
- `tailtriage-tokio`: runtime-pressure sampling
- `tailtriage-axum`: Axum request-boundary integration
- `tailtriage-cli`: analysis and report generation
