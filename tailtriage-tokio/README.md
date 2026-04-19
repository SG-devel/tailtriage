# tailtriage-tokio

Tokio integration for `tailtriage`, including `RuntimeSampler` for periodic runtime snapshots.

## Use from the repo

```bash
cargo run -p tailtriage-tokio --example minimal_checkout
cargo run -p tailtriage-cli -- analyze tailtriage-run.json --format json
```

## Add from crates.io

```toml
[dependencies]
tailtriage-core = "0.1.1"
tailtriage-tokio = "0.1.1"
```

## What this crate provides

- `RuntimeSampler` for periodic Tokio runtime snapshots
- Runtime evidence enrichment on the same run artifact used for request instrumentation
- Works alongside the split lifecycle API from `tailtriage-core` (`StartedRequest { handle, completion }`)

## Split lifecycle reminder

Request lifecycle ownership stays in `tailtriage-core`:

- start with `begin_request` / `begin_request_with`
- instrument via `started.handle`
- finish exactly once via `started.completion`

`shutdown()` does not auto-finish pending requests. Unfinished requests are surfaced in run metadata, and `strict_lifecycle(true)` can make shutdown fail.

## `RuntimeSampler` metric availability

Always available on stable Tokio:

- `alive_tasks`
- `global_queue_depth`

Requires `tokio_unstable`:

- `local_queue_depth`
- `blocking_queue_depth`
- `remote_schedule_count`

When `tokio_unstable` is not enabled, unstable-only fields are recorded as `None`.

## `RuntimeSampler` mode inheritance and overrides

`RuntimeSampler::builder(...)` resolves sampler config in this order:

1. **inherited mode** from `Tailtriage` selected mode (`light` / `investigation`)
2. optional **explicit Tokio override** via `.mode(...)`
3. optional cadence override via `.interval(...)`
4. optional retention override via `.max_runtime_snapshots(...)`

Tokio mode defaults (applied only if sampler is started):

- Light: `cadence = 500ms`, `max_runtime_snapshots = 5_000`
- Investigation: `cadence = 100ms`, `max_runtime_snapshots = 50_000`

`CaptureMode` never auto-starts runtime sampling; you must call `.start()`.
`CaptureMode` does not change core event types or `strict_lifecycle`.
Resolved runtime snapshot retention is clamped to the core run's
`max_runtime_snapshots` cap so artifact metadata matches actual retention.

## Minimal usage

```rust,no_run
use std::sync::Arc;

use tailtriage_core::Tailtriage;
use tailtriage_tokio::RuntimeSampler;

# async fn demo() -> Result<(), Box<dyn std::error::Error>> {
let tailtriage = Arc::new(
    Tailtriage::builder("checkout-service")
        .output("tailtriage-run.json")
        .investigation()
        .build()?,
);

let sampler = RuntimeSampler::builder(Arc::clone(&tailtriage))
    // inherits Investigation mode from core when mode(...) is omitted
    .start()?;

// ... run workload ...

sampler.shutdown().await;
tailtriage.shutdown()?;
# Ok(())
# }
```

## Related docs

- Repo docs index: <https://github.com/SG-devel/tailtriage/tree/main/docs>
- Core crate: <https://github.com/SG-devel/tailtriage/tree/main/tailtriage-core>
- CLI crate: <https://github.com/SG-devel/tailtriage/tree/main/tailtriage-cli>

## Axum adapter crate

Axum adoption helpers and axum examples live in `tailtriage-axum`.

The adapter is an ergonomics layer over core primitives. It does not claim production-hardening or zero-instrumentation auto-diagnosis.
