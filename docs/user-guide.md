# User guide

Use this page for the shortest path to a first useful triage answer.

## Path A — Try from this repo (source/workspace)

Use this path when you are evaluating `tailtriage` from a local clone of this repository.

### 1) Capture one artifact from the repo example

```bash
cargo run -p tailtriage-tokio --example minimal_checkout
```

Expected output includes `wrote tailtriage-run.json`.

If you want a more realistic request + queue + worker shape outside the synthetic demos, run:

```bash
cargo run -p tailtriage-tokio --example mini_service_integration
```

This mini-service example is an adoption-confidence reference and does **not** replace the demo suite.

### 2) Analyze with the workspace CLI crate

```bash
cargo run -p tailtriage-cli -- analyze tailtriage-run.json --format json
```

### 3) Interpret the diagnosis

Inspect these fields first:

- `primary_suspect.kind`
- `primary_suspect.evidence[]`
- `primary_suspect.next_checks[]`
- `p95_queue_share_permille`
- `p95_service_share_permille`

## Path B — Adopt in your app (crates.io)

Use this path when you want to integrate `tailtriage` into your own Tokio application without workspace context.

### 1) Add dependencies to your app

```toml
[dependencies]
tailtriage-core = "0.1"
tailtriage-tokio = "0.1"
tokio = { version = "1", features = ["macros", "rt-multi-thread", "time"] }
```

### 2) Install the published CLI

```bash
cargo install tailtriage-cli
```

The installed binary name is `tailtriage`.

### 3) Capture one artifact in your app

Create one `Tailtriage` instance, wrap request/queue/stage boundaries, and shut down the artifact to disk at process shutdown.

Minimal shape:

```rust
use tailtriage_core::{Outcome, Tailtriage};

let tailtriage = Tailtriage::builder("checkout-service")
    .output("tailtriage-run.json")
    .build()?;

let request = tailtriage.request("/checkout").with_kind("http");
request
    .queue("queue_wait")
    .await_on(async {})
    .await;
request
    .stage("db_call")
    .await_on(async { Ok::<(), &'static str>(()) })
    .await?;
request.complete(Outcome::Ok);

tailtriage.shutdown()?;
```

For a concrete end-to-end instrumentation shape, mirror [`tailtriage-tokio/examples/minimal_checkout.rs`](../tailtriage-tokio/examples/minimal_checkout.rs).

### 4) Analyze your artifact with the installed binary

```bash
tailtriage analyze tailtriage-run.json --format json
```

### 5) Interpret the first useful answer

Inspect these fields first:

- `primary_suspect.kind`
- `primary_suspect.evidence[]`
- `primary_suspect.next_checks[]`
- `p95_queue_share_permille`
- `p95_service_share_permille`

Representative diagnosis shape:

```json
{
  "primary_suspect": {
    "kind": "ApplicationQueueSaturation",
    "evidence": [
      "Queue wait at p95 consumes 98.2% of request time.",
      "Observed queue depth sample up to 230."
    ],
    "next_checks": [
      "Inspect queue admission limits and producer burst patterns.",
      "Compare queue wait distribution before and after increasing worker parallelism."
    ]
  }
}
```

Suspects are evidence-ranked leads, not proof of root cause.

## If result is `InsufficientEvidence`

Add one more queue wrapper and one more stage wrapper around the most likely missing wait points, then rerun with comparable load.

## Optional stronger attribution

Enable runtime snapshots when queue/stage instrumentation is still ambiguous:

```rust
use std::time::Duration;
use std::sync::Arc;
use tailtriage_core::{SamplingConfig, Tailtriage};
use tailtriage_tokio::RuntimeSampler;

let tailtriage = Arc::new(
    Tailtriage::builder("checkout-service")
        .sampling(SamplingConfig::runtime(Duration::from_millis(200)))
        .build()?,
);
let sampler = RuntimeSampler::start_configured(Arc::clone(&tailtriage))?
    .expect("sampling is enabled");
// run workload
sampler.shutdown().await;
```

## Before/after proof path

After first run, validate one mitigation workflow:

- [retry_storm_service before/after comparison](../demos/retry_storm_service/fixtures/before-after-comparison.json)

## Next docs

- [Documentation index](README.md)
- [Diagnostics guide](diagnostics.md)
- [Architecture](architecture.md)
- [Demo workflow](getting-started-demo.md)
- [Mini-service integration example (source)](../tailtriage-tokio/examples/mini_service_integration.rs)
