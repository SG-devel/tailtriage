# tailtriage

`tailtriage` is a focused Rust toolkit for **Tokio tail-latency triage**.

When an async Rust service gets slow, `tailtriage` helps you answer a first practical question quickly:

> Is this slowdown mostly app-level queueing, executor pressure, blocking-pool pressure, or a slow downstream stage?

It produces a triage report with **evidence-ranked suspects** and **next checks**. Suspects are leads, not proof of root cause.

- Built for Tokio services and teams doing iterative triage.
- Useful with partial instrumentation.
- Not an observability backend.
- Not root-cause proof on its own.

## When to use tailtriage

| Symptom | tailtriage helps check |
| --- | --- |
| p95/p99 latency spikes | whether tail latency is dominated by queueing, executor pressure, blocking-pool pressure, or downstream stage latency |
| intermittent request timeouts | whether slow requests share a common bottleneck family in one captured run |
| low CPU but high latency | whether requests are waiting in queues, blocked behind constrained resources, or delayed by downstream work |
| requests appear stuck | whether time is spent before work starts, inside service execution, or in a named downstream stage |
| suspected blocking in async code | whether blocking-pool pressure is visible and should be investigated with a targeted follow-up |
| Tokio runtime seems overloaded | whether captured runtime-pressure signals point toward executor contention rather than app-level queueing |
| queue buildup before work starts | whether application queue wait dominates p95 latency |
| slow database or external API suspected | whether a downstream stage dominates request latency enough to be the next check |
| flaky latency in staging or production | which bottleneck family is the strongest lead from a bounded capture window |
| hard-to-reproduce tail spikes | whether a captured slow window contains enough evidence to choose the next experiment |
| unclear profiler results | whether queueing, runtime pressure, blocking-pool pressure, or downstream waiting explains the tail before pursuing CPU hot paths |
| service has partial instrumentation only | whether available request, queue, stage, runtime, or inflight signals are enough for a useful triage lead |

## Quick start (crates.io)

For direct capture or repeated controller-managed capture windows:

```bash
cargo add tailtriage
```

Optional integrations:

```bash
cargo add tailtriage --features axum
cargo add tailtriage --features tracing
cargo add tailtriage --features tracing-live
cargo add tailtriage --features tracing-tokio
```

`controller` and `tokio` are enabled by default on `tailtriage`; `axum` and tracing intake remain opt-in.

If you want a smaller core-only dependency surface, use `tailtriage-core` directly or depend on `tailtriage` with `default-features = false`.

`tailtriage` captures request/runtime evidence. Install analyzer/report tooling based on how you work.

For command-line analysis of saved Run artifact JSON:

```bash
cargo install tailtriage-cli
```

For in-process Rust analysis/report generation:

```bash
cargo add tailtriage-analyzer
```

Add `tailtriage-analyzer` when you want to analyze a completed Run inside Rust code.
- `tailtriage-cli` consumes Run artifact JSON from disk.
- `tailtriage-analyzer` produces typed `Report` values in process and renders **Report JSON** when you call analyzer renderers.

### Already using tracing?

If your service already emits `tracing` spans, use `tailtriage --features tracing-live` when you want the default crate façade (`tailtriage::tracing`) for live tracing intake, or use `tailtriage-tracing` directly when you want the narrow crate boundary. Native capture remains the default path for new integrations.

Offline import expects completed tailtriage `tt.*` tracing span JSONL (not arbitrary tracing log JSON), requires explicit Unix-ms start/end timestamps, and passes source-valid candidate evidence to core for generic Run integrity normalization. Complete run-relative monotonic offsets improve precision when present; missing offsets remain supported as duration-only legacy evidence.

- Offline JSONL import:
  ```bash
  tailtriage import tracing-spans-jsonl completed-spans.jsonl --service checkout --output tailtriage-run.json
  tailtriage analyze tailtriage-run.json
  ```
- Live session path: install either `tailtriage --features tracing-live` for `tailtriage::tracing::TracingIntakeSession` or `tailtriage-tracing --features live` for `tailtriage_tracing::TracingIntakeSession`, then add `session.layer()` beside your existing subscriber setup.

Both paths convert tracing-shaped evidence into standard `tailtriage_core::Run` data and feed the same analyzer/report workflow (evidence-ranked suspects and next checks). Runtime-pressure evidence still requires runtime snapshots (for example via the Tokio sampler).

### Request ID contract

