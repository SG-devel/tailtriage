# User guide (first use)

This guide covers the shortest path to a useful diagnosis.

## 1) Instrument one request flow

```rust
use tailtriage_core::{Config, RequestMeta, Tailtriage};

let tailtriage = Tailtriage::init(Config::new("my-service"))?;

let meta = RequestMeta::for_route("/checkout").with_kind("http");
let request_id = meta.request_id.clone();

tailtriage
    .request(meta, "ok", async {
        tailtriage
            .queue(request_id.clone(), "ingress_queue")
            .await_on(async_work_that_waits())
            .await;

        tailtriage
            .stage(request_id, "db_call")
            .await_value(async_downstream_call())
            .await;
    })
    .await;

tailtriage.flush()?;
```

## 2) Analyze the run

```bash
tailtriage analyze <run.json> --format json
```

Read these fields first:

- `primary_suspect.kind`
- `primary_suspect.evidence[]`
- `primary_suspect.next_checks[]`
- `p95_queue_share_permille`
- `p95_service_share_permille`

## 3) If result is `InsufficientEvidence`

Add one more queue wrapper and one more stage wrapper around the most likely missing wait points, then rerun with comparable load.

## 4) Optional stronger attribution

Enable runtime snapshots when queue/stage instrumentation is still ambiguous:

```rust
use std::sync::Arc;
use std::time::Duration;
use tailtriage_tokio::RuntimeSampler;

let sampler = RuntimeSampler::start(Arc::clone(&tailtriage), Duration::from_millis(200))?;
// run workload
sampler.shutdown().await;
```

## Next docs

- [Diagnostics guide](diagnostics.md)
- [Architecture](architecture.md)
- [Demo workflow](getting-started-demo.md)
