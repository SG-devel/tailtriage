# tailtriage

`tailtriage` is a Rust toolkit for **Tokio tail-latency triage**.

It is built for ordinary Rust/Tokio developers who need a useful first answer without being expert performance engineers.

Core question:

> Is this request path slow because of **application queueing**, **executor pressure**, **blocking-pool pressure**, or a **slow downstream stage**?

## What it is

`tailtriage` is an interpretation-first diagnosis layer:

- capture one local run artifact from lightweight request, queue, stage, and runtime instrumentation
- analyze it into evidence-ranked suspects
- get concrete next checks for the highest-ranked suspect
- compare before/after runs to keep diagnosis reproducible

Workflow in one line: **capture -> analyze -> choose next check -> re-run**.

## Who it is for

- developers shipping Tokio services
- teams with latency/backpressure incidents but limited perf-engineering bandwidth
- people who want a fast local triage loop before adopting heavier observability workflows

## Why not just use tokio-console or tokio-metrics?

Those tools are valuable and complementary:

- **Live debugger/console tools** (for example `tokio-console`) are great for interactive inspection and runtime/task debugging.
- **Raw metrics libraries** (for example `tokio-metrics`) are great for exposing runtime/task measurements.
- **General observability stacks** are great when you need broad telemetry storage, querying, and cross-service operations.

`tailtriage` is different: it focuses on a first useful **triage** answer from a small, local run artifact by ranking suspects and recommending next checks. It is not trying to replace those tools.

## What it is not

`tailtriage` is intentionally **not**:

- a live debugging console
- a generalized telemetry/export platform
- an observability backend
- a distributed tracing system
- an automated root-cause proof engine

Outputs are evidence-ranked leads, not proof of causality.

## Current scope

MVP scope is intentionally narrow:

- Tokio-only
- single-process diagnosis
- local run artifact + CLI analysis
- rule-based suspect ranking
- no distributed tracing backend
- no live UI
- no exporter/backend requirement

## Quickstart

### 1) Add dependencies

```toml
[dependencies]
tailtriage-core = "0.1"
tailtriage-tokio = "0.1"
tokio = { version = "1", features = ["macros", "rt-multi-thread", "time"] }
```

For local workspace development, swap the version entries for path dependencies in your own repository checkout.

### 2) Instrument one request path

```rust
use std::time::Duration;
use tailtriage_core::{Config, RequestMeta, Tailtriage};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut config = Config::new("checkout-service");
    config.output_path = "tailtriage-run.json".into();
    let tailtriage = Tailtriage::init(config)?;

    let meta = RequestMeta::for_route("/checkout").with_kind("http");
    let request_id = meta.request_id.clone();

    tailtriage
        .request(meta, "ok", async {
            tailtriage
                .queue(request_id.clone(), "ingress_queue")
                .await_on(tokio::time::sleep(Duration::from_millis(5)))
                .await;

            tailtriage
                .stage(request_id, "db_call")
                .await_value(tokio::time::sleep(Duration::from_millis(12)))
                .await;
        })
        .await;

    tailtriage.flush()?;
    Ok(())
}
```

### 3) Analyze

```bash
cargo run --manifest-path tailtriage-cli/Cargo.toml -- analyze tailtriage-run.json --format json
```

Start with:

- `primary_suspect.kind`
- `primary_suspect.evidence[]`
- `primary_suspect.next_checks[]`
- `p95_queue_share_permille`
- `p95_service_share_permille`

## Minimal runnable example

A minimal end-to-end example is available at:

- [`tailtriage-tokio/examples/minimal_checkout.rs`](tailtriage-tokio/examples/minimal_checkout.rs)

Run it with:

```bash
cargo run -p tailtriage-tokio --example minimal_checkout
```

## Canonical integration path

1. Initialize one collector (`Tailtriage::init`).
2. Wrap request entry points (`request(...)` or `#[instrument_request(...)]`).
3. Add a few high-impact `queue(...).await_on(...)` wrappers.
4. Add key downstream `stage(...).await_on(...)` / `await_value(...)` wrappers.
5. Optionally enable `RuntimeSampler::start(...)` for stronger runtime attribution.
6. Flush and analyze.

## Scope and limitations (MVP)

- Tokio-only runtime support.
- Single-process triage (no distributed correlation).
- Rule-based suspect ranking with evidence (not proof of root cause).

## Documentation

For concise docs by audience, start at **[docs/README.md](docs/README.md)**.

For demo-specific behavior and triage expectations, see **[demos/README.md](demos/README.md)**.

For a concrete before/after workflow, see **[demos/retry_storm_service/fixtures/before-after-comparison.json](demos/retry_storm_service/fixtures/before-after-comparison.json)**.
