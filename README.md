# tailtriage

`tailtriage` is a Rust toolkit for diagnosing **tail latency**, **queueing**, and **backpressure** in Tokio services.

It answers one practical question:

> Is this service slow because of application queueing, executor pressure, blocking-pool pressure, or a slow downstream stage?

## Why it is useful

- Produces one local JSON run artifact from lightweight instrumentation.
- Ranks likely bottleneck suspects with evidence and recommended next checks.
- Works with partial instrumentation (you can start small and improve coverage over time).
- Keeps diagnosis reproducible: capture run -> analyze with CLI -> compare before/after.

## Quickstart

### 1) Add dependencies

```toml
[dependencies]
tailtriage-core = { path = "../tailtriage-core" }
tailtriage-tokio = { path = "../tailtriage-tokio" }
tokio = { version = "1", features = ["macros", "rt-multi-thread", "time"] }
```

### 2) Instrument one request path

```rust
use std::time::Duration;
use tailtriage_core::{Config, RequestMeta, Tailtriage};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut config = Config::new("checkout-service");
    config.output_path = "tailtriage-run.json".into();
    let tailtriage = Tailtriage::init(config)?;

    let meta = RequestMeta::for_route("/checkout").with_kind("http");
    let request_id = meta.request_id.clone();

    tailtriage
        .request(meta, "ok", async {
            tailtriage
                .queue(request_id.clone(), "ingress_queue")
                .await_on(tokio::time::sleep(Duration::from_millis(5)))
                .await;

            tailtriage
                .stage(request_id, "db_call")
                .await_value(tokio::time::sleep(Duration::from_millis(12)))
                .await;
        })
        .await;

    tailtriage.flush()?;
    Ok(())
}
```

### 3) Analyze

```bash
cargo run --manifest-path tailtriage-cli/Cargo.toml -- analyze tailtriage-run.json --format json
```

Start with:

- `primary_suspect.kind`
- `primary_suspect.evidence[]`
- `p95_queue_share_permille`
- `p95_service_share_permille`

## Canonical integration path

1. Initialize one collector (`Tailtriage::init`).
2. Wrap request entry points (`request(...)` or `#[instrument_request(...)]`).
3. Add a few high-impact `queue(...).await_on(...)` wrappers.
4. Add key downstream `stage(...).await_on(...)` / `await_value(...)` wrappers.
5. Optionally enable `RuntimeSampler::start(...)` for stronger runtime attribution.
6. Flush and analyze.

## Scope and limitations (MVP)

- Tokio-only runtime support.
- Single-process diagnosis (no distributed correlation).
- Rule-based suspect ranking with evidence (not proof of root cause).

## Documentation

For concise docs by audience, start at **[docs/README.md](docs/README.md)**.
