# User guide (first use)

This page is intentionally scoped to the **first-use flow**: get one run artifact, analyze it, and interpret the top diagnosis fields.

## When to use tailscope

Use `tailscope` when you need to quickly determine whether tail latency in a Tokio service is most likely caused by:

- **Application-level queueing** (work sitting in queues before execution).
- **Executor or blocking-pool pressure** (runtime contention that inflates wait/dispatch time).
- **A slow downstream stage** (for example a DB/cache/RPC call dominating request time).

## Minimal integration

Start with one collector and only a few wrappers.

1. **Initialize once** near service startup.

```rust
use tailscope_core::{Config, Tailscope};

let tailscope = Tailscope::init(Config::new("my-service"))?;
```

2. **Wrap one request entry point**.

```rust
use tailscope_core::RequestMeta;

tailscope
    .request(
        RequestMeta::for_route("/checkout").with_kind("http"),
        "ok",
        async {
            // request body
        },
    )
    .await;
```

3. **Wrap one queue wait and one stage await** inside that request flow.

```rust
// queue wait
let request_id = RequestMeta::for_route("/checkout").with_kind("http").request_id;
tailscope
    .queue(request_id.clone(), "ingress_queue")
    .await_on(async_work_that_waits())
    .await;

// downstream stage
tailscope
    .stage(request_id, "db_call")
    .await_value(async_downstream_call())
    .await;
```

After collecting data, flush once before process exit:

```rust
tailscope.flush()?;
```

## Analyze one run

Use the CLI analysis command:

```bash
tailscope analyze <run.json> --format json
```

## Read the key output fields

Focus on these fields first:

- **`primary_suspect`**: the top-ranked diagnosis candidate.
  - Start with `primary_suspect.kind` to see the likely bottleneck category.
- **`p95_queue_share_permille`**: how much of p95 latency is attributable to queue time, in permille (per-thousand).
  - Example: `420` means ~42.0% of p95 latency.
- **`evidence`** (under the suspect): concrete signals that supported ranking.
  - Treat this as the “why” behind the suspect selection and as guidance for where to inspect next.

## If result is `InsufficientEvidence`

Take two concrete instrumentation actions next:

1. **Add one more `queue(...).await_on(...)` wrapper** around the most likely uninstrumented wait point in the request path.
2. **Add one more `stage(...).await_on(...)` or `stage(...).await_value(...)` wrapper** around the slowest suspected downstream await.

Then run again and re-analyze.

## Next docs

For details beyond first use, see:

- [Architecture](architecture.md)
- [Diagnostics guide](diagnostics.md)
- [Getting started demos](getting-started-demo.md)