Within one Run, `request_id` is the tailtriage identity for one completed logical request or work item. It must be unique among completed requests in that Run. Queue and stage events must reuse that ID only for evidence from the same logical request.

External trace or correlation IDs can be broader than a tailtriage request. If an ID can repeat across retries, fanout branches, batch items, or attempts, convert it into a unique tailtriage `request_id` first, for example by adding attempt, span, branch, or item information. `tailtriage` can warn about ambiguous duplicate IDs, but users remain responsible for meaningful instrumentation and request-boundary semantics.


## Why not just tokio-console or tokio-metrics?

Those tools are complementary building blocks. `tailtriage` fills a different gap: it turns request lifecycle timing plus optional runtime signals into a focused triage loop:

`capture -> analyze -> next check -> re-run`

In short:

- `tokio-console` helps you inspect live runtime/task behavior.
- `tokio-metrics` gives you runtime/task metrics signals.
- `tailtriage` helps you rank likely bottleneck families and choose the next targeted check from one captured run.

## Tool comparison

| Tool | Best for | Use with tailtriage when |
| --- | --- | --- |
| `tracing` | structured logs and spans | you need operational context around the captured slow window |
| `tokio-console` | live Tokio task/runtime inspection | tailtriage points toward executor/runtime pressure and you need live inspection |
| `tokio-metrics` | runtime and task metrics | you want runtime signals to strengthen or explain tailtriage evidence |
| `pprof` / flamegraph | CPU hot paths | tailtriage does not show queueing, runtime, blocking-pool, or downstream waiting as the likely lead |
| `tailtriage` | first-pass ranking of likely latency bottleneck families from one run | you need a focused next-check loop rather than continuous observability |

## What you get from the output

### Four bottleneck families

1. **Application queueing**: work waits before execution.
2. **Blocking-pool pressure**: `spawn_blocking` backlog inflates tails.
3. **Executor pressure**: scheduler contention delays runnable work.
4. **Downstream stage latency**: a dependency dominates request time.

### How to read results

- Treat `primary_suspect` as the best lead, not proof.
- Use `evidence[]` to choose one targeted experiment.
- Re-run and compare p95 shares plus suspect evidence.

## Primary entry points

From `tailtriage`:

- `tailtriage::Tailtriage` — direct capture lifecycle
- `tailtriage::controller::TailtriageController` — repeated arm/disarm bounded capture windows for long-lived services
- `tailtriage::tokio` _(default-enabled)_ — runtime-pressure sampling
- `tailtriage::axum` _(optional feature)_ — Axum middleware/extractor ergonomics
- `tailtriage::tracing` _(optional feature)_ — tracing intake bridge for `tt.*` span JSONL import, live recording, and optional Tokio-coupled tracing sessions

## When to choose the controller

Use `tailtriage::controller::TailtriageController` when your service must stay up and you need repeated capture windows over time:

- arm
- collect
- disarm
- re-arm

> The controller is designed to be easy to start with and configurable when you need more control.

You can begin with straightforward builder defaults, then move to a TOML-backed capture template when you want repeatable operational settings across environments.

### Controller TOML config

TOML config is useful when you want to:

- keep startup simple in development, but use standardized capture settings in shared environments
- control run identity, artifact output paths, and retention defaults without rebuilding the service
- define runtime sampler template settings when enabled
- refresh future capture generations with `reload_config()` while leaving the active generation unchanged

See [`tailtriage-controller/README.md`](tailtriage-controller/README.md) for the TOML field reference, expanded TOML example, and reload semantics.
For a runnable TOML-backed startup path, see the public example `controller_toml_startup` in `tailtriage-controller/examples/`.

## Minimal examples

### Single, immediate capture

```rust,no_run
use tailtriage::Tailtriage;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let run = Tailtriage::builder("checkout-service")
        .output("tailtriage-run.json")
        .build()?;

    let started = run.begin_request("/checkout");
    started.completion.finish_ok();

    run.shutdown()?;
    Ok(())
}
```

### Controller capture window with TOML config

```rust,no_run
use tailtriage::controller::TailtriageController;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let controller = TailtriageController::builder("checkout-service")
        .initially_enabled(false)
        .config_path("tailtriage-controller.toml")
        .build()?;

    let _generation = controller.enable()?;
    let started = controller.begin_request("/checkout");
    started.completion.finish_ok();
    let _ = controller.disable()?;

    Ok(())
}
```

### In-process analysis (library)

