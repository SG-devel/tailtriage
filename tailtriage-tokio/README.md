# tailtriage-tokio

`tailtriage-tokio` adds **Tokio runtime-pressure evidence** to a `tailtriage-core` run.

Use this crate when core request instrumentation alone is not enough to separate:

- application queueing,
- executor pressure,
- blocking-pool pressure, and
- downstream stage slowdown.

## When to use this crate vs others

- Use `tailtriage-core` for request lifecycle instrumentation.
- Add `tailtriage-tokio` to periodically capture runtime metrics into the same artifact.
- Use `tailtriage-axum` only for Axum ergonomics (independent of runtime sampling).

## Installation

```bash
cargo add tailtriage-core tailtriage-tokio
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

## Runtime requirements and feature notes

- `RuntimeSampler::start` must run inside an active Tokio runtime.
- Each `Tailtriage` run allows only one successful sampler start.
- `CaptureMode` does not auto-start sampling.
- Stable Tokio metrics: `alive_tasks`, `global_queue_depth`.
- `tokio_unstable` metrics: `local_queue_depth`, `blocking_queue_depth`, `remote_schedule_count`.

## Configuration precedence

`RuntimeSampler::builder(...)` resolves settings in this order:

1. inherited mode from `Tailtriage`
2. explicit `.mode(...)` override
3. explicit `.interval(...)` override
4. explicit `.max_runtime_snapshots(...)` override

Resolved runtime snapshot retention is clamped by core capture limits.
