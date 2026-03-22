# tailtriage-tokio

Tokio-specific integration for `tailtriage`, including `RuntimeSampler` for periodic runtime snapshots.

This crate extends the same `tailtriage-core` request-context workflow with Tokio runtime evidence.

## What this crate provides

- `RuntimeSampler`: periodic Tokio runtime metrics collection into the active run.
- Integration points that keep runtime evidence and request instrumentation in one artifact.

## What `RuntimeSampler` does

`RuntimeSampler` periodically records runtime snapshots such as:

- worker saturation hints,
- queue/backlog indicators,
- blocking-pool pressure indicators.

These snapshots improve triage reports when you need separation between executor pressure, blocking-pool pressure, and application-level queueing.

## When runtime sampling is useful vs optional

Use runtime sampling when:

- service slowdowns are intermittent,
- queueing/executor/blocking-pool suspects are hard to separate from request data alone,
- you want richer evidence-ranked suspects in one run artifact.

Skip runtime sampling when:

- you only need request-level stage and queue instrumentation,
- or you want the lowest-overhead capture mode first.

`tailtriage-core` remains valid without this crate; `tailtriage-tokio` is an optional enrichment path.

## Minimal usage

```rust,no_run
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

// ... run workload ...

sampler.shutdown().await;
tailtriage.shutdown()?;
# Ok(())
# }
```

## First-use guidance

This repository is pre-publish.

- **After first crates.io publish:** add `tailtriage-tokio` and `tailtriage-core` to your app dependencies.
- **Before publish (current state):** use workspace path dependencies from this repository.

## Related docs

- Core request instrumentation API: <https://docs.rs/tailtriage-core>
- CLI diagnosis workflow: <https://docs.rs/tailtriage-cli>
- Repository guide and demos: <https://github.com/SG-devel/tailtriage>
