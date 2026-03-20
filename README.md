# tailtriage

`tailtriage` is a Rust toolkit for **tail-latency triage** in Tokio services.

It is built for one practical question:

> Is this request path slow because of **application queueing**, **executor pressure**, **blocking-pool pressure**, or a **slow downstream stage**?

## What it is

`tailtriage` helps ordinary Rust developers get a useful first diagnosis without having to read raw runtime metrics or think like a performance engineer.

It does this by:

- capturing one local run artifact from lightweight request, queue, stage, and runtime instrumentation
- ranking likely bottleneck suspects
- showing evidence for that ranking
- suggesting the next checks to run
- keeping diagnosis reproducible: capture -> analyze -> compare before/after

## What it is not

`tailtriage` is **not** a live debugger, metrics backend, or general observability stack.

If you want raw runtime/task data or an interactive debugging UI, tools like `tokio-console`, `tokio-metrics`, and `dial9-tokio-telemetry` already cover those areas well. `tailtriage` is the interpretation layer on top: a focused tool for turning a small amount of instrumentation into an actionable bottleneck hypothesis. :contentReference[oaicite:0]{index=0}

## Why it is useful

- **Focused on triage:** ranks likely suspects instead of just surfacing telemetry
- **Usable with partial instrumentation:** start with a few key waits/stages and improve coverage over time
- **Low-friction workflow:** one run artifact, one CLI analysis step
- **Honest output:** suspects are evidence-ranked leads, not proof of root cause
- **Made for non-experts:** useful hints first, deeper investigation second

## Best fit

`tailtriage` is a good fit when:

- you run a Tokio service
- you have a tail-latency or backpressure problem
- you want a fast, local, reproducible answer
- you do **not** want to start with a full observability platform

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
tailtriage-core = { path = "../tailtriage-core" }
tailtriage-tokio = { path = "../tailtriage-tokio" }
tokio = { version = "1", features = ["macros", "rt-multi-thread", "time"] }
```

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
- `p95_queue_share_permille`
- `p95_service_share_permille`

## Canonical integration path

1. Initialize one collector (`Tailtriage::init`).
2. Wrap request entry points (`request(...)` or `#[instrument_request(...)]`).
3. Add a few high-impact `queue(...).await_on(...)` wrappers.
4. Add key downstream `stage(...).await_on(...)` / `await_value(...)` wrappers.
5. Optionally enable `RuntimeSampler::start(...)` for stronger runtime attribution.
6. Flush and analyze.

## Scope and limitations (MVP)

- Tokio-only runtime support.
- Single-process diagnosis (no distributed correlation).
- Rule-based suspect ranking with evidence (not proof of root cause).

## Documentation

For concise docs by audience, start at **[docs/README.md](docs/README.md)**.
