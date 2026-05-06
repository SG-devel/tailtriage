# tailtriage

`tailtriage` is a focused Rust toolkit for **Tokio tail-latency triage**.

When an async Rust service gets slow, `tailtriage` helps you answer a first practical question quickly:

> Is this slowdown mostly app-level queueing, executor pressure, blocking-pool pressure, or a slow downstream stage?

It produces a triage report with **evidence-ranked suspects** and **next checks**. Suspects are leads, not proof of root cause.

- Built for Tokio services and teams doing iterative triage.
- Useful with partial instrumentation.
- Not an observability backend.
- Not root-cause proof on its own.

## Quick start (crates.io)

For most users, start with the default crate:

```bash
cargo add tailtriage
```

Optional integrations:

```bash
cargo add tailtriage --features tokio
cargo add tailtriage --features "tokio,axum"
```

Install analyzer/report tooling based on how you work:

```bash
cargo add tailtriage-analyzer
cargo install tailtriage-cli
```

`tailtriage` captures request/runtime evidence. `tailtriage-analyzer` analyzes completed in-memory runs or stable snapshots in process. `tailtriage-cli` analyzes saved run artifacts from the command line.

## Why not just tokio-console or tokio-metrics?

Those tools are complementary building blocks. `tailtriage` fills a different gap: it turns request lifecycle timing plus optional runtime signals into a focused triage loop:

`capture -> analyze -> next check -> re-run`

In short:

- `tokio-console` helps you inspect live runtime/task behavior.
- `tokio-metrics` gives you runtime/task metrics signals.
- `tailtriage` helps you rank likely bottleneck families and choose the next targeted check from one captured run.

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
- `tailtriage::tokio` _(optional feature)_ — runtime-pressure sampling
- `tailtriage::axum` _(optional feature)_ — Axum middleware/extractor ergonomics

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
use tailtriage_analyzer::{analyze_run, render_text, AnalyzeOptions};

# use tailtriage_core::Run;
# fn example(run: Run) -> Result<(), serde_json::Error> {
let report = analyze_run(&run, AnalyzeOptions::default());
let text = render_text(&report);
let json = serde_json::to_string_pretty(&report)?;
# let _ = (text, json);
# Ok(())
# }
```

### Analyze artifact (CLI)

```bash
tailtriage analyze tailtriage-run.json --format json
```

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
    "evidence": ["Queue wait at p95 consumes 98.2% of request time.", "Observed queue depth sample up to 230."],
    "next_checks": [
      "Inspect queue admission limits and producer burst patterns.",
      "Compare queue wait distribution before and after increasing worker parallelism."
    ],
    "confidence_notes": []
  },
  "secondary_suspects": [
    {
      "kind": "downstream_stage_dominates",
      "score": 55,
      "confidence": "low",
      "evidence": [
        "Stage 'simulated_work' has p95 latency 26566 us across 250 samples.",
        "Stage 'simulated_work' cumulative latency is 6546159 us.",
        "Stage 'simulated_work' contributes 33 permille of cumulative request latency."
      ],
      "next_checks": [
        "Inspect downstream dependency behind stage 'simulated_work'.",
        "Collect downstream service timings and retry behavior during tail windows.",
        "Review downstream SLO/error budget and align retry budget/backoff with it."
      ]
    }
  ],
  "route_breakdowns": [],
  "temporal_segments": []
}
```

`temporal_segments` is always present in JSON output and is usually an empty array. It is populated only when conservative within-run early/late checks find material signal movement (for example, different early/late primary suspects or a large early/late p95 shift). The global `primary_suspect` remains the primary full-run triage lead. Temporal segments are supporting within-run hints only and do not prove a phase-specific root cause. A temporal p95 warning means early/late latency changed materially in that run. Runtime and in-flight phase attribution is timestamp-filtered to each segment window and can be limited when those segment-filtered samples are sparse; with overlapping early/late request windows under concurrency, timestamp-filtered runtime/in-flight attribution is approximate.

## Operations guidance and overhead

For validation scope, claims, and current diagnostic scorecard, see [VALIDATION.md](VALIDATION.md).


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

## Documentation map

- Default crate docs: [`tailtriage/README.md`](tailtriage/README.md)
- User workflow guide: [`docs/user-guide.md`](docs/user-guide.md)
- Controller docs and config: [`tailtriage-controller/README.md`](tailtriage-controller/README.md)
- Runtime sampler docs: [`tailtriage-tokio/README.md`](tailtriage-tokio/README.md)
- In-process analyzer/report contract: [`tailtriage-analyzer/README.md`](tailtriage-analyzer/README.md)
- CLI artifact loader/report emitter: [`tailtriage-cli/README.md`](tailtriage-cli/README.md)
- Diagnostics field reference and interpretation: [`docs/diagnostics.md`](docs/diagnostics.md)
- Demo walkthrough and recommended first demos: [`docs/getting-started-demo.md`](docs/getting-started-demo.md)
- Runtime-overhead measurement path: [`docs/runtime-cost.md`](docs/runtime-cost.md)
- Collector-stress, truncation, artifact-size, and memory guidance: [`docs/collector-limits.md`](docs/collector-limits.md)
- Architecture and crate responsibilities: [`docs/architecture.md`](docs/architecture.md)
- Full docs index: [`docs/README.md`](docs/README.md)
