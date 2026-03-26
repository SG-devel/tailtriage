# SPEC.md

Product contract for the `tailtriage` triage MVP.

## 1. Product summary

`tailtriage` is a Rust toolkit for **Tokio tail-latency triage**.

Primary question:

> Given one instrumented run, what is the strongest evidence-ranked bottleneck suspect (application queueing, executor pressure, blocking-pool pressure, or downstream stage latency), and what should we check next?

This product is interpretation-first: provide a useful first answer for ordinary developers, then guide deeper investigation.

## 2. Product goals

The MVP must:

1. be easy to integrate into existing Tokio services
2. produce useful output for non-experts with partial instrumentation
3. emit ranked suspects with supporting evidence and actionable next checks
4. stay explicit that suspects are leads, not root-cause proof
5. support reproducible before/after diagnosis from comparable runs
6. measure runtime cost with reproducible scripts

## 3. Non-goals

MVP does **not** include:

- live debugging console
- generalized telemetry/export platform
- observability backend
- distributed tracing system
- metrics backend/exporter
- GUI/web UI
- OpenTelemetry exporter
- Prometheus exporter
- eBPF integration
- non-Tokio runtime support
- auto-remediation or ML root-cause engine
- automated proof claims of causality

## 4. Workspace layout

Current workspace members include:

- `tailtriage-core`
- `tailtriage-tokio`
- `tailtriage-axum`
- `tailtriage-cli`
- `demos/demo_support`
- `demos/queue_service`
- `demos/blocking_service`
- `demos/executor_pressure_service`
- `demos/downstream_service`
- `demos/mixed_contention_service`
- `demos/cold_start_burst_service`
- `demos/db_pool_saturation_service`
- `demos/shared_state_lock_service`
- `demos/retry_storm_service`
- `demos/runtime_cost`

Supporting repository areas:

- `docs/`
- `scripts/`

## 5. Public API

### 5.1 Initialization (`tailtriage-core`)

```rust
use tailtriage_core::{RequestOptions, Tailtriage};

let tailtriage = Tailtriage::builder("invoice-api")
    .light()
    .output("tailtriage-run.json")
    .build()?;
```

### 5.2 Split request lifecycle instrumentation

```rust
let started = tailtriage.begin_request_with(
    "/invoice",
    RequestOptions::new()
        .request_id("req-123")
        .kind("create_invoice"),
);
let request = started.handle.clone();

request
    .queue("invoice_worker")
    .await_on(semaphore.acquire())
    .await;

request
    .stage("fetch_customer")
    .await_on(customer_api.fetch())
    .await?;

started.completion.finish(tailtriage_core::Outcome::Ok);
```

Completion helpers on `RequestCompletion`:

```rust
started.completion.finish_ok();
let result: Result<(), MyError> = started.completion.finish_result(downstream_call().await);
```

### 5.2.1 Request lifecycle contract

`Tailtriage::begin_request(...)` / `begin_request_with(...)` starts one lifecycle and returns a split `StartedRequest`.

- `started.handle` (`RequestHandle`) is instrumentation-only (`queue`, `stage`, `inflight`)
- `started.completion` (`RequestCompletion`) is the only completion path (`finish`, `finish_ok`, `finish_result`)

`RequestCompletion` must be finished exactly once. If it is dropped unfinished, debug builds assert to surface misuse during development. This assertion is a development aid only: `Drop` does **not** infer an outcome and does **not** auto-record request completion.

At `shutdown()`, tailtriage validates unfinished pending requests and surfaces warnings/metadata; it does **not** fabricate completion timing. With `strict_lifecycle(true)`, `shutdown()` fails when unfinished requests remain.

### 5.3 In-flight tracking

```rust
let _inflight = request.inflight("invoice_requests");
```

### 5.4 Queue wait timing wrapper

```rust
request
    .queue("invoice_worker")
    .await_on(semaphore.acquire())
    .await;
```

Optional queue depth sample:

```rust
request
    .queue("invoice_worker")
    .with_depth_at_start(depth)
    .await_on(semaphore.acquire())
    .await;
```

### 5.5 Stage timing wrapper

For fallible stages (`Result` output):

```rust
request
    .stage("fetch_customer")
    .await_on(customer_api.fetch())
    .await;
```

For infallible stages:

```rust
request
    .stage("cache_lookup")
    .await_value(cache.refresh())
    .await;
```

### 5.6 Runtime sampling (`tailtriage-tokio`)

