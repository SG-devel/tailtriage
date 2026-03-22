# tailtriage

`tailtriage` is a Rust toolkit for **Tokio tail-latency triage**.

It is built for ordinary Rust/Tokio developers who need a useful first answer without being expert performance engineers.

Core question:

> Is this request path slow because of **application queueing**, **executor pressure**, **blocking-pool pressure**, or a **slow downstream stage**?

## What it is

`tailtriage` is an interpretation-first diagnosis layer:

- capture one local run artifact from lightweight request, queue, stage, and runtime instrumentation
- analyze it into evidence-ranked suspects
- get concrete next checks for the highest-ranked suspect
- compare before/after runs to keep diagnosis reproducible

Workflow in one line: **capture -> analyze -> choose next check -> re-run**.

## RuntimeSampler availability note (stable Tokio vs `tokio_unstable`)

When you enable `tailtriage-tokio::RuntimeSampler`, runtime snapshot fields differ by Tokio build mode:

- Always available on stable Tokio: `alive_tasks`, `global_queue_depth`
- Requires `tokio_unstable`: `local_queue_depth`, `blocking_queue_depth`, `remote_schedule_count`

On stable Tokio, unstable-only fields are captured as `None`, so executor-pressure vs blocking-pool-pressure separation can be weaker depending on captured request/runtime evidence.

## Who it is for

- developers shipping Tokio services
- teams with latency/backpressure incidents but limited perf-engineering bandwidth
- people who want a fast local triage loop before adopting heavier observability workflows

## Quickstart: choose your path

### Path A — Try from this repo (source/workspace)

Use this when you are exploring `tailtriage` directly from this repository.

1) Run the minimal example and generate an artifact:

```bash
cargo run -p tailtriage-tokio --example minimal_checkout
```

This writes `tailtriage-run.json` in the current directory.

Want a small **adoption-confidence** example that looks more like a service than the synthetic demos? Run:

```bash
cargo run -p tailtriage-tokio --example mini_service_integration
```

That example is intentionally outside `demos/` and exists only as a realistic integration reference (not a production case study).

2) Analyze that artifact with the workspace CLI crate (artifacts include required top-level `schema_version: 1`):

```bash
cargo run -p tailtriage-cli -- analyze tailtriage-run.json --format json
```

3) Read the first useful fields:

- `primary_suspect.kind`
- `primary_suspect.evidence[]`
- `primary_suspect.next_checks[]`
- `p95_queue_share_permille` (95th percentile of per-request queue-time share)
- `p95_service_share_permille` (95th percentile of per-request service-time share)

### Path B — Adopt in your app (crates.io)

Use this when you are integrating `tailtriage` into your own Tokio service.

1) Add library dependencies in your app:

```toml
[dependencies]
tailtriage-core = "0.1"
tailtriage-tokio = "0.1"
tokio = { version = "1", features = ["macros", "rt-multi-thread", "time"] }
```

2) Install the published CLI binary:

```bash
cargo install tailtriage-cli
```

3) Capture one run artifact in your app, then analyze it:

- Capture in your service with `Tailtriage::builder(...).build()?`, explicit request queue/stage wrappers, and `tailtriage.shutdown()?` at process shutdown (see [`docs/user-guide.md`](docs/user-guide.md)).

```bash
tailtriage analyze tailtriage-run.json --format json
```

If you want the smallest realistic capture + analyze flow for an external app, follow **[docs/user-guide.md](docs/user-guide.md)** and use the “Adopt in your app (crates.io)” section.

Representative diagnosis shape:

```json
{
  "primary_suspect": {
    "kind": "application_queue_saturation",
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

## Before/after proof path (secondary)

After first run, use one fixture-backed before/after workflow to validate changes:

- [`demos/retry_storm_service/fixtures/before-after-comparison.json`](demos/retry_storm_service/fixtures/before-after-comparison.json)

## Why not just use tokio-console or tokio-metrics?

Those tools are valuable and complementary:

- **Live debugger/console tools** (for example `tokio-console`) are great for interactive inspection and runtime/task debugging.
- **Raw metrics libraries** (for example `tokio-metrics`) are great for exposing runtime/task measurements.
- **General observability stacks** are great when you need broad telemetry storage, querying, and cross-service operations.

`tailtriage` is different: it focuses on a first useful **triage** answer from a small, local run artifact by ranking suspects and recommending next checks. It is not trying to replace those tools.

## What it is not

`tailtriage` is intentionally **not**:

- a live debugging console
- a generalized telemetry/export platform
- an observability backend
- a distributed tracing system
- an automated root-cause proof engine

Outputs are evidence-ranked leads, not proof of causality.

## Current scope

MVP scope is intentionally narrow:

- Tokio-only
- single-process diagnosis
- local run artifact + CLI analysis
- rule-based suspect ranking
- no distributed tracing backend
- no live UI
- no exporter/backend requirement

## Request lifecycle

Every `RequestContext` starts one request lifecycle and must be finished **exactly once**.

```rust
let request = tailtriage.request("/checkout").with_kind("http");

// queue/stage/inflight instrumentation here

request.finish_ok();
```

Lifecycle contract:

- `queue(...)`, `stage(...)`, and `inflight(...)` record instrumentation only; they do **not** finish the request.
- You must call one terminal method exactly once: `finish(...)`, `finish_ok()`, or `finish_result(...)`.
- `Drop` is a debug-time misuse detector only: unfinished `RequestContext` values trigger a debug assertion in development builds.
- `Drop` does **not** infer success/error and does **not** record request completion automatically.
- Do not rely on scope exit as request completion.

## Bounded capture and truncation

`tailtriage` keeps run data in memory until shutdown. To keep this bounded in production-like runs, configure per-section capture limits on the builder:

```rust
let tailtriage = Tailtriage::builder("checkout-service")
    .capture_limits(tailtriage_core::CaptureLimits::default())
    .build()?;
```

Important request-lifecycle safety note:

- `RequestContext` is `#[must_use]`, and debug builds assert if it is dropped unfinished.
- Finish each request with `finish(...)`, `finish_ok()`, or `finish_result(...)`; scope exit does not finish requests.

Capture limit knobs:

- `max_requests`
- `max_stages`
- `max_queues`
- `max_inflight_snapshots`
- `max_runtime_snapshots`

When a section reaches its configured max, `tailtriage` drops additional entries of that type and increments `truncation` counters in the output artifact. The analyzer also emits warnings when truncation is present so suspects are interpreted as leads from partial data.

## Documentation

For concise docs by audience, start at **[docs/README.md](docs/README.md)**.

For source/workspace and crates.io adoption walkthroughs, see **[docs/user-guide.md](docs/user-guide.md)**.

For demo-specific behavior, recommended public progression, and realism/CI-coverage caveats, see **[demos/README.md](demos/README.md)**.
