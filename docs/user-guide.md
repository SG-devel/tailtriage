# User guide

Use this guide for a reliable capture → analyze → next-check loop.

## Path A — Run from this repo workspace

Use this path to run bundled examples, demos, and contributor workflows from this repository.

### 1) Capture one artifact

```bash
cargo run -p tailtriage-tokio --example minimal_checkout
```

Optional additional examples:

```bash
cargo run -p tailtriage-axum --example axum_minimal
cargo run -p tailtriage-axum --example axum_service_adoption
cargo run -p tailtriage-tokio --example mini_service_integration
```

### 2) Analyze with the workspace CLI

```bash
cargo run -p tailtriage-cli -- analyze tailtriage-run.json --format json
```

### 3) Read the report in order

1. `primary_suspect.kind`
2. `primary_suspect.evidence[]`
3. `primary_suspect.next_checks[]`
4. `p95_queue_share_permille` / `p95_service_share_permille` as directional context

The p95 share fields are independent percentile summaries and are not expected to sum to `1000`.

## Request lifecycle correctness (required)

`Tailtriage::begin_request(...)` / `begin_request_with(...)` returns a split `StartedRequest { handle, completion }`:

- `started.handle` (`RequestHandle`) is instrumentation-only
- `started.completion` (`RequestCompletion`) is explicit finish-only

```rust
use tailtriage_core::RequestOptions;

let started = tailtriage.begin_request_with(
    "/checkout",
    RequestOptions::new().request_id("req-1").kind("http"),
);
let req = started.handle.clone();

helper_a(&req).await?;
helper_b(&req).await?;

started.completion.finish_ok();
```

Terminal methods on `RequestCompletion`:

- `finish(...)`
- `finish_ok()`
- `finish_result(...)`

`queue(...)`, `stage(...)`, and `inflight(...)` on `RequestHandle` do not finish the request. `Drop` is only a debug-time misuse detector and does not record completion automatically.

Helper-layer functions should take `&RequestHandle<'_>` so instrumentation can be spread across middleware/handlers/service helpers while completion remains single-owner:

```rust
async fn helper_a(req: &tailtriage_core::RequestHandle<'_>) -> Result<(), MyError> {
    req.stage("helper_a").await_on(do_work_a()).await
}
```

### Shutdown lifecycle semantics

`tailtriage.shutdown()` only finalizes and writes the run. It does not complete pending requests.

- `shutdown()` does **not** auto-finish requests.
- `shutdown()` does **not** fabricate timings or outcomes.
- unfinished requests are surfaced in run metadata warnings and unfinished-request samples.
- `strict_lifecycle(true)` makes `shutdown()` return an error when unfinished requests remain.

## RuntimeSampler (optional stronger attribution)

`CaptureMode` in core sets retention defaults only. It does not auto-enable `RuntimeSampler`.
When limits are hit, artifacts keep per-category drop counters and mark that limits were hit;
analyzer warnings call out that dropped evidence can reduce completeness/confidence.
Older artifacts may show `metadata.effective_core_config` as `null` (unknown).

### What mode changes in each crate

Core defaults (`tailtriage-core`):

- Light: `max_requests=100_000`, `max_stages=200_000`, `max_queues=200_000`, `max_inflight_snapshots=200_000`, `max_runtime_snapshots=100_000`
- Investigation: `max_requests=300_000`, `max_stages=600_000`, `max_queues=600_000`, `max_inflight_snapshots=600_000`, `max_runtime_snapshots=300_000`

Tokio defaults (`tailtriage-tokio`, only when sampler is started):

- Light: `cadence=500ms`, `max_runtime_snapshots=5_000`
- Investigation: `cadence=100ms`, `max_runtime_snapshots=50_000`

Tokio sampler override precedence:

1. inherited mode from selected core mode
2. explicit Tokio override via `.mode(...)`
3. explicit cadence override via `.interval(...)`
4. explicit runtime snapshot retention override via `.max_runtime_snapshots(...)`

What mode does **not** do:

- does **not** auto-enable Tokio sampling
- does **not** imply sampler cost by itself (core Investigation alone does not start a sampler)
- does **not** require Tokio
- does **not** change `strict_lifecycle`
- does **not** change event types

Artifacts record both selected mode and effective resolved config:

- selected mode: `metadata.mode`
- core effective config: `metadata.effective_core_config`
- Tokio sampler effective config (when sampler starts): `metadata.effective_tokio_sampler_config`

