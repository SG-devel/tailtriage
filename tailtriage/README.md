# tailtriage

`tailtriage` is the default crate for **Tokio tail-latency triage**.

It gives you one capture-side entry point with optional integrations for controller workflows, Tokio runtime sampling, and Axum request boundaries.

## What problem this solves

When a Tokio service slows down, the first triage question is often:

> Is this slowdown mostly application queueing, executor pressure, blocking-pool pressure, or a slow downstream stage?

`tailtriage` helps you run the loop:

`capture -> analyze -> next check -> re-run`

The analysis result is triage guidance (evidence-ranked suspects plus next checks), not proof of root cause.

## Installation

```bash
cargo add tailtriage
```

Optional integrations:

```bash
cargo add tailtriage --features tokio
cargo add tailtriage --features "tokio,axum"
```

Analyzer:

```bash
cargo install tailtriage-cli
```

## Quick start

### 1) Capture one run

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

### 2) Analyze the artifact

```bash
tailtriage analyze tailtriage-run.json
```

## Crate selection

Start with `tailtriage` when you want the recommended entry point and optional integrations behind feature flags.

Choose a focused crate only when you need a narrower boundary:

- `tailtriage-core`: framework-agnostic instrumentation primitives
- `tailtriage-controller`: repeated bounded windows
- `tailtriage-tokio`: runtime-pressure sampling
- `tailtriage-axum`: Axum request-boundary wiring

## Feature flags

- `controller` *(default)*: enables `tailtriage::controller`
- `tokio`: enables `tailtriage::tokio`
- `axum`: enables `tailtriage::axum`
- `full`: enables `controller`, `tokio`, and `axum`

## Important constraints

- Capture and analysis are separate: this crate writes artifacts, `tailtriage-cli` analyzes them.
- `CaptureMode` selection does not auto-start Tokio runtime sampling.
- Analysis output is triage guidance, not root-cause proof.

## Related crates

- `tailtriage-core`: framework-agnostic instrumentation primitives and artifact model
- `tailtriage-controller`: repeated bounded capture windows
- `tailtriage-tokio`: Tokio runtime-pressure sampling
- `tailtriage-axum`: Axum request-boundary integration
- `tailtriage-cli`: artifact analysis and report generation
