# tailtriage-tokio

`tailtriage-tokio` adds **Tokio runtime-pressure evidence** to a `tailtriage` run artifact.

Use it when request lifecycle timing alone is not enough to separate likely runtime-related bottleneck families such as:

- executor pressure
- blocking-pool pressure
- queueing pressure
- slow downstream work that only looks like scheduler pressure at first glance

## What this crate does

This crate owns Tokio runtime sampler behavior:

- startup rules
- mode-specific sampler defaults
- runtime snapshot retention resolution
- recording runtime snapshots into the same run artifact as core request data

It does not change core request lifecycle semantics.

## When to choose this crate

Choose `tailtriage-tokio` when:

- you already use `tailtriage-core` and want runtime snapshots in the same artifact
- you want stronger evidence for runtime-related bottlenecks
- you want direct control over sampler cadence and runtime snapshot retention

Choose `tailtriage` instead when you want the default entry point and feature-gated access to this crate.

## Installation

Direct crates:

```bash
cargo add tailtriage-core tailtriage-tokio
```

Via the default crate:

```bash
cargo add tailtriage --features tokio
```

## Quick start

```rust,no_run
use std::sync::Arc;

use tailtriage_core::Tailtriage;
use tailtriage_tokio::RuntimeSampler;

async fn demo() -> Result<(), Box<dyn std::error::Error>> {
    let run = Arc::new(
        Tailtriage::builder("checkout-service")
            .output("tailtriage-run.json")
            .investigation()
            .build()?,
    );

    let sampler = RuntimeSampler::builder(Arc::clone(&run)).start()?;

    // ... run workload here ...

    sampler.shutdown().await;
    run.shutdown()?;
    Ok(())
}
```

## Important constraints

- `RuntimeSampler::start()` must run inside an active Tokio runtime
- only one successful sampler start is allowed per `Tailtriage` run
- `CaptureMode` does **not** auto-start runtime sampling
- runtime snapshot retention is bounded by the resolved core capture limits

## What gets added to the artifact

On successful sampler start, tailtriage records effective sampler configuration metadata into the run artifact metadata before runtime snapshots are captured.

When the sampler is running, the run artifact can include runtime snapshots such as:

- `alive_tasks`
- `global_queue_depth`
- `local_queue_depth`
- `blocking_queue_depth`
- `remote_schedule_count`

Some of these fields depend on Tokio build/runtime capabilities.

## Minimal configuration examples

### Start with inherited mode defaults

```rust,no_run
use std::sync::Arc;
use tailtriage_core::Tailtriage;
use tailtriage_tokio::RuntimeSampler;

# async fn demo() -> Result<(), Box<dyn std::error::Error>> {
let run = Arc::new(
    Tailtriage::builder("checkout-service")
        .light()
        .output("tailtriage-run.json")
        .build()?,
);

let sampler = RuntimeSampler::builder(Arc::clone(&run)).start()?;
sampler.shutdown().await;
run.shutdown()?;
# Ok(())
# }
```

### Override cadence and runtime snapshot retention

```rust,no_run
use std::sync::Arc;
use std::time::Duration;
use tailtriage_core::Tailtriage;
use tailtriage_tokio::RuntimeSampler;

# async fn demo() -> Result<(), Box<dyn std::error::Error>> {
let run = Arc::new(
    Tailtriage::builder("checkout-service")
        .investigation()
        .output("tailtriage-run.json")
        .build()?,
);

let sampler = RuntimeSampler::builder(Arc::clone(&run))
    .interval(Duration::from_millis(50))
    .max_runtime_snapshots(10_000)
    .start()?;

sampler.shutdown().await;
run.shutdown()?;
# Ok(())
# }
```

## Mode defaults

When you do not override sampler settings, this crate uses Tokio-owned defaults based on the resolved sampler mode.

### Light defaults

- cadence: `500ms`
- `max_runtime_snapshots = 5_000`

### Investigation defaults

- cadence: `100ms`
- `max_runtime_snapshots = 50_000`

These defaults apply only when the sampler is started.

## Resolution rules

`RuntimeSampler::builder(...)` resolves configuration in this order:

1. inherited mode from the core-selected `CaptureMode`
2. optional explicit mode override via `.mode(...)`
3. optional cadence override via `.interval(...)`
4. optional runtime snapshot retention override via `.max_runtime_snapshots(...)`

The resolved runtime snapshot retention is then **clamped** by the core run cap:

`effective_core_config.capture_limits.max_runtime_snapshots`

## Metrics availability notes

On stable Tokio, runtime snapshots always include:

- `alive_tasks`
- `global_queue_depth`

Additional fields such as:

- `local_queue_depth`
- `blocking_queue_depth`
- `remote_schedule_count`

depend on `tokio_unstable` support and may be `None`.

That means runtime evidence quality can vary by build and environment.

## What this crate does not do

This crate does not provide:

- request lifecycle instrumentation by itself
- repeated arm/disarm capture windows
- framework-boundary integration for Axum
- analysis or report generation

For those surfaces, use:

- `tailtriage-core`
- `tailtriage-controller`
- `tailtriage-axum`
- `tailtriage-cli`

## Related crates

- `tailtriage`: recommended default entry point
- `tailtriage-core`: core request instrumentation and artifact writing
- `tailtriage-controller`: repeated bounded windows
- `tailtriage-cli`: artifact analysis