Overhead terminology used in docs and scripts:

- Core mode overhead
- Tokio mode overhead
- Incremental runtime sampler overhead
- Baked-in overhead
- Post-limit / drop-path overhead

Use runtime snapshots when request-level signals are not enough to separate queueing vs executor vs blocking-pool pressure. For runtime-cost attribution categories and measurement workflow, see [runtime-cost.md](runtime-cost.md).

`RuntimeSampler` resolves Tokio config from:

1. inherited mode from `Tailtriage` selected mode
2. optional explicit Tokio override via `.mode(...)`
3. optional explicit cadence override via `.interval(...)`
4. optional explicit runtime snapshot retention override via `.max_runtime_snapshots(...)`

`CaptureMode` does not auto-start runtime sampling.
Resolved runtime snapshot retention is clamped to the core collector limit for
`max_runtime_snapshots`.

```rust
use std::sync::Arc;
use tailtriage_core::Tailtriage;
use tailtriage_core::CaptureMode;
use tailtriage_tokio::RuntimeSampler;

# async fn demo() -> Result<(), Box<dyn std::error::Error>> {
let tailtriage = Arc::new(
    Tailtriage::builder("checkout-service")
        .output("tailtriage-run.json")
        .light()
        .build()?,
);
let sampler = RuntimeSampler::builder(Arc::clone(&tailtriage))
    .mode(CaptureMode::Investigation)
    .interval(std::time::Duration::from_millis(200))
    .max_runtime_snapshots(25_000)
    .start()?;

// run workload

sampler.shutdown().await;
tailtriage.shutdown()?;
# Ok(())
# }
```

Always call both shutdowns:

- `sampler.shutdown().await`
- `tailtriage.shutdown()?`

`RuntimeSampler` works on stable Tokio, but some runtime fields (`local_queue_depth`, `blocking_queue_depth`, `remote_schedule_count`) require `tokio_unstable`.

## Axum adapter surface (optional)

`tailtriage-axum` provides a narrow axum ergonomics layer for request-scoped triage:

- middleware: `tailtriage_axum::middleware`
- extractor: `tailtriage_axum::TailtriageRequest`

This layer reduces repeated handler boundary code (request start/finish + handle wiring). It is an adoption helper, not auto-instrumentation magic.

```rust,no_run
use std::sync::Arc;
use axum::{extract::State, middleware::from_fn_with_state, routing::get, Router};
use tailtriage_core::Tailtriage;
use tailtriage_axum::{middleware, TailtriageRequest};

# async fn app(tailtriage: Arc<Tailtriage>) {
async fn checkout(TailtriageRequest(req): TailtriageRequest, State(_): State<()>) {
    let _: Result<(), ()> = req.stage("inventory_lookup").await_on(async { Ok(()) }).await;
}

let app = Router::new()
    .route("/checkout", get(checkout))
    .layer(from_fn_with_state(tailtriage, middleware))
    .with_state(());
# let _ = app;
# }
```

Finish semantics at the framework boundary:

- middleware starts one request per incoming axum request
- middleware finishes with `Outcome::Ok` for non-5xx responses and `Outcome::Error` for 5xx responses
- queue/stage/inflight instrumentation remains explicit in handlers/helpers via `TailtriageRequest`

Example split:

- `axum_minimal`: smallest framework starter in `tailtriage-axum` with explicit manual lifecycle wiring
- `axum_service_adoption`: larger service-shaped path in `tailtriage-axum` using the adapter and multiple routes

## If report shows `insufficient_evidence`

Add one queue wrapper and one stage wrapper around the most likely missing waits, rerun under comparable load, then compare suspects/evidence.

## Path B — Use published crates from crates.io

Use this path when adopting `tailtriage` in an external project.

```toml
[dependencies]
tailtriage-core = "0.1.1"
tailtriage-tokio = "0.1.1" # optional, for RuntimeSampler and runtime-pressure evidence
tailtriage-axum = "0.1.1" # optional, only for axum middleware/extractor ergonomics
tokio = { version = "1", features = ["macros", "rt-multi-thread", "time"] }
```

```bash
cargo install tailtriage-cli
tailtriage analyze tailtriage-run.json --format json
```

## Next docs

- [Documentation index](README.md)
- [Demo walkthrough](getting-started-demo.md)
- [Diagnostics details](diagnostics.md)
- [Architecture](architecture.md)
