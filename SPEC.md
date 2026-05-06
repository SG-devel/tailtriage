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
- `tailtriage-analyzer`
- `tailtriage-cli`
- demos crates under `demos/`

Supporting repository areas:

- `docs/`
- `scripts/`
- `validation/`

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

## 6. Run artifact, analyzer, and CLI contracts

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


## 8. Validation contract

Validation exists to show bounded diagnostic behavior, not root-cause proof.

The validation surface includes:

1. deterministic diagnostic corpus validation
2. adversarial synthetic fixtures for sparse, missing, truncated, noisy, or mixed evidence
3. repeated-run controlled demo validation
4. mitigation matrix validation
5. runtime-cost operational validation
6. collector-limit operational validation
7. future real-service validation

### 8.1 Deterministic diagnostic corpus

The deterministic corpus validates analyzer/report behavior against labeled fixtures.

It may check:

- primary suspect expectations
- required top-2 suspect visibility
- expected and allowed warnings
- required evidence substrings
- required next-check substrings
- confidence ceilings for sparse, missing, truncated, noisy, or ambiguous evidence
- high-confidence-wrong counts

Corpus labels describe expected diagnostic-family behavior for controlled fixtures. They are not production root-cause proof.

### 8.2 Repeated-run validation

Repeated-run validation measures stability across repeated controlled demo runs on a specific machine and workload profile.

It may report:

- top-1 accuracy
- top-2 visibility
- primary suspect stability
- high-confidence-wrong count
- confidence bucket summaries
- p95/p99 latency distribution summaries

Repeated-run validation is machine-scoped and workload-scoped.

### 8.3 Mitigation validation

Mitigation validation compares baseline and mitigated controlled runs.

It may check:

- p95/p99 movement
- queue-share movement
- service/stage-share movement
- runtime-pressure movement
- blocking-depth movement
- explainable suspect movement

Mitigation validation supports next-check usefulness. It does not prove formal causality.

### 8.4 Operational validation

Runtime-cost validation measures overhead under documented synthetic workloads.

Collector-limit validation measures bounded retention behavior, visible drops, truncation warnings, and confidence downgrade behavior.

Operational validation is machine-scoped, workload-scoped, and profile-scoped. It is not a universal production guarantee.

### 8.5 Validation non-claims

Validation does not claim:

- root-cause proof from one run
- universal production accuracy
- universal production overhead
- replacement of tracing, metrics, tokio-console, or tokio-metrics
- zero collector drops under all load
- real-service validation until curated real-service artifacts exist

## 9. Runtime-cost and limits measurement contract

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

## 10. Documentation contract

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
