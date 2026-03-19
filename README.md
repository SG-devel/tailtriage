# tailscope

Async bottleneck diagnosis for Tokio services.

`tailscope` helps answer a practical question:

> Why is my async Rust service slow right now?

It combines:
- request-level timing
- stage timing
- queue-wait timing
- in-flight tracking
- Tokio runtime metrics
- a small diagnosis engine

It is **not** a tracing backend, profiler, or observability platform. It is a **developer-facing diagnosis layer** for Tokio-based services.

## Project status

Early development / design-first.

The MVP goal is:

> Take one Tokio service run and tell the developer whether tail latency is primarily caused by application-level queueing, executor pressure, blocking-pool pressure, or a slow downstream stage.

## Why this exists

Async Rust services are often hard to reason about from logs alone. Developers can usually observe that:
- p99 got worse
- throughput flattened
- requests are “slow”

But the harder question is:
- are requests slow because they are waiting to start?
- is the Tokio runtime itself under pressure?
- is `spawn_blocking` or another blocking path causing trouble?
- is a downstream dependency stretching the tail?
- is the service hiding work in buffers while appearing healthy?

`tailscope` exists to narrow that down quickly.

## Design principles

- **Easy to integrate**
  - One init call
  - One request macro
  - Optional wrappers around the important awaits
- **Useful with partial instrumentation**
  - The tool should provide value even if only a few hot paths are annotated
- **Cheap in normal mode**
  - Default mode should be low-overhead
- **More detail when needed**
  - Investigation mode can be more expensive and more verbose
- **Honest about limits**
  - `tailscope` provides ranked suspects, not proofs

## What `tailscope` is not

`tailscope` is not:
- a replacement for `tracing`
- a replacement for `tokio-console`
- a replacement for `tokio-metrics`
- a tracing backend
- a metrics backend
- an eBPF toolkit
- a general-purpose profiler

It sits **on top of** existing Rust observability primitives and turns them into a diagnosis workflow.

## MVP feature set

The MVP includes:

1. `tailscope-core`
   - request instrumentation
   - stage instrumentation
   - queue-wait instrumentation
   - in-flight tracking
   - local aggregation

2. `tailscope-tokio`
   - Tokio runtime sampling
   - runtime queue depth / blocking queue depth / alive tasks / scheduling clues

3. `tailscope-cli`
   - analyze one run
   - compute stage percentiles
   - rank likely bottlenecks
   - emit a human-readable report and machine-readable JSON

4. `demos/`
   - queue/backpressure demo
   - blocking contamination demo

## Example

Without tailscope:

```rust
async fn create_invoice(state: AppState, input: InvoiceInput) -> anyhow::Result<Invoice> {
    let permit = state.invoice_sem.acquire().await?;

    let customer = state.customer_api.fetch(&input.customer_id).await?;

    let total = state.pricer.calculate(&customer, &input).await?;

    let invoice = state.repo.insert(total).await?;

    drop(permit);

    Ok(invoice)
}

```

With tailscope:

```rust
use tailscope::{inflight, instrument_request, queue, stage};

#[instrument_request(route = "/invoice", kind = "create_invoice", skip(state, input))]
async fn create_invoice(state: AppState, input: InvoiceInput) -> anyhow::Result<Invoice> {
    let _inflight = inflight("invoice_requests");

    let permit = queue("invoice_worker")
        .await_on(state.invoice_sem.acquire())
        .await?;

    let customer = stage("fetch_customer")
        .await_on(state.customer_api.fetch(&input.customer_id))
        .await?;

    let total = stage("price_calculation")
        .await_on(state.pricer.calculate(&customer, &input))
        .await?;

    let invoice = stage("persist_invoice")
        .await_on(state.repo.insert(total))
        .await?;

    drop(permit);

    Ok(invoice)
}
```

What this adds
- total request timing
- queue wait timing
- stage breakdown
- in-flight request tracking
- better diagnosis of p95/p99 behavior

Intended user workflow
1. Add Tailscope::init(...)
2. Add #[instrument_request(...)] to important entry points
3. Wrap the important awaits with queue(...).await_on(...) and stage(...).await_on(...)
4. Run load or benchmark
5. Run tailscope analyze run.json
6. Get a ranked diagnosis report

## Runtime cost target

tailscope should support three practical modes:
- off: effectively no meaningful collection
- light: low-overhead counters/histograms and runtime samples
- investigation: richer timing/tracing for benchmark or incident windows

The repository should measure these costs rather than assert them.

## Non-goals for MVP

We do not aim for:
- distributed tracing backend
- GUI
- OpenTelemetry backend/export
- eBPF integration
- auto-remediation
- ML-based diagnosis
- multi-service distributed root-cause inference
- GPU anything


## Workspace bootstrap status

Current workspace members:
- `tailscope-core`
- `tailscope-tokio`
- `tailscope-cli`

Current repository state:
- Cargo workspace compiles
- CI runs format, clippy, and tests
- `tailscope-core` includes run schema, local JSON sink, and initial `Config`/`Tailscope::init` plus request, in-flight, stage, and queue timing primitives
- `tailscope-tokio` exports `#[instrument_request(...)]` and `RuntimeSampler` for periodic Tokio runtime metrics snapshots
- `tailscope-tokio` records `None` for runtime metrics unavailable without `tokio_unstable` (such as local queue, blocking queue, and remote schedule counters)
- `tailscope-cli` supports `tailscope analyze <run.json>` with text or JSON diagnosis output

### CLI quick start

```bash
tailscope analyze tailscope-run.json
tailscope analyze tailscope-run.json --format json
```

## Development philosophy
- small PRs
- tests before merge
- benchmark before performance claims
- document every public API
- keep the MVP tight
- do not expand scope casually

## Queue and backpressure demo

A reproducible queue-saturation proof case is provided in `demos/queue_service`.

What it does:
- launches a Tokio runtime with low worker-permit capacity
- drives offered load above service capacity
- records queue wait and stage timing with `tailscope-core`
- produces a run artifact and analyzer report

Run the demo and produce analysis:

```bash
scripts/run_queue_demo.sh
```

Validate that the analyzer flags application queue saturation as the primary suspect:

```bash
scripts/validate_queue_demo.sh
```

Artifacts are written to:
- `demos/queue_service/artifacts/queue-run.json`
- `demos/queue_service/artifacts/queue-analysis.json`

A sample analyzer output fixture is stored at:
- `demos/queue_service/fixtures/sample-analysis.json`
