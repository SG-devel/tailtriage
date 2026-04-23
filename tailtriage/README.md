# tailtriage

`tailtriage` is the recommended default entry point for **Tokio tail-latency triage**.

It re-exports `tailtriage-core` at the crate root and exposes integration namespaces for controller workflows, Tokio runtime sampling, and Axum request boundaries. Only `controller` is enabled by default; `tokio` and `axum` are opt-in features.

## What problem this solves

When a Tokio service slows down, the first triage question is often:

> Is this slowdown mostly application queueing, executor pressure, blocking-pool pressure, or a slow downstream stage?

`tailtriage` helps you run `capture -> analyze -> next check -> re-run`.

Analysis output is triage guidance (evidence-ranked suspects plus next checks), not root-cause proof.

## Installation

```bash
cargo add tailtriage
```

Optional integrations:

```bash
cargo add tailtriage --features tokio
cargo add tailtriage --features "tokio,axum"
```

Install the analyzer CLI separately:

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

Start with `tailtriage` for the default integration path and feature-gated siblings.

Choose a focused crate only when you need a narrower boundary:

- `tailtriage-core`: framework-agnostic instrumentation primitives
- `tailtriage-controller`: repeated bounded windows
- `tailtriage-tokio`: runtime-pressure sampling
- `tailtriage-axum`: Axum request-boundary wiring

## Feature flags

- `controller` _(default)_: enables `tailtriage::controller`
- `tokio` _(opt-in)_: enables `tailtriage::tokio`
- `axum` _(opt-in)_: enables `tailtriage::axum`
- `full`: enables `controller`, `tokio`, and `axum`

## Important constraints

- Capture and analysis are separate: this crate writes artifacts, `tailtriage-cli` analyzes them.
- `CaptureMode` selection does not auto-start Tokio runtime sampling.
- Analysis output is triage guidance, not root-cause proof.

## Related crates

- `tailtriage-core`: <https://docs.rs/tailtriage-core>
- `tailtriage-controller`: <https://docs.rs/tailtriage-controller>
- `tailtriage-tokio`: <https://docs.rs/tailtriage-tokio>
- `tailtriage-axum`: <https://docs.rs/tailtriage-axum>
- `tailtriage-cli`: <https://docs.rs/tailtriage-cli>