```rust
use std::sync::Arc;
use std::time::Duration;
use tailtriage_core::Tailtriage;
use tailtriage_tokio::RuntimeSampler;

let tailtriage = Arc::new(
    Tailtriage::builder("invoice-api")
        .build()?,
);
let sampler = RuntimeSampler::start(
    Arc::clone(&tailtriage),
    Duration::from_millis(200),
)?;
// ... run workload ...
sampler.shutdown().await;
tailtriage.shutdown()?;
```

### 5.7 Axum adapter surface (`tailtriage-axum`)

`tailtriage-axum` provides a narrow axum ergonomics layer for request-scoped triage:

- middleware: `tailtriage_axum::middleware`
- extractor: `tailtriage_axum::TailtriageRequest`

This adapter reduces repeated framework-boundary wiring. It is an adoption helper, not automatic diagnosis magic.

Middleware behavior:

- starts one request lifecycle per incoming axum request
- finishes with `Outcome::Ok` for non-5xx responses
- finishes with `Outcome::Error` for 5xx responses

Queue/stage/inflight instrumentation remains explicit in handlers/helpers via `TailtriageRequest`.

## 6. Run data model

`tailtriage-core` emits one JSON run artifact with:

- `metadata`
- `requests`
- `stages`
- `queues`
- `inflight`
- `runtime_snapshots`
- `truncation` (per-section dropped counters when capture limits are hit)

Each section captures timestamped events/snapshots used by the CLI triage rules.

Capture limits are configurable through `Tailtriage::builder(...).capture_limits(...)` (`max_requests`, `max_stages`, `max_queues`, `max_inflight_snapshots`, `max_runtime_snapshots`). When limits are hit, capture is deterministically truncated for that section and analyzer output should be interpreted as evidence-ranked suspects from partial data.

## 7. Analyzer CLI (`tailtriage-cli`)

Command:

```text
tailtriage analyze <run.json>
```

Output formats:

- text (default)
- JSON (`--format json`)

The report includes:

- request count
- request p50/p95/p99
- primary suspect
- secondary suspects
- per-suspect evidence + next checks
- warnings when run capture was truncated
- warnings when unfinished request lifecycle state was detected at shutdown

## Script portability strategy

Canonical invocation for demo validation and runtime-cost measurement is **Python-first** (`python3 scripts/*.py`).

- `scripts/*.py` are the source-of-truth implementations.
- Required runtime dependencies for script workflows: `python3` and `cargo`.

## 8. Suspect categories

MVP categories:

- `application_queue_saturation`
- `blocking_pool_pressure`
- `executor_pressure_suspected`
- `downstream_stage_dominates`
- `insufficient_evidence`

Important: these are evidence-ranked suspects, **not** proof of root cause.

## 9. Demos and validation contract

The canonical demo run/validation surface is `python3 scripts/demo_tool.py`.

Supported scenarios:

- `queue`
- `blocking`
- `executor`
- `downstream`
- `mixed`
- `cold-start`
- `db-pool`
- `shared-lock`
- `retry-storm`

Expected baseline diagnosis contract:

- `queue` -> `application_queue_saturation`
- `blocking` -> `blocking_pool_pressure`
- `executor` -> `executor_pressure_suspected`
- `downstream` -> `downstream_stage_dominates`
- `mixed` -> primary `application_queue_saturation`, with downstream suspect also present
- `cold-start` -> `application_queue_saturation`
- `db-pool` -> `application_queue_saturation`
- `shared-lock` -> `application_queue_saturation`
- `retry-storm` -> `downstream_stage_dominates`

These demos are deterministic triage exercises and proof cases for diagnosis behavior. They do not claim universal causality proof.

## 10. Runtime-cost measurement

Repro harness:

- binary: `demos/runtime_cost`
- canonical script: `python3 scripts/measure_runtime_cost.py`

Modes measured:

- baseline
- light
- investigation

Metrics measured:

- throughput
- p50/p95/p99
- relative overhead vs baseline

## 11. Documentation requirements

When behavior or public guidance changes, update as needed:

- `README.md`
- `SPEC.md`
- `IMPLEMENTATION_PLAN.md` (if scope or operating phase changes)
- `docs/README.md`
- `docs/user-guide.md`
- `docs/getting-started-demo.md`
- `docs/diagnostics.md`
- `docs/runtime-cost.md`
- relevant crate docs/readmes

## 12. Definition of done

A change is done only when:

1. scope is satisfied
2. code builds
3. tests pass
4. docs are updated where needed
5. no quiet scope expansion occurred
