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
- `tailtriage::tracing` (optional tracing intake namespace; JSONL import APIs, live recording APIs with `tracing-live`, and Tokio-coupled sessions with `tracing-tokio`)

For a smaller core-only dependency surface, use `tailtriage-core` directly or use `tailtriage` with `default-features = false`.

### 5.2 Direct capture lifecycle (`Tailtriage`)

Single-run shape:

1. build
2. capture request lifecycle data
3. `shutdown()` to finalize a standard `Run` artifact

### 5.3 Request lifecycle contract

`begin_request(...)` / `begin_request_with(...)` returns `StartedRequest { handle, completion }`.

- `RequestHandle` is instrumentation-only (`queue`, `stage`, `inflight`)
- `RequestCompletion` is the explicit completion path (`finish`, `finish_ok`, `finish_result`)

Contract details:

- completion must happen exactly once through `RequestCompletion`
- dropping an admitted completion token while capture is open records one request with outcome `cancelled`
- `shutdown()` does not fabricate outcomes or timings for unfinished requests
- unfinished requests are surfaced as warnings/metadata
- `strict_lifecycle(true)` makes `shutdown()` fail if unfinished requests remain
- queue/stage timing begins on first poll; a never-polled helper records no event, while a polled-then-dropped helper records one bounded partial child event if capture remains open

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

`tailtriage-analyzer` is the diagnosis engine and owns typed report generation from completed runs:

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

Users can depend on `tailtriage-tracing` directly for the narrow crate boundary, or enable the `tailtriage` façade features (`tracing`, `tracing-live`, `tracing-tokio`) to access the same APIs under `tailtriage::tracing`.

- primary live path is `TracingSession` / `TracingSessionBuilder`; users add `session.layer()` beside their existing `tracing_subscriber` setup
- live session output can write standard Run JSON on shutdown via `run_json_path(...)`
- live session output can write stable completed-span JSONL on shutdown via `completed_span_jsonl_path(...)`, using retained original source spans selected after tracing-specific parsing, semantic limits, and core normalization
- private provenance joins core dispositions back to original `SpanRecord` values before writing; excluded, semantically dropped, and raw-unavailable records are absent and never revived
- direct conversion and wrapper JSONL import preserve supplied source order; live session JSONL is section-grouped as requests, then stages, then queues, preserving recorder order inside each section
- completed-span JSONL replay is equivalent to direct conversion for representable normalized request/stage/queue evidence only; it does not encode Run-only metadata, runtime/in-flight snapshots, lifecycle warnings, truncation/drop counters, source file/line context, omitted-source diagnostics, or output-path failures
- Run JSON remains the complete persisted triage artifact; configured JSONL and Run outputs are independent file transactions
- stable completed-span JSONL wrapper format is:
  `{"format":"tailtriage.tracing-span.v1","span":{...}}`
- CLI imports that wrapper shape with:
  `tailtriage import tracing-spans-jsonl <completed-spans.jsonl> --service <service> --output <run.json>`
- native direct capture and tracing intake both produce standard `Run` artifacts
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

`RequestEvent.request_id` is the per-run identity of one completed logical request/work item. It must be unique among completed requests in one `Run`. `StageEvent.request_id` and `QueueEvent.request_id` must reuse that ID only for evidence from the same logical request. External trace/correlation IDs that can repeat across retries, fanout branches, batch items, or attempts must be converted into a unique tailtriage request ID, for example by adding attempt, span, branch, or item information. The artifact schema records mechanical evidence; users remain responsible for semantically meaningful instrumentation and request-boundary choices.

Direct capture lifecycle output options:

- default direct capture writes a local run artifact JSON through `LocalJsonSink`
- choose `MemorySink` when you want a finalized typed `Run` in memory without file output
- choose `DiscardSink` when you want shutdown/finalization without persisting the finalized `Run`

Analyzer output includes:

- request count
- p50/p95/p99 request latency
- p95 queue/service share summaries
- warnings, including analyzer/report warnings such as truncation and evidence-quality limitations
- canonical core validation warnings in permissive analysis when generic completed-Run evidence is excluded, repaired, or precision-limited
- primary and secondary suspects with evidence and next checks

Suspect ranking selects the primary only after every eligible candidate receives final evidence-aware confidence. The deterministic order is final confidence, then unchanged raw score, then a stable suspect-kind rank; raw-score proximity still drives ambiguity warnings, and a lower raw-score suspect may be promoted when stronger evidence leaves it at higher final confidence. These rankings remain triage leads, not proof of root cause.

Schema contract:

Core Run integrity contract:

- `tailtriage-core` owns generic Run inspection through `inspect_run`, strict rejection through `validate_run_strict`, and deterministic permissive normalization through `normalize_run_permissive`.
- Strict validation rejects unsupported schema versions, blank required metadata/event strings, inverted wall-clock or run-relative intervals, partial run-relative intervals, duration mismatches beyond the shared 2,000 microsecond tolerance, duplicated completed request IDs, orphan or ambiguous request-scoped children, children of excluded parents, and precise child intervals outside precise parent request intervals.
- Permissive normalization keeps duration fields authoritative, clears invalid optional run-relative offsets instead of repairing or clipping them, excludes every duplicated completed request rather than selecting first- or last-wins, excludes children of duplicated/excluded/missing parents, and retains duration-only legacy evidence with deterministic precision warnings.
- Canonical core issue-code summaries are surfaced by analyzer, CLI, tracing import, and native lifecycle output where appropriate; suspects remain evidence-ranked leads, not proof of root cause.
- Native capture owns lifecycle, retention, and artifact construction; tracing owns `tt.*` source parsing, raw retention, semantic limits, JSONL decoding, and line/source warnings; the CLI owns file reading, JSON decoding, schema-envelope errors, command-specific minimum-request requirements, output formatting, stderr, and exit behavior; the analyzer owns diagnostic scoring, evidence quality, report rendering, and non-validation analyzer warnings.
- Strict entry points validate the original unnormalized candidate and reject error-level core findings. Warning-only missing optional precision does not reject.
- Current tracing provenance keeps retained source spans private through normalization and writes completed-span JSONL directly from retained original sources. Prompt 05 owns public tracing API simplification; Prompt 06 owns compatibility-mode removal and stable-wrapper-only input.