```rust
use tailtriage_analyzer::{analyze_run, render_json_pretty, render_text, AnalyzeOptions};

# use tailtriage::Run;
# fn example(run: Run) -> Result<(), Box<dyn std::error::Error>> {
let report = analyze_run(&run, AnalyzeOptions::default());
let text = render_text(&report);
let json = render_json_pretty(&report)?;
# let _ = (text, json);
# Ok(())
# }
```

You can avoid JSON output entirely by using `MemorySink` and the typed `Report`, then call `render_json` / `render_json_pretty` only when you need Report JSON.

```rust,no_run
use tailtriage::{MemorySink, Tailtriage};
use tailtriage_analyzer::{analyze_run, render_json_pretty, AnalyzeOptions};

# fn example() -> Result<(), Box<dyn std::error::Error>> {
let sink = MemorySink::new();
let run = Tailtriage::builder("checkout-service")
    .sink(sink.clone())
    .build()?;

let started = run.begin_request("/checkout");
started.completion.finish_ok();
run.shutdown()?;

if let Some(finalized_run) = sink.take_run() {
    let report = analyze_run(&finalized_run, AnalyzeOptions::default());
    let report_json = render_json_pretty(&report)?;
    let _ = report_json;
}
# Ok(())
# }
```

### Analyze artifact (CLI)

```bash
tailtriage analyze tailtriage-run.json --format json
```

Import completed tailtriage tracing span JSONL into a Run artifact first when needed:

```bash
tailtriage import tracing-spans-jsonl completed-spans.jsonl --service checkout --output tailtriage-run.json
```

`tailtriage import tracing-spans-jsonl` imports completed tailtriage tracing span JSONL and writes **Run JSON** (capture artifact and CLI input), not Report JSON. Tracing-specific source parsing and retention happen before core normalization; private provenance joins retained core evidence back to original `SpanRecord` values. Completed-span JSONL contains only retained original source records, preserving source order, source names, IDs, parent IDs, fields, Unix-ms bounds, optional run-relative offsets, and optional explicit duration. Replay is equivalent to direct conversion for normalized request/stage/queue evidence that JSONL can represent; it does not encode Run-only metadata, runtime/in-flight snapshots, lifecycle warnings, truncation or raw-drop counters, source file/line context, omitted-source diagnostics, or output-path failures. Run JSON remains the complete persisted triage artifact. Use `--strict` to fail on malformed/incomplete `tt.*` spans; without `--strict`, malformed `tt.*` spans are skipped and surfaced as `warning: ...` lines on stderr. Arbitrary `tracing_subscriber::fmt().json()` log JSON is not imported, and timing is not guessed from line receive time: completed spans must include explicit Unix-ms start/end timestamps; complete run-relative monotonic offsets are optional and, when present, are preferred for elapsed-duration derivation and validation. Persisted Run JSON intended for `tailtriage analyze` must include at least one completed request event; in-process library snapshots may still be zero-request for inspection.
Tracing-only imports can provide request/stage/queue evidence outside Tokio runtimes, but they do not fabricate runtime-pressure snapshots.

Analyzer thresholds can be tuned through Rust (`AnalyzeOptions`), TOML (`[analyzer]` with `schema_version = 1`), and CLI (`--analyzer-config` / `--analyzer-set`). Start with defaults first, then tune after representative runs. See [docs/diagnostics.md](docs/diagnostics.md), [docs/operations.md](docs/operations.md), and [`examples/analyzer-config.toml`](examples/analyzer-config.toml).

#### Example output (representative JSON)

```json
{
  "request_count": 250,
  "p50_latency_us": 782227,
  "p95_latency_us": 1468239,
  "p99_latency_us": 1518551,
  "p95_queue_share_permille": 982,
  "p95_service_share_permille": 267,
  "inflight_trend": {
    "gauge": "queue_service_inflight",
    "sample_count": 500,
    "peak_count": 234,
    "p95_count": 225,
    "growth_delta": 0,
    "growth_per_sec_milli": 0
  },
  "warnings": [],
  "evidence_quality": {
    "request_count": 250,
    "queue_event_count": 250,
    "stage_event_count": 250,
    "runtime_snapshot_count": 500,
    "inflight_snapshot_count": 500,
    "requests": "present",
    "queues": "present",
    "stages": "present",
    "runtime_snapshots": "present",
    "inflight_snapshots": "present",
    "truncated": false,
    "dropped_requests": 0,
    "dropped_stages": 0,
    "dropped_queues": 0,
    "dropped_inflight_snapshots": 0,
    "dropped_runtime_snapshots": 0,
    "quality": "strong",
    "limitations": []
  },
  "primary_suspect": {
    "kind": "application_queue_saturation",
    "score": 90,
    "confidence": "high",
    "evidence": ["Queue wait at p95 consumes 98.2% of request time."],
    "next_checks": ["Inspect queue admission limits and producer burst patterns."],
    "confidence_notes": []
  },
  "secondary_suspects": [
    {
      "kind": "downstream_stage_dominates",
      "score": 55,
      "confidence": "low",
      "evidence": ["Stage 'simulated_work' has p95 latency 26566 us across 250 samples."],
      "next_checks": ["Inspect downstream dependency behind stage 'simulated_work'."]
    }
  ],
  "route_breakdowns": [],
  "temporal_segments": []
}
```

