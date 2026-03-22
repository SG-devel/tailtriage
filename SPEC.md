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

- `tailtriage-core`
- `tailtriage-tokio`
- `tailtriage-cli`
- `demos/`
- `scripts/`
- `docs/`

## 5. Public API (current MVP)

### 5.1 Initialization (`tailtriage-core`)

```rust
use tailtriage_core::{RequestOptions, Tailtriage};

let tailtriage = Tailtriage::builder("invoice-api")
    .light()
    .output("tailtriage-run.json")
    .build()?;
```

### 5.2 Request-context instrumentation

```rust
let request = tailtriage
    .request_with("/invoice", RequestOptions::new().request_id("req-123"))
    .with_kind("create_invoice");

request
    .queue("invoice_worker")
    .await_on(semaphore.acquire())
    .await;

request
    .stage("fetch_customer")
    .await_on(customer_api.fetch())
    .await?;

request.finish(tailtriage_core::Outcome::Ok);
```

Completion helpers on the same request-context model:

```rust
request.finish_ok();
let result: Result<(), MyError> = request.finish_result(downstream_call().await);
```

### 5.2.1 Request lifecycle contract

`RequestContext` starts a request lifecycle and instrumentation wrappers (`queue(...)`, `stage(...)`, `inflight(...)`) do not complete it.

Every request context must call exactly one terminal method:

- `finish(...)`
- `finish_ok()`
- `finish_result(...)`

If a request context is dropped unfinished, debug builds assert to surface misuse during development. This assertion is a development aid only: `Drop` does **not** infer an outcome and does **not** auto-record request completion.

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
```

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

## Script portability strategy

Canonical invocation for demo validation and runtime-cost measurement is **Python-first** (`python3 scripts/*.py`).

- `scripts/*.py` are the source-of-truth implementations.
- Required runtime dependencies for script workflows: `python3` and `cargo`.

## 8. Suspect categories

MVP categories:

- `ApplicationQueueSaturation`
- `BlockingPoolPressure`
- `ExecutorPressureSuspected`
- `DownstreamStageDominates`
- `InsufficientEvidence`

Important: these are evidence-ranked suspects, **not** proof of root cause.

## 9. Demos (required)

- `demos/queue_service`: should rank queue saturation as primary suspect
- `demos/blocking_service`: should rank blocking-pool pressure as primary suspect
- `demos/executor_pressure_service`: should rank executor pressure as primary suspect without relying on blocking-depth evidence

Validation scripts in `scripts/` must pass for these demos.

### 9.1 Additional runnable proof case

- `demos/downstream_service`: deterministic downstream-stage dominance scenario that should rank `DownstreamStageDominates` as the primary suspect.
- `demos/mixed_contention_service`: deterministic mixed queue + downstream contention scenario where both suspects should be present in ranked evidence and mitigation should shift rank and/or score when one bottleneck is reduced.
- `demos/shared_state_lock_service`: deterministic shared-state lock contention scenario where lock wait is modeled as queue-like wait and lock-protected work is modeled as a service stage; mitigation should reduce queueing/serialization signals.
- `demos/retry_storm_service`: deterministic retry-heavy downstream scenario with intermittently failing/slow calls; baseline should show downstream-stage dominance with elevated service share, and mitigated mode should improve p95 and lower suspect score via capped retries/jitter/circuit-break style behavior.

This demo is intentionally small and single-purpose; it extends storytelling trust without expanding MVP scope.

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

When behavior changes, update as needed:

- `README.md`
- `SPEC.md`
- `IMPLEMENTATION_PLAN.md` (if scope/milestone changes)
- `docs/architecture.md`
- `docs/diagnostics.md`
- `docs/runtime-cost.md`

## 12. Definition of done

A change is done only when:

1. scope is satisfied
2. code builds
3. tests pass
4. docs are updated where needed
5. no quiet scope expansion occurred
