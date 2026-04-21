# tailtriage-tokio

`tailtriage-tokio` adds **Tokio runtime-pressure evidence** to a `tailtriage-core` run artifact.

This crate owns runtime sampler behavior and Tokio-specific constraints.

## What this crate is for

Use this crate when request lifecycle instrumentation alone is not enough to separate:

- application queueing
- executor pressure
- blocking-pool pressure
- downstream stage slowdown

## When to use this crate vs others

- **Use `tailtriage-tokio`:** runtime-pressure sampling in the same run artifact.
- **Use `tailtriage-core` only:** if request timing is sufficient for your triage pass.
- **Use `tailtriage-axum`:** for framework ergonomics (independent from runtime sampling).
- **Use `tailtriage` facade:** for default onboarding with feature-gated access to this module.

## Installation

Direct crates:

```bash
cargo add tailtriage-core tailtriage-tokio
```

Via facade:

```bash
cargo add tailtriage --features tokio
```

## Minimal example

```rust,no_run
use std::sync::Arc;

use tailtriage_core::Tailtriage;
use tailtriage_tokio::RuntimeSampler;

# async fn demo() -> Result<(), Box<dyn std::error::Error>> {
let run = Arc::new(
    Tailtriage::builder("checkout-service")
        .output("tailtriage-run.json")
        .investigation()
        .build()?,
);

let sampler = RuntimeSampler::builder(Arc::clone(&run)).start()?;

// ... workload ...

sampler.shutdown().await;
run.shutdown()?;
# Ok(())
# }
```

## Runtime-specific constraints

- `RuntimeSampler::start()` must run inside an active Tokio runtime.
- A single `Tailtriage` run allows only one successful runtime sampler start.
- `CaptureMode` does not auto-start runtime sampling.
- Runtime snapshot retention is bounded by core capture limits.

## Metrics availability notes

- Stable Tokio metrics include `alive_tasks` and `global_queue_depth`.
- Additional metrics such as `local_queue_depth`, `blocking_queue_depth`, and `remote_schedule_count` depend on `tokio_unstable` support.

## Deeper docs

- Facade/default integration path: [`../tailtriage/README.md`](../tailtriage/README.md)
- Core lifecycle semantics: [`../tailtriage-core/README.md`](../tailtriage-core/README.md)
- User workflow and interpretation: [`../docs/user-guide.md`](../docs/user-guide.md)
