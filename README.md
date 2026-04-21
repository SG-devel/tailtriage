# tailtriage

`tailtriage` is a focused Rust toolkit for **Tokio tail-latency triage**.

When an async Rust service gets slow, `tailtriage` helps you answer a first practical question quickly:

> Is this slowdown mostly app-level queueing, executor pressure, blocking-pool pressure, or a slow downstream stage?

It produces a triage report with **evidence-ranked suspects** and **next checks**. Suspects are leads, not proof of root cause.

- Built for Tokio services and teams doing iterative triage.
- Useful with partial instrumentation.
- Not an observability backend.
- Not root-cause proof on its own.

## Quick start (crates.io)

For most users, start with the facade crate:

```bash
cargo add tailtriage
```

Optional facade integrations:

```bash
cargo add tailtriage --features tokio
cargo add tailtriage --features "tokio,axum"
```

Install the CLI separately for analysis/report generation:

```bash
cargo install tailtriage-cli
```

> Library crates capture data. `tailtriage-cli` analyzes artifacts.

## Why not just tokio-console or tokio-metrics?

Those tools are complementary building blocks. `tailtriage` fills a different gap: it turns request lifecycle timing plus optional runtime signals into a focused triage loop:

`capture -> analyze -> next check -> re-run`

In short:

- `tokio-console` helps you inspect live runtime/task behavior.
- `tokio-metrics` gives you runtime/task metrics signals.
- `tailtriage` helps you rank likely bottleneck families and choose the next targeted check from one captured run.

## What you get from the output

### Four bottleneck families

1. **Application queueing**: work waits before execution.
2. **Blocking-pool pressure**: `spawn_blocking` backlog inflates tails.
3. **Executor pressure**: scheduler contention delays runnable work.
4. **Downstream stage latency**: a dependency dominates request time.

### How to read results

- Treat `primary_suspect` as the best lead, not proof.
- Use `evidence[]` to choose one targeted experiment.
- Re-run and compare p95 shares plus suspect evidence.

## Primary entry points

From `tailtriage`:

- `tailtriage::Tailtriage` — direct capture lifecycle
- `tailtriage::controller::TailtriageController` — repeated arm/disarm bounded capture windows for long-lived services
- `tailtriage::tokio` _(optional feature)_ — runtime-pressure sampling
- `tailtriage::axum` _(optional feature)_ — Axum middleware/extractor ergonomics

## Which package should I use?

- **Default:** `tailtriage` + `tailtriage-cli`
- **Controller-heavy operations:** `tailtriage` (controller is included by default)
- **Fine-grained dependency control:** direct `tailtriage-core`, `tailtriage-controller`, `tailtriage-tokio`, or `tailtriage-axum`
- **Analysis only:** `tailtriage-cli`

## When to choose the controller

Use `tailtriage::controller::TailtriageController` when your service must stay up and you need repeated capture windows over time:

- arm
- collect
- disarm
- re-arm

This is a major capability of the facade crate, not a niche add-on.

## Minimal examples

### Facade capture (library)

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

### Controller capture window (library)

```rust,no_run
use tailtriage::controller::TailtriageController;

fn main() -> Result<(), Box<dyn std::error::Error>> {
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

### Analyze artifact (CLI)

```bash
tailtriage analyze tailtriage-run.json --format json
```

### Example output (JSON)

```json
{
  "request_count": 250,
  "p50_latency_us": 782227,
  "p95_latency_us": 1468239,
  "p99_latency_us": 1518551,
  "p95_queue_share_permille": 982,
  "p95_service_share_permille": 267,
  "inflight_trend": {
    "gauge": "queue_service_inflight",
    "sample_count": 500,
    "peak_count": 234,
    "p95_count": 225,
    "growth_delta": 0,
    "growth_per_sec_milli": 0
  },
  "warnings": [],
  "primary_suspect": {
    "kind": "application_queue_saturation",
    "score": 90,
    "confidence": "high",
    "evidence": ["Queue wait at p95 consumes 98.2% of request time.", "Observed queue depth sample up to 230."],
    "next_checks": [
      "Inspect queue admission limits and producer burst patterns.",
      "Compare queue wait distribution before and after increasing worker parallelism."
    ]
  },
  "secondary_suspects": [
    {
      "kind": "downstream_stage_dominates",
      "score": 55,
      "confidence": "low",
      "evidence": [
        "Stage 'simulated_work' has p95 latency 26566 us across 250 samples.",
        "Stage 'simulated_work' cumulative latency is 6546159 us.",
        "Stage 'simulated_work' contributes 33 permille of cumulative request latency."
      ],
      "next_checks": [
        "Inspect downstream dependency behind stage 'simulated_work'.",
        "Collect downstream service timings and retry behavior during tail windows.",
        "Review downstream SLO/error budget and align retry budget/backoff with it."
      ]
    }
  ]
}
```

## Development alternative (workspace checkout)

Use the GitHub/workspace path when you want to run packaged examples, inspect internals, or contribute:

```bash
cargo run -p tailtriage-tokio --example minimal_checkout
cargo run -p tailtriage-cli -- analyze tailtriage-run.json --format json
```

## Examples

Five public examples to start with:

- `minimal_checkout` — fastest capture-to-analyze loop
- `axum_minimal` — smallest Axum framework starter
- `axum_service_adoption` — service-shaped Axum adoption example
- `mini_service_integration` — helper-layer or fractured-code instrumentation shape
- `controller_minimal` — arm/disarm controller lifecycle starter

```bash
cargo run -p tailtriage-tokio --example minimal_checkout
cargo run -p tailtriage-axum --example axum_minimal
cargo run -p tailtriage-axum --example axum_service_adoption
cargo run -p tailtriage-tokio --example mini_service_integration
cargo run -p tailtriage-controller --example controller_minimal
python3 scripts/smoke_public_examples.py
```

## Demos

The demos are intentionally small services for Tokio tail-latency triage. They are designed to exercise diagnosis behavior with deterministic, reviewable artifacts, not universal causality proof.

If you only run three demos, start with:

```bash
python3 scripts/demo_tool.py validate queue
python3 scripts/demo_tool.py validate downstream
python3 scripts/demo_tool.py validate db-pool
```

Use before/after comparisons as a reproducible mitigation-confirmation loop, not causal proof.

Demo walkthrough and CI coverage details: [`docs/getting-started-demo.md`](docs/getting-started-demo.md)

## What this is not

`tailtriage` is not:

- an observability backend
- a distributed tracing system
- a general telemetry platform
- a root-cause proof engine

## Documentation map

- Facade/default crate docs: [`tailtriage/README.md`](tailtriage/README.md)
- Controller docs and config: [`tailtriage-controller/README.md`](tailtriage-controller/README.md)
- Runtime sampler docs: [`tailtriage-tokio/README.md`](tailtriage-tokio/README.md)
- User workflow guide: [`docs/user-guide.md`](docs/user-guide.md)
- Analyzer and diagnostics references:
  - [`tailtriage-cli/README.md`](tailtriage-cli/README.md)
  - [`docs/diagnostics.md`](docs/diagnostics.md)

- Advanced references:
  - [`docs/getting-started-demo.md`](docs/getting-started-demo.md)
  - [`docs/runtime-cost.md`](docs/runtime-cost.md)
  - [`docs/collector-limits.md`](docs/collector-limits.md)
