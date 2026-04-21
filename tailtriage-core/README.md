# tailtriage-core

`tailtriage-core` is the **foundation instrumentation crate** in the tailtriage workspace.

It captures request lifecycle timing and writes bounded JSON run artifacts that downstream analysis uses.

## What this crate is for

Use `tailtriage-core` when you want explicit, framework-agnostic instrumentation with minimal dependencies.

This crate owns core lifecycle semantics:

- request admission and request handle APIs
- queue/stage/inflight measurements
- completion semantics
- run artifact writing and shutdown behavior

## When to use this crate vs others

- **Use `tailtriage-core`:** direct instrumentation in any async Rust service.
- **Add `tailtriage-tokio`:** if you also need Tokio runtime-pressure snapshots.
- **Add `tailtriage-axum`:** if you want Axum middleware/extractor ergonomics.
- **Use `tailtriage-controller`:** if you need repeated arm/disarm capture windows.
- **Use `tailtriage` (default crate):** as the default starting point for most users.

## Installation

```bash
cargo add tailtriage-core
```

## Minimal examples

### Basic request lifecycle

```rust,no_run
use tailtriage_core::Tailtriage;

# fn demo() -> Result<(), Box<dyn std::error::Error>> {
let run = Tailtriage::builder("checkout-service")
    .output("tailtriage-run.json")
    .build()?;

let started = run.begin_request("/checkout");
started.completion.finish_ok();

run.shutdown()?;
# Ok(())
# }
```

### Explicit queue/stage instrumentation

```rust,no_run
use tailtriage_core::{RequestOptions, Tailtriage};

# async fn demo() -> Result<(), Box<dyn std::error::Error>> {
let run = Tailtriage::builder("checkout-service")
    .output("tailtriage-run.json")
    .build()?;

let started = run.begin_request_with(
    "/checkout",
    RequestOptions::new().request_id("req-1").kind("http"),
);
let req = started.handle.clone();

req.queue("ingress").await_on(async {}).await;
req.stage("db").await_on(async { Ok::<(), std::io::Error>(()) }).await?;
started.completion.finish_ok();

run.shutdown()?;
# Ok(())
# }
```

## Core lifecycle constraints

- `queue(...)`, `stage(...)`, and `inflight(...)` do not finish requests.
- Every admitted request must be finished exactly once.
- `shutdown()` flushes artifact data and does not fabricate missing completions.
- `strict_lifecycle(true)` makes `shutdown()` fail if unfinished requests remain.
- `CaptureMode` adjusts retention defaults only; it does not start runtime sampling.

## Deeper docs

- Default crate path: [`../tailtriage/README.md`](../tailtriage/README.md)
- Controller capture windows: [`../tailtriage-controller/README.md`](../tailtriage-controller/README.md)
- Tokio runtime sampling: [`../tailtriage-tokio/README.md`](../tailtriage-tokio/README.md)
- Analyzer/report generation: [`../tailtriage-cli/README.md`](../tailtriage-cli/README.md)