For full report-field interpretation, see [`docs/diagnostics.md`](docs/diagnostics.md) and [`tailtriage-cli/README.md`](tailtriage-cli/README.md).

`temporal_segments` is always present in JSON output and is usually an empty array. It is populated only when conservative within-run early/late checks find material signal movement (for example, different early/late primary suspects or a large early/late p95 shift). The global `primary_suspect` remains the primary full-run triage lead. Temporal segments are supporting within-run hints only and do not prove a phase-specific root cause. A temporal p95 warning means early/late latency changed materially in that run. Runtime and in-flight phase attribution is timestamp-filtered to each segment window and can be limited when those segment-filtered samples are sparse; with overlapping early/late request windows under concurrency, timestamp-filtered runtime/in-flight attribution is approximate.

## Operations guidance and overhead

For validation scope, claims, and current diagnostic scorecard, see [VALIDATION.md](VALIDATION.md).

For production rollout, capture-mode selection, runtime-sampling decisions, artifact sizing, truncation handling, weak-signal troubleshooting, and current operational limits, start with [`docs/operations.md`](docs/operations.md).

`tailtriage` includes repo-local measurement paths for both runtime-overhead attribution and sustained collector-stress behavior. These are based on synthetic, controlled tests in this repository and should be treated as machine- and workload-scoped guidance, not universal production guarantees.

For overhead attribution and measurement workflow, see [`docs/runtime-cost.md`](docs/runtime-cost.md). For sustained-load behavior, truncation onset, artifact-size growth, and memory trends under stress-shaped workloads, see [`docs/collector-limits.md`](docs/collector-limits.md).

## What this is not

`tailtriage` is not:

- an observability backend
- a distributed tracing system
- a general telemetry platform
- a root-cause proof engine

## Development alternative (workspace checkout)

Use the GitHub/workspace path when you want to run packaged examples, inspect internals, or contribute.

## Examples

Six public examples to start with:

- `minimal_checkout` — fastest capture-to-analyze loop
- `axum_core_manual` — manual Axum + `tailtriage-core` framework wiring
- `axum_service_adoption` — service-shaped Axum adoption example
- `mini_service_integration` — helper-layer or fractured-code instrumentation shape
- `controller_minimal` — arm/disarm controller lifecycle starter
- `controller_toml_startup` — TOML-backed controller startup and activation example

Start with `controller_toml_startup` when you want the most direct example of config-file-driven controller startup.

```bash
cargo run -p tailtriage-tokio --example minimal_checkout
cargo run -p tailtriage-axum --example axum_core_manual
cargo run -p tailtriage-axum --example axum_service_adoption
cargo run -p tailtriage-tokio --example mini_service_integration
cargo run -p tailtriage-controller --example controller_minimal
cargo run -p tailtriage-controller --example controller_toml_startup
python3 scripts/smoke_public_examples.py
```

## Demos

The demos are intentionally small services for Tokio tail-latency triage. They are designed to exercise diagnosis behavior with deterministic, reviewable artifacts, not universal causality proof.

If you only run three demos, start with:

```bash
python3 scripts/demo_tool.py validate queue
python3 scripts/demo_tool.py validate downstream
python3 scripts/demo_tool.py validate db-pool
```

Use before/after comparisons as a reproducible mitigation-confirmation loop, not causal proof.

Demo walkthrough and CI coverage details: [`docs/getting-started-demo.md`](docs/getting-started-demo.md)

## Documentation

The canonical user documentation index lives in [`docs/README.md`](docs/README.md).

Start there for the user workflow, crate selection, controller configuration, analyzer and CLI contracts, diagnostics interpretation, demos, validation, runtime-cost measurement, collector limits, and architecture.
