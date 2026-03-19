# SPEC.md

Implementation contract for the `tailscope` MVP.

## 1. Product summary

`tailscope` is a Rust toolkit for diagnosing tail-latency, queueing, and backpressure problems in Tokio services.

Primary question:

> Given one instrumented run, what is the strongest suspect: application queueing, executor pressure, blocking-pool pressure, or downstream stage latency?

## 2. Goals

The MVP must:

1. be easy to integrate
2. remain useful with partial instrumentation
3. produce a clear diagnosis report
4. be honest about uncertainty
5. measure runtime cost with reproducible scripts

## 3. Non-goals

MVP does **not** include:

- distributed tracing backend
- metrics backend/exporter
- GUI/web UI
- OpenTelemetry exporter
- Prometheus exporter
- eBPF integration
- non-Tokio runtime support
- auto-remediation or ML root-cause engine

## 4. Workspace layout

- `tailscope-core`
- `tailscope-tokio`
- `tailscope-cli`
- `demos/`
- `scripts/`
- `docs/`

## 5. Public API (current MVP)

### 5.1 Initialization (`tailscope-core`)

```rust
use tailscope_core::{Config, Tailscope};

let mut config = Config::new("invoice-api");
config.output_path = "tailscope-run.json".into();
let tailscope = Tailscope::init(config)?;
```

### 5.2 Request timing wrapper

```rust
use tailscope_core::RequestMeta;

let meta = RequestMeta::for_route("/invoice").with_kind("create_invoice");
let request_id = meta.request_id.clone();

tailscope
    .request(meta, "ok", async move {
        tailscope
            .queue(request_id.clone(), "invoice_worker")
            .await_on(semaphore.acquire())
            .await;
    })
    .await;
```

### 5.3 In-flight tracking

```rust
let _inflight = tailscope.inflight("invoice_requests");
```

### 5.4 Queue wait timing wrapper

```rust
tailscope
    .queue(request_id.clone(), "invoice_worker")
    .await_on(semaphore.acquire())
    .await;
```

Optional queue depth sample:

```rust
tailscope
    .queue(request_id.clone(), "invoice_worker")
    .with_depth_at_start(depth)
    .await_on(semaphore.acquire())
    .await;
```

### 5.5 Stage timing wrapper

```rust
tailscope
    .stage(request_id, "fetch_customer")
    .await_on(customer_api.fetch())
    .await;
```

### 5.6 Runtime sampling (`tailscope-tokio`)

```rust
use std::sync::Arc;
use std::time::Duration;
use tailscope_tokio::RuntimeSampler;

let sampler = RuntimeSampler::start(Arc::clone(&tailscope), Duration::from_millis(200))?;
// ... run workload ...
sampler.shutdown().await;
```

### 5.7 Request attribute macro (`tailscope-tokio`)

`tailscope-tokio` re-exports `#[instrument_request]` from `tailscope-macros` for request entry-point ergonomics.

The macro always emits tracing request events. When `tailscope = <expr>` is provided,
it also records `RequestEvent` entries directly into the active run artifact.

Supported arguments:
- `route = <expr>` (optional; defaults to `module_path!()::fn_name`)
- `kind = <expr>` (optional; defaults to `fn_name`)
- `tailscope = <expr>` (optional; enables run-artifact request recording)
- `request_id = <expr>` (optional; defaults to a route+timestamp id when `tailscope` is set)
- `skip(...)` (optional; passed through to `tracing::instrument`)

## 6. Run data model

`tailscope-core` emits one JSON run artifact with:

- `metadata`
- `requests`
- `stages`
- `queues`
- `inflight`
- `runtime_snapshots`

Each section captures timestamped events/snapshots used by the CLI diagnosis rules.

## 7. Analyzer CLI (`tailscope-cli`)

Command:

```text
tailscope analyze <run.json>
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

## Script portability strategy

Canonical invocation for demo validation and runtime-cost measurement is **Python-first** (`python3 scripts/*.py`).

- `scripts/*.py` are the source-of-truth implementations.
- Required runtime dependencies for script workflows: `python3` and `cargo`.

## 8. Diagnosis categories

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

Validation scripts in `scripts/` must pass for both demos.

### 9.1 Additional runnable proof case

- `demos/downstream_service`: deterministic downstream-stage dominance scenario that should rank `DownstreamStageDominates` as the primary suspect.

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
