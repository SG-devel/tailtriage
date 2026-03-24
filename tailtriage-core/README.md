# tailtriage-core

Core run schema, request-context lifecycle, and instrumentation primitives for `tailtriage`.

For the public repo launch, the primary path is workspace/source integration from this repository. Crates.io snippets below are post-publish guidance.

## Use from this repo now

From the workspace root, run examples and analysis directly:

```bash
cargo run -p tailtriage-tokio --example minimal_checkout
cargo run -p tailtriage-cli -- analyze tailtriage-run.json --format json
```

## Post-publish crate add (when released)

```toml
[dependencies]
tailtriage-core = "0.1"
```

## What this crate owns

- Run artifact schema (`RunArtifact`, requests, runtime snapshots)
- Unified request-context model (`Tailtriage`, `RequestContext`)
- Queue/stage/in-flight instrumentation primitives
- Request lifecycle completion (`finish`, `finish_ok`, `finish_result`) and final artifact flush (`shutdown`)

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

request.queue("ingress").await_on(async {}).await;
request.stage("db").await_on(async { Ok::<(), std::io::Error>(()) }).await?;
request.finish_ok();

tailtriage.shutdown()?;
# Ok(())
# }
```

## Related docs

- Repo docs index: <https://github.com/SG-devel/tailtriage/tree/main/docs>
- Tokio integration crate: <https://github.com/SG-devel/tailtriage/tree/main/tailtriage-tokio>
- CLI crate: <https://github.com/SG-devel/tailtriage/tree/main/tailtriage-cli>
