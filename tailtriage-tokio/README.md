# tailtriage-tokio

`tailtriage-tokio` adds Tokio-specific instrumentation for `tailtriage`:

- **runtime-pressure sampling** that records Tokio runtime snapshots into a run artifact
- **primitive helpers** that map common Tokio APIs to queue, stage, and in-flight evidence

Use it when request lifecycle timing alone is not enough to separate likely runtime-related bottleneck families, or when you want lower-friction instrumentation around common Tokio primitives such as semaphores, bounded channels, async mutexes, rwlocks, spawned tasks, timeouts, and blocking work.

## What this crate provides

This crate provides two Tokio-specific layers.

### Runtime sampler

The runtime sampler owns Tokio runtime sampler behavior:

- startup rules
- mode-specific sampler defaults
- runtime snapshot retention resolution
- recording runtime snapshots into the same run artifact as core request data

Runtime snapshots help strengthen evidence for bottleneck families such as:

- executor pressure
- blocking-pool pressure
- queueing pressure
- slow downstream work that only looks like scheduler pressure at first glance

### Tokio primitive helpers

The primitive helpers map common Tokio APIs to explicit queue, stage, and in-flight signals while preserving Tokio return/error types.

They are useful when you want to instrument resource waits and async boundaries without manually timing each section.

Examples:

- semaphore permit waits as queue evidence
- bounded `mpsc` send backpressure as queue evidence
- async mutex/rwlock contention as queue evidence
- `JoinHandle`, timeout, and blocking work wrappers as stage evidence
- active bounded sections as in-flight evidence

This crate does not change core request lifecycle semantics.
Runtime-pressure evidence still depends on runtime snapshots captured by the sampler; tracing spans alone do not provide Tokio executor/blocking queue-depth fields.

## When to choose this crate

Choose `tailtriage-tokio` when:

- you already use `tailtriage-core` and want runtime snapshots in the same artifact
- you want stronger evidence for runtime-related bottlenecks
- you want direct control over sampler cadence and runtime snapshot retention
- you want Tokio primitive helpers that map queue/stage/in-flight instrumentation to common Tokio APIs

Choose `tailtriage` instead when you want the default entry point, where `tailtriage::tokio` is available with default features.

## Installation

Direct crates:

```bash
cargo add tailtriage-core tailtriage-tokio
```

Via the default crate:

```bash
cargo add tailtriage
```

## Quick start: runtime sampler

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

## Quick start: primitive helpers

Primary import path for default `tailtriage` users:

```ignore
use tailtriage::tokio::TokioRequestHandleExt;
```

Direct-crate alternative:

```rust
use tailtriage_tokio::TokioRequestHandleExt;
```

```rust,no_run
use std::sync::Arc;
use std::time::Duration;

use tailtriage_core::Tailtriage;
use tailtriage_tokio::TokioRequestHandleExt;

async fn demo() -> Result<(), Box<dyn std::error::Error>> {
    let run = Tailtriage::builder("checkout-service")
        .output("tailtriage-run.json")
        .build()?;

    let started = run.begin_request("/checkout");
    let req = started.handle.clone();

    let db_pool = Arc::new(tokio::sync::Semaphore::new(32));
    {
        let _permit = req.semaphore("db_pool_wait", &db_pool).acquire().await?;

        let _: Result<Result<(), ()>, tokio::time::error::Elapsed> = req
            .timeout_stage("downstream_http", Duration::from_millis(200), async {
                Ok::<(), ()>(())
            })
            .await;
    }

    let (tx, _rx) = tokio::sync::mpsc::channel(8);
    let _ = req.mpsc_send("worker_backpressure", &tx, "event").await;

    started.completion.finish_ok();
    run.shutdown()?;
    Ok(())
}
```

## Important constraints

- `RuntimeSampler::start()` must run inside an active Tokio runtime
- only one successful sampler start is allowed per `Tailtriage` run
- `CaptureMode` does **not** auto-start runtime sampling
- runtime snapshot retention is bounded by the resolved core capture limits
- queue/stage helper events are completion-based: dropping/canceling a pending helper future records no queue/stage event

## Runtime sampler details

### What gets added to the artifact

On successful sampler start, tailtriage records effective sampler configuration metadata into the run artifact metadata before runtime snapshots are captured.

When the sampler is running, the run artifact can include runtime snapshots such as:

- `alive_tasks`
- `global_queue_depth`
- `local_queue_depth`
- `blocking_queue_depth`
- `remote_schedule_count`

Some of these fields depend on Tokio build/runtime capabilities.

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

### Mode defaults

When you do not override sampler settings, this crate uses Tokio-owned defaults based on the resolved sampler mode.

#### Light defaults

- cadence: `500ms`
- `max_runtime_snapshots = 5_000`

#### Investigation defaults

- cadence: `100ms`
- `max_runtime_snapshots = 50_000`

These defaults apply only when the sampler is started.

### Resolution rules

