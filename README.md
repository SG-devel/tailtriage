# tailscope

`tailscope` is a Rust toolkit for diagnosing **tail latency**, **queueing**, and **backpressure** issues in Tokio services.

It is the diagnosis layer above raw timings and runtime metrics. The goal is to answer:

> Is this service slow because of application-level queueing, executor pressure, blocking-pool pressure, or a slow downstream stage?

## MVP status

This repository is an MVP release candidate with three workspace crates:

- `tailscope-core`: run schema + instrumentation primitives + JSON sink
- `tailscope-tokio`: Tokio runtime sampling + `#[instrument_request]` macro re-export
- `tailscope-cli`: run analyzer (`tailscope analyze <run.json>`)

## What tailscope is (and is not)

### tailscope is

- easy to integrate in one service process
- useful with partial instrumentation
- explicit about evidence and uncertainty
- based on reproducible JSON run artifacts

### tailscope is not

- a tracing backend
- a metrics backend
- a distributed tracing platform
- a GUI observability product
- a claim of root-cause certainty

## Quick start

### 1) Collect a run artifact

```rust
use std::sync::Arc;
use std::time::Duration;

use tailscope_core::{Config, RequestMeta, Tailscope};
use tailscope_tokio::RuntimeSampler;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut config = Config::new("invoice-api");
    config.output_path = "tailscope-run.json".into();

    let tailscope = Arc::new(Tailscope::init(config)?);
    let sampler = RuntimeSampler::start(Arc::clone(&tailscope), Duration::from_millis(200))?;

    let request_id = "req-42".to_string();
    let meta = RequestMeta::new(request_id.clone(), "/invoice");

    tailscope
        .request(meta, "ok", async {
            let _inflight = tailscope.inflight("invoice_inflight");

            tailscope
                .queue(request_id.clone(), "invoice_worker")
                .await_on(tokio::time::sleep(Duration::from_millis(2)))
                .await;

            tailscope
                .stage(request_id, "persist_invoice")
                .await_on(tokio::time::sleep(Duration::from_millis(4)))
                .await;
        })
        .await;

    sampler.shutdown().await;
    tailscope.flush()?;
    Ok(())
}
```

### 2) Analyze the run

```bash
cargo run --manifest-path tailscope-cli/Cargo.toml -- analyze tailscope-run.json
cargo run --manifest-path tailscope-cli/Cargo.toml -- analyze tailscope-run.json --format json
```

### 3) Macro-based request entry point

```rust
use std::sync::Arc;
use tailscope_core::Tailscope;
use tailscope_tokio::instrument_request;

#[instrument_request(
    route = "/invoice",
    kind = "create_invoice",
    tailscope = tailscope,
    request_id = request_id.clone(),
    skip(tailscope)
)]
async fn create_invoice(
    tailscope: Arc<Tailscope>,
    request_id: String,
) -> Result<(), &'static str> {
    let _inflight = tailscope.inflight("invoice_inflight");
    Ok(())
}
```

```bash
tailscope analyze tailscope-run.json
```

## Diagnosis categories (MVP)

The analyzer ranks suspects from run evidence:

- `ApplicationQueueSaturation`
- `BlockingPoolPressure`
- `ExecutorPressureSuspected`
- `DownstreamStageDominates`
- `InsufficientEvidence`

For each suspect, the report includes:

- score + confidence
- supporting evidence
- recommended next checks

## Demos

### Queue/backpressure demo

```bash
scripts/run_queue_demo.sh
scripts/validate_queue_demo.sh
```

Artifacts:

- `demos/queue_service/artifacts/before-run.json`
- `demos/queue_service/artifacts/before-analysis.json`
- `demos/queue_service/artifacts/after-run.json`
- `demos/queue_service/artifacts/after-analysis.json`
- `demos/queue_service/artifacts/before-after-comparison.json`

Fixture snapshots:

- `demos/queue_service/fixtures/before-analysis.json`
- `demos/queue_service/fixtures/after-analysis.json`

Observed signal in the checked-in queue demo fixtures:

- p95 latency drops from ~1,682,454us (before) to ~24,745us (after)
- primary suspect score drops from 90 to 60
- p95 queue share drops from 981 permille to 5 permille

### Blocking-pool pressure demo

```bash
scripts/run_blocking_demo.sh
scripts/validate_blocking_demo.sh
```

Artifacts:

- `demos/blocking_service/artifacts/blocking-run.json`
- `demos/blocking_service/artifacts/blocking-analysis.json`

## Runtime cost measurement

Use the reproducible harness:

```bash
scripts/measure_runtime_cost.sh
```

See `docs/runtime-cost.md` for the latest sample output and interpretation notes.

## Known limitations (MVP)

- Tokio-only (no non-Tokio runtime support).
- Single-process run analysis (no multi-service correlation).
- Diagnosis is rule-based and evidence-ranked, not a proof engine.
- Runtime metrics such as local queue depth / blocking queue depth may be `None` without `tokio_unstable`.
- Stage and queue attribution quality depends on explicit `stage(...).await_on(...)` and `queue(...).await_on(...)` coverage.
- No OpenTelemetry / Prometheus / GUI integrations in MVP.

## Repository map

- `tailscope-core/`: instrumentation and run schema
- `tailscope-tokio/`: runtime sampler and macro integration
- `tailscope-cli/`: analyzer and report rendering
- `demos/`: queue and blocking proof cases
- `scripts/`: reproducible demo + validation + runtime-cost scripts
- `docs/`: architecture, diagnostics, and runtime-cost docs

## Development checks

From the repository root:

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```
