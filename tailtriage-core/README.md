# tailtriage-core

Core run schema, split request lifecycle API, and instrumentation primitives for `tailtriage`.

## Use from this repo now

From the workspace root, run examples and analysis directly:

```bash
cargo run -p tailtriage-tokio --example minimal_checkout
cargo run -p tailtriage-cli -- analyze tailtriage-run.json --format json
```

## Add from crates.io

```toml
[dependencies]
tailtriage-core = "0.1.1"
```

## What this crate owns

- Run artifact schema (`Run`, requests, stages, queues, inflight snapshots, runtime snapshots)
- Unified started-request model (`Tailtriage`, `StartedRequest`, `RequestHandle`, `RequestCompletion`)
- Queue/stage/in-flight instrumentation primitives
- Explicit completion token lifecycle (`finish`, `finish_ok`, `finish_result`) and final artifact flush (`shutdown`)

## Minimal usage

```rust,no_run
use tailtriage_core::{RequestOptions, Tailtriage};

# async fn demo() -> Result<(), Box<dyn std::error::Error>> {
let tailtriage = Tailtriage::builder("checkout-service")
    .output("tailtriage-run.json")
    .build()?;

let started = tailtriage
    .begin_request_with("/checkout", RequestOptions::new().request_id("req-1").kind("http"));
let request = started.handle.clone();

request.queue("ingress").await_on(async {}).await;
request.stage("db").await_on(async { Ok::<(), std::io::Error>(()) }).await?;
started.completion.finish_ok();

tailtriage.shutdown()?;
# Ok(())
# }
```

## Lifecycle ownership

`begin_request(...)` / `begin_request_with(...)` returns `StartedRequest { handle, completion }`:

- `started.handle` (`RequestHandle`) is instrumentation-only
- `started.completion` (`RequestCompletion`) is the only finish path

`queue(...)`, `stage(...)`, and `inflight(...)` do not finish the request. Every request must be finished exactly once via `finish(...)`, `finish_ok()`, or `finish_result(...)`.

## Shutdown semantics

- `shutdown()` does **not** auto-finish requests.
- `shutdown()` does **not** fabricate timings or outcomes.
- unfinished requests are surfaced in run metadata warnings and unfinished-request samples.
- `strict_lifecycle(true)` makes `shutdown()` return an error when unfinished requests remain.

## Related docs

- Repo docs index: <https://github.com/SG-devel/tailtriage/tree/main/docs>
- Tokio integration crate: <https://github.com/SG-devel/tailtriage/tree/main/tailtriage-tokio>
- CLI crate: <https://github.com/SG-devel/tailtriage/tree/main/tailtriage-cli>
