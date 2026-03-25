# tailtriage-tokio

Tokio integration for `tailtriage`, including `RuntimeSampler` for periodic runtime snapshots.

For the public repo launch, use workspace/source integration first. Crates.io dependency snippets are post-publish guidance.

## Use from this repo now

```bash
cargo run -p tailtriage-tokio --example minimal_checkout
cargo run -p tailtriage-cli -- analyze tailtriage-run.json --format json
```

## Post-publish crate add (when released)

```toml
[dependencies]
tailtriage-core = "0.1"
tailtriage-tokio = "0.1"
```

## What this crate provides

- `RuntimeSampler` for periodic Tokio runtime snapshots
- Runtime evidence enrichment on the same run artifact used for request instrumentation
- Works with the split lifecycle model from `tailtriage-core` (`StartedRequest`, `RequestHandle`, `RequestCompletion`)

## Split lifecycle reminder

Request lifecycle ownership stays in `tailtriage-core`:

- start with `begin_request` / `begin_request_with`
- instrument via `started.handle`
- finish once via `started.completion`

`shutdown()` does not auto-finish pending requests. Unfinished requests are surfaced in run metadata, and strict lifecycle mode can fail shutdown.

## `RuntimeSampler` metric availability

Always available on stable Tokio:

- `alive_tasks`
- `global_queue_depth`

Requires `tokio_unstable`:

- `local_queue_depth`
- `blocking_queue_depth`
- `remote_schedule_count`

When `tokio_unstable` is not enabled, unstable-only fields are recorded as `None`.

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

## Related docs

- Repo docs index: <https://github.com/SG-devel/tailtriage/tree/main/docs>
- Core crate: <https://github.com/SG-devel/tailtriage/tree/main/tailtriage-core>
- CLI crate: <https://github.com/SG-devel/tailtriage/tree/main/tailtriage-cli>
