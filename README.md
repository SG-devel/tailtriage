# tailtriage

`tailtriage` is a focused Rust toolkit for **Tokio tail-latency triage**.

## 1) Why this exists (one-screen overview)

When an async Rust service gets slow, `tailtriage` helps you answer a first practical question quickly:

> Is this slowdown mostly app-level queueing, executor pressure, blocking-pool pressure, or a slow downstream stage?

It produces a triage report with **evidence-ranked suspects** and **next checks**.

- Built for Tokio services and teams doing iterative triage.
- Useful with partial instrumentation.
- Not an observability backend.
- Not root-cause proof on its own.

## 2) Default install path (crates.io)

For most users, start with the facade crate:

```bash
cargo add tailtriage
```

Optional facade integrations:

```bash
cargo add tailtriage --features tokio
cargo add tailtriage --features "tokio,axum"
```

Install the analysis/reporting CLI separately:

```bash
cargo install tailtriage-cli
```

> `tailtriage` (library) handles capture/instrumentation. `tailtriage-cli` (binary) handles artifact analysis/report generation.

## 3) Entry points in the facade crate

The `tailtriage` crate is the official facade and default library entry point.

- **Direct capture:** `tailtriage::Tailtriage`
  - Build one capture run, instrument request lifecycle, write artifact.
- **Repeated bounded capture windows (default-enabled):** `tailtriage::controller::TailtriageController`
  - Arm/disarm generations for live services.
  - This is one of the highest-leverage reasons to choose the facade crate.
- **Optional runtime evidence:** `tailtriage::tokio` *(feature: `tokio`)*
  - Runtime sampler and Tokio-pressure signals.
- **Optional Axum ergonomics:** `tailtriage::axum` *(feature: `axum`)*
  - Middleware/extractor helpers.

## 4) Which path do I need?

- **Default recommendation:** use `tailtriage` + `tailtriage-cli`.
- **Controller-first operations workflow:** use the facade default (`controller` is enabled by default) and start with `tailtriage::controller::TailtriageController`.
- **Focused-crate advanced selection:** use `tailtriage-core`, `tailtriage-controller`, `tailtriage-tokio`, or `tailtriage-axum` directly when you need tighter dependency control.
- **CLI-only workflow:** install `tailtriage-cli` when you only need to read/analyze existing run artifacts.

## 5) Minimal examples

### Facade direct capture (library side)

```rust,no_run
use tailtriage::Tailtriage;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let tailtriage = Tailtriage::builder("checkout-service")
        .output("tailtriage-run.json")
        .build()?;

    let started = tailtriage.begin_request("/checkout");
    started.completion.finish_ok();

    tailtriage.shutdown()?;
    Ok(())
}
```

### Controller bounded window (library side)

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

### Analyze a captured artifact (CLI side)

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
    "evidence": [
      "Queue wait at p95 consumes 98.2% of request time.",
      "Observed queue depth sample up to 230."
    ],
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

## 6) GitHub/workspace path (development alternative)

Use the repository workspace when you want to:

- run bundled examples and demos,
- inspect internals,
- contribute changes.

Typical dev commands from a checkout:

```bash
cargo run -p tailtriage-tokio --example minimal_checkout
cargo run -p tailtriage-cli -- analyze tailtriage-run.json --format json
```

## 7) Docs map (details live here)

This root README is intentionally short and adoption-oriented. For deeper semantics:

- User workflow and guidance: [`docs/user-guide.md`](docs/user-guide.md)
- Diagnostics details: [`docs/diagnostics.md`](docs/diagnostics.md)
- Architecture: [`docs/architecture.md`](docs/architecture.md)
- Runtime cost notes: [`docs/runtime-cost.md`](docs/runtime-cost.md)
- Collector limits and stress behavior: [`docs/collector-limits.md`](docs/collector-limits.md)
- Demo walkthrough: [`docs/getting-started-demo.md`](docs/getting-started-demo.md)
- Crate-specific docs:
  - [`tailtriage/README.md`](tailtriage/README.md)
  - [`tailtriage-core/README.md`](tailtriage-core/README.md)
  - [`tailtriage-controller/README.md`](tailtriage-controller/README.md)
  - [`tailtriage-tokio/README.md`](tailtriage-tokio/README.md)
  - [`tailtriage-axum/README.md`](tailtriage-axum/README.md)
  - [`tailtriage-cli/README.md`](tailtriage-cli/README.md)