`RuntimeSampler::builder(...)` resolves configuration in this order:

1. inherited mode from the core-selected `CaptureMode`
2. optional explicit mode override via `.mode(...)`
3. optional cadence override via `.interval(...)`
4. optional runtime snapshot retention override via `.max_runtime_snapshots(...)`

The resolved runtime snapshot retention is then **clamped** by the core run cap:

```text
effective_core_config.capture_limits.max_runtime_snapshots
```

### Metrics availability notes

On stable Tokio, the runtime sampler always attempts to populate:

- `alive_tasks`
- `global_queue_depth`

The artifact schema keeps these fields optional for compatibility and unavailable-data cases.

Additional fields such as:

- `local_queue_depth`
- `blocking_queue_depth`
- `remote_schedule_count`

depend on `tokio_unstable` support and may be `None`.

That means runtime evidence quality can vary by build and environment.

## Tokio primitive helper trait

Helpers map common Tokio primitives to explicit queue/stage/in-flight signals while preserving Tokio return/error types.

| Use case                     | Helper                                   | Records   |
| ---------------------------- | ---------------------------------------- | --------- |
| DB pool / capacity wait      | `semaphore(...).acquire()`               | queue     |
| owned permit wait            | `owned_semaphore(...).acquire_owned()`   | queue     |
| bounded channel backpressure | `mpsc_send(...)`                         | queue     |
| async mutex contention       | `mutex_lock(...)`                        | queue     |
| async rwlock contention      | `rwlock_read(...)` / `rwlock_write(...)` | queue     |
| spawned task result          | `join_task(...)`                         | stage     |
| timeout-wrapped work         | `timeout_stage(...)`                     | stage     |
| blocking pool work           | `blocking_stage(...)`                    | stage     |
| active bounded section       | `inflight_guard(...)`                    | in-flight |

### Semantics notes

- Queue/stage helper events are completion-based: dropping/canceling a pending helper future records no queue/stage event.
- The helper API intentionally does not include a generic mpsc receive wait helper. Receiver-side recv wait cannot distinguish idle workers from queued work residence time. For worker intake, start request/work-item capture after receiving the item unless you have explicit enqueue timestamps.
- `join_task(...)` records await time for the supplied `JoinHandle`, not necessarily the full task runtime.
- `join_task(...)`, `timeout_stage(...)`, and `blocking_stage(...)` preserve nested `Result`s; recorded stage success/failure comes from the outer Tokio wrapper result, so `Ok(Err(_))` is preserved and records as successful.
- `blocking_stage(...)` is lazy: it submits `spawn_blocking` only when awaited. Use `tokio::task::spawn_blocking` plus `join_task(...)` when you need eager overlap.
- If you need blocking work to start immediately or overlap with other work, call `tokio::task::spawn_blocking(...)` directly and instrument the returned `JoinHandle` with `join_task(...)`.
- `timeout_stage(...)` is lazy: timeout budget starts when the returned future is polled/awaited, not at helper construction.

### Extended helper example

```rust,no_run
use std::sync::Arc;
use std::time::Duration;

use tailtriage_core::Tailtriage;
use tailtriage_tokio::TokioRequestHandleExt;

# async fn demo() -> Result<(), Box<dyn std::error::Error>> {
let run = Tailtriage::builder("checkout-service").output("tailtriage-run.json").build()?;
let started = run.begin_request("/checkout");
let req = started.handle.clone();

let db_pool = Arc::new(tokio::sync::Semaphore::new(32));
{
    let _permit = req.semaphore("db_pool_wait", &db_pool).acquire().await?;
    let _: Result<Result<(), ()>, tokio::time::error::Elapsed> = req
        .timeout_stage("downstream_http", Duration::from_millis(200), async {
            Ok::<(), ()>(())
        })
        .await;
}

let (tx, _rx) = tokio::sync::mpsc::channel(8);
let _ = req.mpsc_send("worker_backpressure", &tx, "event").await;

started.completion.finish_ok();
run.shutdown()?;
# Ok(())
# }
```

## What this crate does not do

This crate does not provide:

- core request lifecycle instrumentation by itself
- repeated arm/disarm capture windows
- framework-boundary integration for Axum
- analysis or report generation

For those surfaces, use:

- `tailtriage-core`
- `tailtriage-controller`
- `tailtriage-axum`
- `tailtriage-analyzer` for in-process analysis/report generation
- `tailtriage-cli` for command-line analysis of saved artifacts

## Related crates

- `tailtriage`: recommended default entry point
- `tailtriage-core`: core request instrumentation and artifact writing
- `tailtriage-controller`: repeated bounded windows
- `tailtriage-axum`: Axum middleware/extractor integration
- `tailtriage-analyzer`: in-process analysis/report generation for completed runs
- `tailtriage-cli`: command-line analysis of saved run artifacts
- `tailtriage-tracing`: optional tracing intake bridge; combine with runtime sampling when executor/blocking-pressure separation is required
