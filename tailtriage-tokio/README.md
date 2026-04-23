# tailtriage-tokio

`tailtriage-tokio` adds **Tokio runtime-pressure evidence** to a `tailtriage` run artifact.

Use it when request lifecycle timing alone is not enough to separate likely bottleneck families (executor pressure, blocking-pool pressure, queueing pressure, or slow downstream work that can look like scheduler pressure).

## What this crate does

This crate owns Tokio runtime sampler behavior:

- startup rules
- mode-specific sampler defaults
- runtime snapshot retention resolution
- recording runtime snapshots into the same run artifact as core request data

It does not change core request lifecycle semantics.

## Crate selection

Choose `tailtriage-tokio` when you already use `tailtriage-core` and want runtime snapshots in the same artifact, with direct control over sampler cadence and runtime snapshot retention.

Choose `tailtriage` when you want the default entry point with feature-gated access.

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

- `RuntimeSampler::start()` requires an active Tokio runtime.
- Only one successful sampler start is allowed per `Tailtriage` run.
- `CaptureMode` does **not** auto-start runtime sampling.
- Runtime snapshot retention is clamped by the resolved core capture limits.

## What gets added to the artifact

On successful sampler start, effective sampler configuration metadata is recorded before runtime snapshots are captured.

When running, artifacts can include runtime snapshots such as:

- `alive_tasks`
- `global_queue_depth`
- `local_queue_depth`
- `blocking_queue_depth`
- `remote_schedule_count`

(Some fields depend on Tokio build/runtime capabilities.)

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

When sampler settings are not overridden, defaults follow the resolved sampler mode:

- Light: `cadence = 500ms`, `max_runtime_snapshots = 5_000`
- Investigation: `cadence = 100ms`, `max_runtime_snapshots = 50_000`

These defaults apply only when the sampler is started.

## Resolution rules

`RuntimeSampler::builder(...)` resolves configuration in this order:

1. inherited mode from core-selected `CaptureMode`
2. optional explicit mode override via `.mode(...)`
3. optional cadence override via `.interval(...)`
4. optional runtime snapshot retention override via `.max_runtime_snapshots(...)`

Resolved runtime snapshot retention is then clamped by:

`effective_core_config.capture_limits.max_runtime_snapshots`
