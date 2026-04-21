# SPEC.md

Product contract for `tailtriage`.

## 1. Product summary

`tailtriage` is a Rust toolkit for **Tokio tail-latency triage**.

Primary question:

> Is this async Rust service slow because of application-level queueing, executor pressure, blocking-pool pressure, or a slow downstream stage?

`tailtriage` produces **evidence-ranked suspects** and **next checks** from captured run artifacts. Suspects are leads, not proof of root cause.

## 2. Goals

`tailtriage` should:

1. be easy to integrate in Tokio services
2. stay useful with partial instrumentation
3. produce actionable triage output for non-experts
4. keep lifecycle and evidence limits explicit
5. support iterative capture -> analyze -> next check -> re-run workflows

## 3. Non-goals

`tailtriage` is not:

- an observability backend
- a distributed tracing backend
- a metrics/export platform
- a GUI/web diagnosis tool
- an automated causal-proof engine
- a non-Tokio runtime solution

## 4. Workspace surface

Current workspace members include:

- `tailtriage` (default crate)
- `tailtriage-core`
- `tailtriage-controller`
- `tailtriage-tokio`
- `tailtriage-axum`
- `tailtriage-cli`
- demos crates under `demos/`

Supporting repository areas:

- `docs/`
- `scripts/`

## 5. Public integration surfaces

### 5.1 Default entry point: `tailtriage` (default crate)

`tailtriage` is the default onboarding crate and re-exports the primary product surfaces:

- `tailtriage::Tailtriage` (direct capture)
- `tailtriage::controller::TailtriageController` (repeated bounded windows)
- `tailtriage::tokio` (optional runtime sampler integration)
- `tailtriage::axum` (optional Axum ergonomics)

### 5.2 Direct capture lifecycle (`Tailtriage`)

Single-run shape:

1. build
2. capture request lifecycle data
3. `shutdown()` to finalize artifact

### 5.3 Request lifecycle contract

`begin_request(...)` / `begin_request_with(...)` returns `StartedRequest { handle, completion }`.

- `RequestHandle` is instrumentation-only (`queue`, `stage`, `inflight`)
- `RequestCompletion` is the explicit completion path (`finish`, `finish_ok`, `finish_result`)

Contract details:

- completion must happen exactly once
- drop does not auto-complete
- `shutdown()` does not fabricate outcomes or timings
- unfinished requests are surfaced as warnings/metadata
- `strict_lifecycle(true)` makes `shutdown()` fail if unfinished requests remain

### 5.4 Controller surface (`tailtriage-controller`)

`TailtriageController` is for repeated bounded capture windows in long-lived services:

- enable window
- collect
- disable/finalize
- re-enable later

Controller semantics:

- one active generation at a time
- requests admitted to a generation stay bound to that generation
- disabled/closing admissions return inert wrappers

### 5.5 Controller TOML config contract

Controller builder can load TOML template settings via `config_path(...)`.

Reload contract:

- `reload_config()` refreshes template settings from disk
- reload affects **future generations only**
- active generation keeps activation-time config

### 5.6 Runtime sampler (`tailtriage-tokio`)

`RuntimeSampler` adds optional Tokio runtime-pressure snapshots to the same run artifact.

Semantics:

- sampler start requires an active Tokio runtime
- one successful sampler start per run
- `CaptureMode` does not auto-start the sampler
- runtime snapshot retention is bounded by core capture limits
- stable Tokio always provides a subset of fields; some fields require `tokio_unstable`

### 5.7 Axum adapter (`tailtriage-axum`)

`tailtriage-axum` is framework ergonomics, not automatic diagnosis.

- middleware starts/finishes request lifecycle at boundary
- extractor exposes request handle for explicit queue/stage/inflight instrumentation

### 5.8 Analyzer CLI (`tailtriage-cli`)

`tailtriage-cli` analyzes artifacts and renders text/JSON reports.

Primary command:

```text
tailtriage analyze <run.json>
```

## 6. Run artifact and analyzer contract

Run artifacts include request, stage, queue, in-flight, and optional runtime snapshot data plus metadata/truncation context.

Analyzer output includes:

- request count
- p50/p95/p99 request latency
- p95 queue/service share summaries
- warnings (including truncation and lifecycle warnings)
- primary and secondary suspects with evidence and next checks

Schema contract:

- artifacts require top-level `schema_version`
- current supported schema version is `1`

## 7. Suspect taxonomy

Primary suspect kinds:

- `application_queue_saturation`
- `blocking_pool_pressure`
- `executor_pressure_suspected`
- `downstream_stage_dominates`
- `insufficient_evidence`

These are ranked suspects, not proof.

## 8. Runtime-cost and limits measurement contract

Repository-local measurement paths:

- Runtime-overhead attribution: `python3 scripts/measure_runtime_cost.py`
- Sustained collector-stress/limits path: `python3 scripts/measure_collector_limits.py`

These measurements are synthetic, machine-scoped, and workload-scoped guidance from this repository. They are not universal production guarantees.

Runtime-cost interpretation tracks at least:

- baked-in overhead
- core mode overhead
- incremental runtime sampler overhead
- post-limit/drop-path overhead

Collector-limits interpretation tracks at least:

- truncation onset markers
- dropped-category progression
- artifact-size and memory trends under stress profiles

## 9. Documentation contract

When behavior or public guidance changes, update relevant public docs together:

- `README.md`
- `SPEC.md`
- `IMPLEMENTATION_PLAN.md`
- `docs/README.md`
- `docs/user-guide.md`
- `docs/diagnostics.md`
- `docs/runtime-cost.md`
- `docs/collector-limits.md`
- `docs/getting-started-demo.md`
- `docs/architecture.md`
- relevant crate READMEs
- relevant examples, demos, and tests
