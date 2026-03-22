# User guide

Use this page for the shortest path to a first useful triage answer.

## Path A — Try from this repo (source/workspace)

Use this path when you are evaluating `tailtriage` from a local clone of this repository.

### 1) Capture one artifact from the repo example

```bash
cargo run -p tailtriage-tokio --example minimal_checkout
```

Expected output includes `Wrote tailtriage-run.json`.

If you want a more realistic request + queue + worker shape outside the synthetic demos, run:

```bash
cargo run -p tailtriage-tokio --example mini_service_integration
```

This mini-service example is an adoption-confidence reference and does **not** replace the demo suite.

## Finish every request explicitly

Every `RequestContext` starts one request lifecycle. Queue/stage/inflight instrumentation does not finish that lifecycle; you must finish it explicitly exactly once.

```rust
let request = tailtriage.request("/checkout").with_kind("http");

// queue/stage/inflight instrumentation here

request.finish_ok();
```

Use one terminal method per request:

- `finish(...)`
- `finish_ok()`
- `finish_result(...)`

`Drop` is only a debug-time misuse detector. In debug builds, dropping an unfinished context asserts so you catch forgotten finishes during development. `Drop` does **not** infer an outcome and does **not** record request completion automatically.

### 2) Analyze with the workspace CLI crate

```bash
cargo run -p tailtriage-cli -- analyze tailtriage-run.json --format json
```

### 3) Interpret the diagnosis

Inspect these fields first:

- `primary_suspect.kind`
- `primary_suspect.evidence[]`
- `primary_suspect.next_checks[]`
- `p95_queue_share_permille` (95th percentile of per-request queue-time share)
- `p95_service_share_permille` (95th percentile of per-request service-time share)

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
use tailtriage_core::Tailtriage;

let tailtriage = Tailtriage::builder("checkout-service")
    .output("tailtriage-run.json")
    .build()?;

let request = tailtriage.request("/checkout").with_kind("http");
request
    .queue("queue_wait")
    .with_depth_at_start(3)
    .await_on(tokio::time::sleep(std::time::Duration::from_millis(5)))
    .await;
request
    .stage("db_call")
    .await_on(async {
        tokio::time::sleep(std::time::Duration::from_millis(8)).await;
        Ok::<(), &'static str>(())
    })
    .await?;
request.finish_ok();

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
- `p95_queue_share_permille` (95th percentile of per-request queue-time share)
- `p95_service_share_permille` (95th percentile of per-request service-time share)

Representative diagnosis shape:

```json
{
  "primary_suspect": {
    "kind": "application_queue_saturation",
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

## If result is `insufficient_evidence`

Add one more queue wrapper and one more stage wrapper around the most likely missing wait points, then rerun with comparable load.

## Optional stronger attribution

Enable runtime snapshots when queue/stage instrumentation is still ambiguous:

```rust
use std::time::Duration;
use std::sync::Arc;
use tailtriage_core::Tailtriage;
use tailtriage_tokio::RuntimeSampler;

let tailtriage = Arc::new(
    Tailtriage::builder("checkout-service")
        .output("tailtriage-run.json")
        .build()?,
);
let sampler = RuntimeSampler::start(
    Arc::clone(&tailtriage),
    Duration::from_millis(200),
)?;
// run workload
sampler.shutdown().await;
```

### RuntimeSampler metric availability

`RuntimeSampler` does not expose the same fields in all Tokio build modes.

Always available on stable Tokio:

- `alive_tasks`
- `global_queue_depth`

Available only with `tokio_unstable`:

- `local_queue_depth`
- `blocking_queue_depth`
- `remote_schedule_count`

Without `tokio_unstable`, unstable-only fields are captured as `None`.

This means runtime sampling still helps triage on stable Tokio, but blocking-pool vs executor separation can be less decisive depending on which request-level signals you captured.

## Before/after proof path

After first run, validate one mitigation workflow:

- [retry_storm_service before/after comparison](../demos/retry_storm_service/fixtures/before-after-comparison.json)

## Next docs

- [Documentation index](README.md)
- [Diagnostics guide](diagnostics.md)
- [Architecture](architecture.md)
- [Demo workflow](getting-started-demo.md)
- [Mini-service integration example (source)](../tailtriage-tokio/examples/mini_service_integration.rs)
