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
- `tailtriage-tracing`
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
- `tailtriage::tokio` (default-enabled runtime sampler/helper namespace; sampler start stays explicit)
- `tailtriage::axum` (optional Axum ergonomics)

For a smaller core-only dependency surface, use `tailtriage-core` directly or use `tailtriage` with `default-features = false`.

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

### 5.8 In-process analyzer (`tailtriage-analyzer`)

`tailtriage-analyzer` owns typed report generation from completed runs:

- `analyze_run(&Run, AnalyzeOptions) -> Report`
- `render_text(&Report)` for human-readable output
- `render_json(&Report)` for canonical compact Report JSON
- `render_json_pretty(&Report)` for canonical pretty Report JSON
- `analyze_run_json(&Run, AnalyzeOptions)` for analyze+compact Report JSON
- `analyze_run_json_pretty(&Run, AnalyzeOptions)` for analyze+pretty Report JSON

Semantics are batch/snapshot for completed runs, not streaming analysis.


Analyzer configuration contract:

- `AnalyzeOptions` is a meaningful configuration surface for tuning analyzer interpretation thresholds while keeping the same capture artifact contract.
- Analyzer configuration is supported across Rust (`AnalyzeOptions` builders), TOML (`[analyzer]` schema with `schema_version = 1`), and CLI (`--analyzer-config` plus `--analyzer-set`).
- `AnalyzeOptions::default()` preserves the current analyzer behavior and is the recommended starting point.
- Report JSON includes `analyzer_config` only when non-default analyzer options are used; default reports omit this field.
- Analyzer tuning changes interpretation/ranking of already captured evidence; it does not change capture artifacts, capture limits, truncation, or what was collected.


### 5.9 Optional tracing intake (`tailtriage-tracing`)

`tailtriage-tracing` is an optional integration surface for services that already emit Rust `tracing` spans.

- primary live path is `TracingIntakeSession` / `TracingIntakeSessionBuilder`; users add `session.layer()` beside their existing `tracing_subscriber` setup
- live session output can write standard Run JSON on shutdown via `run_json_path(...)`
- live session output can write stable completed tailtriage tracing span JSONL on shutdown via `completed_span_jsonl_path(...)`, using retained semantically valid request/stage/queue evidence
- stable completed tailtriage tracing span JSONL wrapper format is:
  `{"format":"tailtriage.tracing-span.v1","span":{...}}`
- CLI imports that wrapper shape with:
  `tailtriage import tracing-spans-jsonl <completed-spans.jsonl> --service <service> --output <run.json>`
- tracing intake converts request/stage/queue evidence into the same standard `Run` schema; analyzer semantics remain unchanged
- request `tt.outcome` is optional; missing defaults to `ok` with a warning, recommended common labels are `ok`/`error`/`timeout`/`cancelled`/`rejected`, and custom non-empty string labels are preserved exactly
- `tracing_subscriber::fmt().json()` arbitrary log scraping is intentionally unsupported
- tracing-only intake does not fabricate runtime-pressure evidence without runtime snapshots / Tokio sampler coupling
- OTel/OTLP and tracing backend behavior are out of scope

### 5.10 Analyzer CLI (`tailtriage-cli`)

`tailtriage-cli` owns artifact loading + command-line report emission and uses `tailtriage-analyzer` for analysis logic. CLI JSON output delegates to `tailtriage-analyzer`’s canonical pretty Report JSON renderer.

Primary command:

```text
tailtriage analyze <run.json>
```

## 6. Run artifact, analyzer, and CLI contracts

Run artifacts include request, stage, queue, in-flight, and optional runtime snapshot data plus metadata/truncation context.

Direct capture lifecycle output options:

- default direct capture writes a local run artifact JSON through `LocalJsonSink`
- choose `MemorySink` when you want a finalized typed `Run` in memory without file output
- choose `DiscardSink` when you want shutdown/finalization without persisting the finalized `Run`

Analyzer output includes:

- request count
- p50/p95/p99 request latency
- p95 queue/service share summaries
- warnings, including analyzer/report warnings such as truncation and evidence-quality limitations
- primary and secondary suspects with evidence and next checks

Schema contract:

- artifacts require top-level `schema_version`
- current supported schema version is `1`

Artifact/report contract split:

- **Run artifact JSON**: capture output and CLI input
- **Report JSON**: analyzer/CLI output and not CLI input
- **Typed `Report`**: in-process analyzer output for Rust users

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