- artifacts require top-level `schema_version`
- Run JSON schema version 2 is the current Run JSON schema version
- `metadata.finalized_at_unix_ms` is the sole run-level finalization timestamp; Event-level completion timestamps remain unchanged
- active in-memory snapshots serialize `metadata.finalized_at_unix_ms` as `null`, while persisted CLI artifacts require numeric finalization
- Schema-v1 Run JSON is rejected by the CLI and must be regenerated with a current tailtriage version
- default Run artifact analysis is compatibility-oriented and warns on some ambiguous request-scoped attribution cases instead of failing
- strict Run artifact validation is opt-in through the analyzer strict-validation APIs and `tailtriage analyze --strict-artifact`
- tracing import `--strict` separately controls malformed or incomplete `tt.*` span handling during conversion; it does not replace strict Run artifact validation
- tracing completed-span JSONL import supports the stable wrapper format as the only accepted tracing JSONL file format; pre-stable/internal JSONL must be regenerated with the current writer or converted externally

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

The deterministic corpus validates analyzer/report behavior against labeled fixtures. The current corpus mixes analyzer-executed artifacts (`run_artifact` and `tracing_span_jsonl`, which flow through Run JSON analysis) with report-only fixtures that validate report contract handling without re-running the analyzer.

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

### Request completion, cancellation, and shutdown lifecycle

Explicit completion remains preferred whenever the application knows the request outcome. Dropping an admitted unfinished completion token while capture is still open records one completed request with outcome `cancelled`; Drop is non-panicking, including during panic unwinding. If shutdown wins before a held token finishes or drops, that request is recorded only as unfinished metadata and a late finish or Drop is inert. A finalized Run is immutable to late request admission, completion, stage, queue, in-flight, runtime-snapshot, sampler-metadata, and end-reason mutations.

Strict lifecycle shutdown with pending requests returns a retryable lifecycle error, performs no sink attempt, leaves pending requests open, and does not add finalization timestamps, unfinished metadata, or lifecycle warnings. Once an eligible shutdown attempts the sink, that finalization is terminal and single-shot on both success and failure; repeated or concurrent shutdown callers observe the same terminal attempt rather than writing again. Controller completion Drop participates in admitted-generation drain accounting exactly once, so a closing generation can finalize after the last admitted token is dropped. Completion-token Drop records the cancelled request and does not itself fabricate child evidence. Independently, any queue or stage helper that was polled and then dropped while capture was open records one partial child event.


### Partial queue and stage events

Completed queue and stage JSON remains wire-compatible: schema version stays `2`, older schema-v2 JSON without `completed` reads as completed evidence, and completed events omit `completed` when serialized. The Rust structs now include `completed: bool`, which is an intentional pre-1.0 source break for external exhaustive `StageEvent` and `QueueEvent` struct literals. Prefer `StageEvent::new(...)` and `QueueEvent::new(...)`; constructors default to completed evidence and `into_partial()` should be used only when intentionally constructing partial evidence.

Timing starts on first poll. Dropping a never-polled helper records no event. Dropping a polled pending helper while capture is open records one bounded partial event whose duration ends at observed helper Drop; late Drop after collector finalization is inert. Partial evidence is a lower-bound observation and does not prove that the underlying operation stopped. For partial stages, `success` is forced to `false`; it is not a completed operation result, so completion-aware consumers must inspect `completed`. Tracing spans remain completed-only, and analyzer interpretation is unchanged in this release.

Migration example:

```rust
# use tailtriage_core::StageEvent;
// Old exhaustive struct literal (now must include `completed`).
let _old = StageEvent {
    request_id: "req".into(),
    stage: "db".into(),
    started_at_unix_ms: 1,
    started_at_run_us: None,
    finished_at_unix_ms: 2,
    finished_at_run_us: None,
    latency_us: 10,
    success: true,
    completed: true,
};

// Recommended: constructors default to completed evidence.
let completed = StageEvent::new("req", "db", 1, 2, 10, true);
let partial = completed.clone().into_partial();
```


## Partial queue/stage evidence

Completed queue and stage distributions exclude partial observations. Partial durations are observed lower bounds: tailtriage observed the helper from first poll until Drop, not proof that the underlying operation completed, failed, or stopped. Partial evidence remains visible in event totals, evidence-quality limitations, top-level warnings, and suspect evidence.

Queue/service public p95 fields remain completed-only. A queue or downstream-stage suspect materially relying on an observed-lower-bound path cannot exceed medium confidence; partial evidence that does not affect selected eligibility or score does not automatically cap a completed candidate. Partial stage `success = false` is not interpreted as a completed operation failure.

Global, route, and temporal projections share this policy. Tracing imports remain completed-only. Completed-only Report JSON and text remain unchanged; mixed or partial Runs may change scores or ranking only when explicitly labeled lower-bound evidence is selected and qualified. Suspects remain triage leads, not root-cause proof.
