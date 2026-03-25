# User guide

Use this guide for a reliable capture → analyze → next-check loop.

## Path A — Use from this repo now

This is the recommended public path today.

### 1) Capture one artifact

```bash
cargo run -p tailtriage-tokio --example minimal_checkout
```

Optional additional examples:

```bash
cargo run -p tailtriage-tokio --example axum_minimal
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

Every request lifecycle starts as a `StartedRequest` and must be finished **exactly once** through its `RequestCompletion`.

```rust
use tailtriage_core::RequestOptions;

let started = tailtriage.begin_request_with(
    "/checkout",
    RequestOptions::new().kind("http"),
);
let request = started.handle.clone();

let started = tailtriage.begin_request_with(
    "/checkout",
    RequestOptions::new().request_id("req-1").kind("http"),
);
let req = started.handle.clone();

helper_a(&req).await?;
helper_b(&req).await?;

started.completion.finish_ok();
```

Terminal methods:

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
- `strict_lifecycle(true)` makes `shutdown()` fail when unfinished requests remain.

## RuntimeSampler (optional stronger attribution)

Use runtime snapshots when request-level signals are not enough to separate queueing vs executor vs blocking-pool pressure.

```rust
use std::sync::Arc;
use std::time::Duration;
use tailtriage_core::Tailtriage;
use tailtriage_tokio::RuntimeSampler;

# async fn demo() -> Result<(), Box<dyn std::error::Error>> {
let tailtriage = Arc::new(
    Tailtriage::builder("checkout-service")
        .output("tailtriage-run.json")
        .build()?,
);
let sampler = RuntimeSampler::start(Arc::clone(&tailtriage), Duration::from_millis(200))?;

// run workload

sampler.shutdown().await;
tailtriage.shutdown()?;
# Ok(())
# }
```

Always call both shutdowns:

- `sampler.shutdown().await`
- `tailtriage.shutdown()?`

Stable Tokio always captures `alive_tasks` and `global_queue_depth`. `local_queue_depth`, `blocking_queue_depth`, and `remote_schedule_count` require `tokio_unstable`.

## If report shows `insufficient_evidence`

Add one queue wrapper and one stage wrapper around the most likely missing waits, rerun under comparable load, then compare suspects/evidence.

## Path B — After crates are published (post-publish path)

Use this path only after crates are released. For launch-day public docs, Path A is the supported path.

```toml
[dependencies]
tailtriage-core = "0.1"
tailtriage-tokio = "0.1"
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
