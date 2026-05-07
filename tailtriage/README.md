# tailtriage

`tailtriage` is the recommended default entry point for **Tokio tail-latency triage**.

It re-exports `tailtriage-core` at the crate root and exposes integration namespaces for controller workflows, Tokio runtime sampling, and Axum request boundaries. Only `controller` is enabled by default; `tokio` and `axum` are opt-in features.

## What problem this solves

When a Tokio service slows down, the first triage question is often:

> Is this slowdown mostly application queueing, executor pressure, blocking-pool pressure, or a slow downstream stage?

`tailtriage` helps you run the loop:

`capture -> analyze -> next check -> re-run`

The analysis result is triage guidance (evidence-ranked suspects plus next checks), not proof of root cause.

## Common use cases

| Symptom | tailtriage helps check |
| --- | --- |
| p95/p99 latency spikes | whether tail latency is dominated by queueing, executor pressure, blocking-pool pressure, or downstream stage latency |
| intermittent request timeouts | whether slow requests share a common bottleneck family in one captured run |
| low CPU but high latency | whether requests are waiting in queues, blocked behind constrained resources, or delayed by downstream work |
| requests appear stuck | whether time is spent before work starts, inside service execution, or in a named downstream stage |
| suspected blocking in async code | whether blocking-pool pressure is visible and should be investigated with a targeted follow-up |
| Tokio runtime seems overloaded | whether captured runtime-pressure signals point toward executor contention rather than app-level queueing |
| queue buildup before work starts | whether application queue wait dominates p95 latency |
| slow database or external API suspected | whether a downstream stage dominates request latency enough to be the next check |
| flaky latency in staging or production | which bottleneck family is the strongest lead from a bounded capture window |
| hard-to-reproduce tail spikes | whether a captured slow window contains enough evidence to choose the next experiment |
| unclear profiler results | whether queueing, runtime pressure, blocking-pool pressure, or downstream waiting explains the tail before pursuing CPU hot paths |
| service has partial instrumentation only | whether available request, queue, stage, runtime, or inflight signals are enough for a useful triage lead |

## Installation

For direct capture or repeated controller-managed capture windows:

```bash
cargo add tailtriage
```

Optional integrations:

```bash
cargo add tailtriage --features tokio
cargo add tailtriage --features "tokio,axum"
```

`tailtriage` captures request/runtime evidence. Install analyzer/report tooling based on how you work. 

For command-line analysis of saved Run artifact JSON:

```bash
cargo install tailtriage-cli
```

For in-process Rust analysis/report generation:

```bash
cargo add tailtriage-analyzer
```

Add `tailtriage-analyzer` when you want to analyze a completed Run inside Rust code.
- `tailtriage-cli` consumes Run artifact JSON from disk.
- `tailtriage-analyzer` produces typed `Report` values in process and renders **Report JSON** when you call analyzer renderers.

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

### 2) Analyze the captured run

In process (typed `Report` + optional text/JSON rendering), use `tailtriage-analyzer`.

From the command line for saved artifacts, use `tailtriage-cli`:

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

- `controller` _(default)_: enables `tailtriage::controller`
- `tokio` _(opt-in)_: enables `tailtriage::tokio`
- `axum` _(opt-in)_: enables `tailtriage::axum`
- `full`: enables `controller`, `tokio`, and `axum`

Docs.rs note: `tailtriage` docs are built with `all-features = true`, so docs.rs may render optional namespaces such as `tailtriage::tokio` and `tailtriage::axum`. In downstream crates, those namespaces are available only when their Cargo features are enabled.

## Important constraints

- Capture and analysis are separate. For in-process analysis/report generation, use `tailtriage-analyzer`.
- For command-line analysis of saved artifacts, use `tailtriage-cli`.
- `CaptureMode` selection does not auto-start Tokio runtime sampling.
- Analysis output is triage guidance, not root-cause proof.

## Related crates

- `tailtriage-core`: framework-agnostic instrumentation primitives and artifact model
- `tailtriage-controller`: repeated bounded capture windows
- `tailtriage-tokio`: Tokio runtime-pressure sampling
- `tailtriage-axum`: Axum request-boundary integration
- `tailtriage-analyzer`: in-process analysis/report generation for completed runs
- `tailtriage-cli`: command-line analysis of saved run artifacts
