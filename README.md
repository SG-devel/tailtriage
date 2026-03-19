# tailscope

`tailscope` is a Rust toolkit for diagnosing **tail latency**, **queueing**, and **backpressure** in Tokio services.

## What tailscope does

- Produces one local JSON run artifact from lightweight request/queue/stage instrumentation.
- Analyzes a run and ranks likely bottleneck suspects (queue saturation, blocking pressure, executor pressure, downstream stage dominance).
- Includes supporting evidence and recommended next checks for each suspect.
- Works with partial instrumentation and can optionally include Tokio runtime sampling for stronger attribution.

## 2-minute quickstart

### 1) Add dependencies

```toml
[dependencies]
tailscope-core = { path = "../tailscope-core" }
tailscope-tokio = { path = "../tailscope-tokio" }
tokio = { version = "1", features = ["macros", "rt-multi-thread", "time"] }
```

### 2) Minimal code (`src/main.rs`)

```rust
use std::time::Duration;

use tailscope_core::{Config, RequestMeta, Tailscope};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut config = Config::new("quickstart-service");
    config.output_path = "tailscope-run.json".into();

    let tailscope = Tailscope::init(config)?;

    let request = RequestMeta::for_route("/demo").with_kind("quickstart");
    let request_id = request.request_id.clone();

    tailscope
        .request(request, "ok", async {
            tailscope
                .queue(request_id.clone(), "ingress_queue")
                .await_on(tokio::time::sleep(Duration::from_millis(5)))
                .await;

            tailscope
                .stage(request_id, "db_call")
                .await_on(tokio::time::sleep(Duration::from_millis(12)))
                .await;
        })
        .await;

    tailscope.flush()?;
    Ok(())
}
```

### 3) Analyze

```bash
cargo run --manifest-path tailscope-cli/Cargo.toml -- analyze tailscope-run.json
```

## Canonical integration path

1. Initialize one collector: `Tailscope::init(Config::new("service-name"))`.
2. Wrap request entry points with `request(...)` (or the macro path from `tailscope-tokio`).
3. Add `queue(...).await_on(...)` around known wait points.
4. Add `stage(...).await_on(...)` around key downstream awaits.
5. Optionally add `inflight(...)` guards and `RuntimeSampler::start(...)` when diagnosis evidence is insufficient.
6. Flush and analyze: `tailscope.flush()?` then `tailscope analyze <run.json>`.

## MVP limitations

- Tokio-only runtime support.
- Single-process diagnosis (no multi-service correlation).
- Rule-based, evidence-ranked diagnosis (not proof of root cause).

## Docs index

- [Architecture](docs/architecture.md)
- [Diagnostics guide](docs/diagnostics.md)
- [Getting started demos](docs/getting-started-demo.md)
- [Runtime cost measurement](docs/runtime-cost.md)
