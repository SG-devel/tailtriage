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

For in-process analysis/report generation, use `tailtriage-analyzer`.
For command-line analysis of saved artifacts, use `tailtriage-cli`.

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

For `Arc<Tailtriage>` flows that move request handles across spawned tasks or helper layers, use `begin_request_owned(...)` / `begin_request_with_owned(...)`. Owned handles keep the same lifecycle rule: instrumentation does not finish the request, and the completion token must be finished exactly once.

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

## Output sinks

`tailtriage-core` captures run data and finalizes through a sink. It does not perform analysis/report generation.

- `LocalJsonSink` (or builder `.output(...)`) writes Run artifact JSON to disk.
- `MemorySink` stores finalized typed `Run` values in memory.
- `DiscardSink` finalizes lifecycle and drops the finalized `Run` without persisting output.

`MemorySink` stores only the last finalized `Run`; each new finalized run replaces the previous stored value.

Use `MemorySink` when you want in-process analysis. `DiscardSink` drops finalized runs; use `MemorySink` instead when the finalized `Run` should be analyzed in process.

```rust,no_run
use tailtriage_core::{MemorySink, Tailtriage};

# fn example() -> Result<(), Box<dyn std::error::Error>> {
let sink = MemorySink::new();
let run = Tailtriage::builder("checkout-service")
    .sink(sink.clone())
    .build()?;

let started = run.begin_request("/checkout");
started.completion.finish_ok();
run.shutdown()?;

let finalized = sink.last_run();
# let _ = finalized;
# Ok(())
# }
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
- When `finalized_at_unix_ms` is present, prefer that field as the finalization signal; `finished_at_unix_ms` remains for backward compatibility.

## Timing model

- Duration fields in microseconds (`latency_us`, `wait_us`, stage `latency_us`) are authoritative for elapsed-time analysis.
- Unix millisecond timestamps are wall-clock anchors for log correlation, artifact readability, and coarse temporal grouping.
- Wall-clock timestamps can be coarse and can move if the system clock changes.
- Analyzer scoring uses duration fields for latency, queue wait, and stage duration.
- Temporal segmentation currently uses Unix-ms wall-clock timestamp anchors, so runtime/in-flight attribution can be approximate.

## Capture modes

Modes change retention defaults only. They do not change lifecycle semantics and do **not** auto-start runtime sampling.

- `CaptureMode::Light`
- `CaptureMode::Investigation`

Override limits with:

- `capture_limits(...)` (full override)
- `capture_limits_override(...)` (field-level override)

## Advanced: assembling completed run artifacts

Most users should use `Tailtriage::builder(...)` for live request instrumentation.
Use `RunBuilder` only when you already have completed request, stage, queue, in-flight, or runtime evidence and need to assemble a standard `Run` artifact.

`RunBuilder` is intended for import/conversion paths. It does not perform live lifecycle tracking. `RunBuilder::new` validates top-level run timestamp ordering. Each `push_*` call validates required event/snapshot shape and timestamp ordering before retention is applied. Completed request latency, stage latency, and queue wait fields are authoritative evidence; `RunBuilder` does not synthesize, repair, or reject those durations based on wall-clock timestamp deltas. It applies the same bounded retention/truncation semantics as live core capture, so events beyond configured `CaptureLimits` are dropped without error and `Run.truncation` counters are updated. `RunBuilder` does not validate cross-event correlation (for example a stage without a matching request) and does not synthesize lifecycle completions. For assembled/imported artifacts, host and pid default to `None`, and generated run IDs use the same core run-id semantics as live capture.

```rust,no_run
use tailtriage_core::{RequestEvent, RunBuilder, RunBuilderOptions};

fn assemble_run() -> Result<(), Box<dyn std::error::Error>> {
    let mut builder = RunBuilder::new(RunBuilderOptions::new("checkout-service"))?;
    builder.push_request(RequestEvent {
        request_id: "req-1".into(),
        route: "/test".into(),
        kind: Some("http".into()),
        started_at_unix_ms: 1,
        started_at_run_us: None,
        finished_at_unix_ms: 2,
        finished_at_run_us: None,
        latency_us: 1_000,
        outcome: "ok".into(),
    })?;

    let run = builder.finish();
    assert_eq!(run.requests.len(), 1);
    Ok(())
}
```

## What this crate does not do

This crate does not provide:

- repeated arm/disarm controller windows
- Tokio runtime sampling
- Axum middleware/extractors
- analysis/report generation

Use sibling crates for those surfaces: `tailtriage-controller`, `tailtriage-tokio`, `tailtriage-axum`, `tailtriage-analyzer`, and `tailtriage-cli`.
