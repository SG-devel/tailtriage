# tailtriage-core

Core run schema, request-context lifecycle, and instrumentation primitives for `tailtriage`.

`tailtriage-core` is the crate that owns the data model consumed by the CLI analyzer and the per-request capture API used by integrations.

## What this crate owns

- Run artifact schema (`RunArtifact`, requests, runtime snapshots).
- Unified request-context model (`Tailtriage`, `RequestContext`).
- Instrumentation primitives for queue wait, stage timing, and in-flight spans.
- Request lifecycle completion (`finish`, `finish_ok`, `finish_result`) and final artifact flush (`shutdown`).

## When to depend on `tailtriage-core` directly

Use this crate directly when you want to:

- instrument request/work-item flow in your service,
- produce run JSON artifacts for triage,
- or build custom integration code without Tokio runtime sampling.

If you also want periodic Tokio runtime metrics in the same run artifact, add `tailtriage-tokio` alongside this crate.

## Minimal usage

```rust,no_run
use tailtriage_core::{RequestOptions, Tailtriage};

# async fn demo() -> Result<(), Box<dyn std::error::Error>> {
let tailtriage = Tailtriage::builder("checkout-service")
    .output("tailtriage-run.json")
    .build()?;

let request = tailtriage
    .request_with("/checkout", RequestOptions::new().request_id("req-1"))
    .with_kind("http");

request.queue("ingress").await_on(async {
    // wait for semaphore / bounded queue
}).await;

request.stage("db").await_on(async {
    // downstream stage call
    Ok::<(), std::io::Error>(())
}).await?;

request.finish_ok();

tailtriage.shutdown()?;
# Ok(())
# }
```

## First-use guidance

This repository is pre-publish.

- **After first crates.io publish:** add `tailtriage-core` in your app's `Cargo.toml`.
- **Before publish (current state):** use the workspace path dependency from this repository.

## Related docs

- Tokio integration and `RuntimeSampler`: <https://docs.rs/tailtriage-tokio>
- CLI analysis workflow: <https://docs.rs/tailtriage-cli>
- Repository guide and demos: <https://github.com/SG-devel/tailtriage>
