# tailtriage

`tailtriage` is the default crate for **Tokio tail-latency triage**.

Use it when you want one crate that can cover the common capture-side workflow:

- direct request instrumentation
- repeated arm/disarm capture windows for long-lived services
- optional Tokio runtime-pressure sampling
- optional Axum request-boundary integration

For most users, this is the right crate to start with.

## What problem this solves

When a Tokio service gets slow, the first useful question is usually not “what is the root cause?” but:

> Is this slowdown mostly application queueing, executor pressure, blocking-pool pressure, or a slow downstream stage?

`tailtriage` helps you capture one bounded run, analyze it, and choose the next targeted check.

The intended loop is:

`capture -> analyze -> next check -> re-run`

The analysis result is a triage aid. It ranks likely bottleneck families and gives follow-up checks. It does **not** prove root cause on its own.

## What you get from this crate

With the default crate you can use:

- `tailtriage::Tailtriage` for a direct capture lifecycle
- `tailtriage::controller::TailtriageController` for repeated bounded windows in long-lived services
- `tailtriage::tokio` for optional runtime-pressure sampling
- `tailtriage::axum` for optional Axum ergonomics

## Installation

Start with the default crate:

```bash
cargo add tailtriage
```

Enable optional integrations as needed:

```bash
cargo add tailtriage --features tokio
cargo add tailtriage --features "tokio,axum"
```

Install the analyzer separately:

```bash
cargo install tailtriage-cli
```

## Quick start

### 1. Capture one run

```rust,no_run
use tailtriage::Tailtriage;

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

### 2. Analyze the artifact

```bash
tailtriage analyze tailtriage-run.json --format json
```

### 3. Read the result in this order

1. `primary_suspect.kind`
2. `primary_suspect.evidence[]`
3. `primary_suspect.next_checks[]`

Then run one targeted check, change one thing, and re-run under comparable load.

## When to use this crate

Use `tailtriage` when:

- you want the recommended entry point
- you want one dependency that can grow with your integration
- you may need controller, Tokio, or Axum support later
- you do not need the narrowest possible dependency surface

Choose a focused crate directly only when you want a smaller or more explicit integration boundary:

- `tailtriage-core` for framework-agnostic instrumentation primitives
- `tailtriage-controller` for repeated bounded windows
- `tailtriage-tokio` for runtime sampling
- `tailtriage-axum` for Axum request-boundary wiring

## Feature flags

- `controller` *(default)*: enables `tailtriage::controller`
- `tokio`: enables `tailtriage::tokio`
- `axum`: enables `tailtriage::axum`
- `full`: enables `controller`, `tokio`, and `axum`

## Minimal examples

### Direct capture

```rust,no_run
use tailtriage::Tailtriage;

fn demo() -> Result<(), Box<dyn std::error::Error>> {
    let run = Tailtriage::builder("checkout-service")
        .output("tailtriage-run.json")
        .build()?;

    let started = run.begin_request("/checkout");
    started.completion.finish_ok();

    run.shutdown()?;
    Ok(())
}
```

### Controller window for a long-lived service

```rust,no_run
use tailtriage::controller::TailtriageController;

fn demo() -> Result<(), Box<dyn std::error::Error>> {
    let controller = TailtriageController::builder("checkout-service")
        .initially_enabled(false)
        .output("tailtriage-run.json")
        .build()?;

    let _generation = controller.enable()?;
    let started = controller.begin_request("/checkout");
    started.completion.finish_ok();
    let _ = controller.disable()?;

    Ok(())
}
```

## Important constraints

- Capture and analysis are separate steps. This crate writes artifacts; `tailtriage-cli` analyzes them.
- Runtime sampling is optional. Selecting a capture mode does **not** start the Tokio runtime sampler.
- Axum support is optional. It handles request-boundary wiring, not diagnosis by itself.
- Analysis output is triage guidance, not proof of root cause.

## What this crate is not

`tailtriage` is not:

- an observability backend
- a distributed tracing system
- a general telemetry platform
- a root-cause proof engine

## Related crates

- `tailtriage-core`: framework-agnostic instrumentation primitives and artifact model
- `tailtriage-controller`: repeated bounded capture windows for long-lived services
- `tailtriage-tokio`: Tokio runtime-pressure sampling
- `tailtriage-axum`: Axum request-boundary integration
- `tailtriage-cli`: artifact analysis and report generation
