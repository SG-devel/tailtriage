# User guide (canonical first run)

This is the shortest capture -> analyze -> interpret path.

## 1) Add dependencies

```toml
[dependencies]
tailtriage-core = "0.1"
tailtriage-tokio = "0.1"
tokio = { version = "1", features = ["macros", "rt-multi-thread", "time"] }
```

## 2) Capture one artifact

Use the minimal runnable example:

```bash
cargo run -p tailtriage-tokio --example minimal_checkout
```

Expected output includes `wrote tailtriage-run.json`.

## 3) Analyze

```bash
cargo run -p tailtriage-cli -- analyze tailtriage-run.json --format json
```

## 4) Interpret the diagnosis

Inspect these fields first:

- `primary_suspect.kind`
- `primary_suspect.evidence[]`
- `primary_suspect.next_checks[]`
- `p95_queue_share_permille`
- `p95_service_share_permille`

Representative diagnosis shape:

```json
{
  "primary_suspect": {
    "kind": "ApplicationQueueSaturation",
    "evidence": [
      "Queue wait at p95 consumes 98.2% of request time.",
      "Observed queue depth sample up to 230."
    ],
    "next_checks": [
      "Inspect queue admission limits and producer burst patterns.",
      "Compare queue wait distribution before and after increasing worker parallelism."
    ]
  }
}
```

Suspects are evidence-ranked leads, not proof of root cause.

## 5) If result is `InsufficientEvidence`

Add one more queue wrapper and one more stage wrapper around the most likely missing wait points, then rerun with comparable load.

## 6) Optional stronger attribution

Enable runtime snapshots when queue/stage instrumentation is still ambiguous:

```rust
use std::sync::Arc;
use std::time::Duration;
use tailtriage_tokio::RuntimeSampler;

let sampler = RuntimeSampler::start(Arc::clone(&tailtriage), Duration::from_millis(200))?;
// run workload
sampler.shutdown().await;
```

## Before/after proof path

After first run, validate one mitigation workflow:

- [retry_storm_service before/after comparison](../demos/retry_storm_service/fixtures/before-after-comparison.json)

## Next docs

- [Diagnostics guide](diagnostics.md)
- [Architecture](architecture.md)
- [Demo workflow](getting-started-demo.md)
