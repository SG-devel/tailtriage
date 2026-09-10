# tailtriage-tokio

`tailtriage-tokio` has two independent jobs:

1. **Runtime sampling** periodically adds Tokio runtime observations to an existing
   `Tailtriage` run.
2. **Tokio await helpers** measure selected primitive waits as queue or stage evidence while
   returning Tokio's native results, guards, and permits.

Use either path or combine them. Both add triage evidence; neither proves root cause.

## Installation

For this package's direct API:

```bash
cargo add tailtriage-core tailtriage-tokio
```

The default `tailtriage` package also reexports this surface under `tailtriage::tokio`.

## Runtime sampling

Start sampling explicitly inside an active Tokio runtime. A core `CaptureMode` selects inherited
sampler defaults, but never starts a sampler. Stop and await the sampler before finalizing the core
run:

```rust,no_run
use std::sync::Arc;

use tailtriage_core::{CaptureMode, Tailtriage};
use tailtriage_tokio::RuntimeSampler;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let run = Arc::new(
        Tailtriage::builder("checkout-service")
            .mode(CaptureMode::Investigation)
            .output("tailtriage-run.json")
            .build()?,
    );
    let sampler = RuntimeSampler::builder(Arc::clone(&run)).start()?;

    let started = run.begin_request("/checkout");
    // Run the request workload, then complete its lifecycle explicitly.
    started.completion.finish_ok();

    sampler.shutdown().await;
    run.shutdown()?;
    Ok(())
}
```

Effective sampler retention cannot exceed the core run's resolved runtime-snapshot limit. Runtime
fields can be unavailable: a missing observation is not measured zero and does not rule out the
corresponding pressure.

## Tokio await helpers

Import `TokioRequestHandleExt` to instrument common Tokio waits. In this example, semaphore
acquisition is queue evidence and timeout-wrapped work is stage evidence. The returned permit stays
alive around the protected work, although only acquisition wait is measured:

```rust,no_run
use std::time::Duration;

use tailtriage_core::Tailtriage;
use tailtriage_tokio::TokioRequestHandleExt;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let run = Tailtriage::builder("checkout-service")
        .output("tailtriage-run.json")
        .build()?;
    let started = run.begin_request("/checkout");
    let req = started.handle.clone();
    let capacity = tokio::sync::Semaphore::new(8);

    let permit: tokio::sync::SemaphorePermit<'_> =
        req.semaphore("db_capacity", &capacity).await?;
    let result: Result<Result<(), &'static str>, tokio::time::error::Elapsed> = req
        .timeout_stage("downstream", Duration::from_millis(200), async {
            // Protected work; the permit is still held here.
            Ok::<(), &'static str>(())
        })
        .await;
    drop(permit);

    // Tokio's nested result is preserved. Finish the request before core shutdown.
    started.completion.finish_ok();
    run.shutdown()?;
    result??;
    Ok(())
}
```

Helpers work with borrowed and owned core request handles and do not require runtime sampling.
Constructing a helper does not start timing: timing begins on first poll. Dropping a never-polled
helper records nothing; dropping one after a pending poll may record bounded, lower-bound partial
evidence while capture remains open. Request completion remains the core completion token's job.

The core request handle's `inflight(...)` method is complementary core instrumentation; it is not
provided by `TokioRequestHandleExt`.

## Boundaries

This crate adds Tokio-specific capture evidence. It does not provide controller windows, framework
integration, tracing intake, or report analysis. Its output contributes evidence-ranked suspects
and next checks, not causal certainty.
