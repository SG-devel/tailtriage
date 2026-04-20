# tailtriage

`tailtriage` is a Rust toolkit for **Tokio tail-latency triage**.

## What problem this solves

When a Tokio service gets slow, `tailtriage` helps you answer a first practical question quickly:

> Is this slow because of application-level queueing, executor pressure, blocking-pool pressure, or a slow downstream stage?

It produces **evidence-ranked suspects** with **next checks**. Suspects are leads, not proof of root cause.

## Why not just tokio-console or tokio-metrics?

Those tools are complementary building blocks. `tailtriage` fills a different gap: it gives you a run-level triage report that ranks likely bottleneck families and recommends concrete next checks from the evidence collected in that run.

In short:

- `tokio-console` helps you inspect live runtime/task behavior.
- `tokio-metrics` gives you runtime/task metrics signals.
- `tailtriage` helps you turn request lifecycle timing + optional runtime signals into a focused triage decision loop (`capture -> analyze -> next check -> re-run`).

## Fastest first run from this repo

Use the workspace/source path when you want to run bundled examples and hack on this repository:

```bash
cargo run -p tailtriage-tokio --example minimal_checkout
cargo run -p tailtriage-cli -- analyze tailtriage-run.json --format json
```

## Use published crates in your own project

Use crates.io when adopting `tailtriage` in an external project:

```bash
cargo add tailtriage
cargo add tailtriage --features tokio # optional runtime-pressure evidence
cargo add tailtriage --features "tokio,axum" # optional axum + runtime integrations
cargo install tailtriage-cli
```

For tighter dependency control, you can still add focused crates directly:

```bash
cargo add tailtriage-core
cargo add tailtriage-controller # optional, enabled by default in facade
cargo add tailtriage-tokio # optional
cargo add tailtriage-axum # optional
```

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

### Example output (JSON)

```json
{
  "request_count": 1200,
  "p50_latency_us": 41200,
  "p95_latency_us": 108900,
  "p99_latency_us": 144300,
  "p95_queue_share_permille": 732,
  "p95_service_share_permille": 418,
  "inflight_trend": {
    "gauge": "checkout_inflight",
    "sample_count": 320,
    "peak_count": 74,
    "p95_count": 69,
    "growth_delta": 12,
    "growth_per_sec_milli": 153
  },
  "warnings": [],
  "primary_suspect": {
    "kind": "application_queue_saturation",
    "score": 89,
    "confidence": "high",
    "evidence": [
      "Queue wait at p95 consumes 73.2% of request time.",
      "Observed queue depth sample up to 68."
    ],
    "next_checks": [
      "Inspect queue admission limits and producer burst patterns.",
      "Compare queue wait distribution before and after increasing worker parallelism."
    ]
  },
  "secondary_suspects": [
    {
      "kind": "downstream_stage_dominates",
      "score": 54,
      "confidence": "low",
      "evidence": [
        "Stage 'db' has p95 latency 38700 us across 1200 samples.",
        "Stage 'db' contributes 241 permille of cumulative request latency."
      ],
      "next_checks": [
        "Inspect downstream dependency behind stage 'db'.",
        "Collect downstream service timings and retry behavior during tail windows."
      ]
    }
  ]
}
```

## Examples

Four public examples to start with:

- `minimal_checkout` — fastest capture→analyze loop
- `axum_minimal` — smallest axum framework starter (adapter crate)
- `axum_service_adoption` — service-shaped axum adoption example using the adapter surface
- `mini_service_integration` — helper-layer/fractured-code instrumentation shape

```bash
cargo run -p tailtriage-tokio --example minimal_checkout
cargo run -p tailtriage-axum --example axum_minimal
cargo run -p tailtriage-axum --example axum_service_adoption
cargo run -p tailtriage-tokio --example mini_service_integration
python3 scripts/smoke_public_examples.py
```

## Demos

The nine demos are intentionally small services for Tokio tail-latency triage. They are designed to exercise diagnosis behavior with deterministic and reviewable artifacts, not universal causality proof. If you only run three demos, run the three strongest public proof demos:

```bash
python3 scripts/demo_tool.py validate queue
python3 scripts/demo_tool.py validate downstream
python3 scripts/demo_tool.py validate db-pool
```

Use before/after comparisons as a reproducible mitigation confirmation loop, not causal proof.

Demo walkthrough and CI coverage details: [`docs/getting-started-demo.md`](docs/getting-started-demo.md)

## Which crates do I need?

#### Recommendation by use case

| User type                                         | Recommendation                                                               |
| ------------------------------------------------- | ---------------------------------------------------------------------------- |
| “I just want to try it”                           | Run workspace examples + workspace CLI from source                           |
| “I have a Tokio service”                          | Start with `tailtriage`; add `tailtriage-cli` for analysis                   |
| “I need executor vs blocking evidence”            | Add `tailtriage-tokio`                                                       |
| “I use axum”                                      | Add `tailtriage-axum`; add `tailtriage-tokio` only if runtime snapshots help |
| “I only need to read artifacts from CI/incidents” | Install `tailtriage-cli` only                                                |

#### Dependency / adoption matrix:

| Goal                                                            | Add these crates                                         | Optional                             | Why                                                                                                                        |
| --------------------------------------------------------------- | -------------------------------------------------------- | ------------------------------------ | -------------------------------------------------------------------------------------------------------------------------- |
| Simplest canonical onboarding                                   | `tailtriage`                                             | `tailtriage-cli`                     | Facade re-exports `tailtriage-core` and includes controller convenience by default while keeping integrations feature-gated |
| Instrument a Tokio service, no runtime snapshots                | `tailtriage-core`                                        | `tailtriage-cli`                     | Core request/queue/stage/inflight instrumentation and JSON artifact writing live in `tailtriage-core`                      |
| Instrument a Tokio service and capture runtime pressure signals | `tailtriage-core`, `tailtriage-tokio`                    | `tailtriage-cli`                     | `tailtriage-tokio` provides `RuntimeSampler` and runtime snapshot capture on top of the core artifact model                |
| Use with axum                                                   | `tailtriage-core`, `tailtriage-axum`                     | `tailtriage-tokio`, `tailtriage-cli` | `tailtriage-axum` is the framework ergonomics layer: middleware + extractor. It depends on core, not on `tailtriage-tokio` |
| Use with axum plus runtime snapshots                            | `tailtriage-core`, `tailtriage-axum`, `tailtriage-tokio` | `tailtriage-cli`                     | Axum request-boundary wiring plus optional runtime evidence enrichment                                                     |
| Analyze artifacts only                                          | `tailtriage-cli`                                         | none                                 | CLI loads run JSON, validates schema version, analyzes, and renders text/JSON reports                                      |
| Minimal first run from repo                                     | none beyond workspace                                    | none                                 | Fastest path for bundled examples, demo scripts, and contributor workflows                                                 |

## Measurement methodology and limits

For measurement-path details and conservative interpretation guidance:

- Collector stress methodology/findings/limits + machine-scoped reference guidance: [`docs/collector-limits.md`](docs/collector-limits.md)
- Runtime overhead attribution path: [`docs/runtime-cost.md`](docs/runtime-cost.md)

Use these as distinct measurement paths:

- **Runtime overhead attribution:** isolate baked-in/core/sampler/drop-path overhead categories (`docs/runtime-cost.md`).
- **Sustained-load collector limits:** stress retention and truncation behavior under high event volume (`docs/collector-limits.md`).
- **Artifact-size scaling:** compare shape-driven artifact growth in the collector-limits matrix (`docs/collector-limits.md`).
- **Memory-growth behavior:** compare peak/end RSS trends across stress cases and modes (`docs/collector-limits.md`).

## What this is not

`tailtriage` is not:

- an observability backend
- a distributed tracing system
- a general telemetry platform
- a root-cause proof engine

## RuntimeSampler note (short)

`RuntimeSampler` works on stable Tokio, but some runtime fields (`local_queue_depth`, `blocking_queue_depth`, `remote_schedule_count`) require `tokio_unstable`. See [`docs/user-guide.md`](docs/user-guide.md) for details.

When you use `RuntimeSampler::builder(...)`, Tokio defaults are resolved from the core-selected mode by default (inherited mode), and you can provide an explicit Tokio override with `.mode(...)`.
`RuntimeSampler::start()` requires an active Tokio runtime and allows only one sampler startup per `Tailtriage` run.

## Request lifecycle shape (public API)

`Tailtriage::begin_request(...)` / `begin_request_with(...)` returns `StartedRequest { handle, completion }`:

- `started.handle` (`RequestHandle`) is instrumentation-only (`queue`, `stage`, `inflight`)
- `started.completion` (`RequestCompletion`) is the only finish path (`finish`, `finish_ok`, `finish_result`)

`shutdown()` validates unfinished pending requests and records warnings/metadata. It does not fabricate completion timing. With `strict_lifecycle(true)`, `shutdown()` fails when unfinished requests remain.

## What mode changes in each crate

In `tailtriage-core`, `CaptureMode` controls **retention defaults only**:

- Light core defaults: `max_requests=100_000`, `max_stages=200_000`, `max_queues=200_000`, `max_inflight_snapshots=200_000`, `max_runtime_snapshots=100_000`
- Investigation core defaults: `max_requests=300_000`, `max_stages=600_000`, `max_queues=600_000`, `max_inflight_snapshots=600_000`, `max_runtime_snapshots=300_000`

In `tailtriage-tokio`, mode affects Tokio sampler defaults **only when `RuntimeSampler` is started**:

- Light Tokio defaults: `cadence=500ms`, `max_runtime_snapshots=5_000`
- Investigation Tokio defaults: `cadence=100ms`, `max_runtime_snapshots=50_000`

Precedence for Tokio sampler config resolution:

1. inherited mode from selected core mode
2. optional explicit Tokio mode override via `.mode(...)`
3. optional explicit cadence override via `.interval(...)`
4. optional explicit runtime snapshot retention override via `.max_runtime_snapshots(...)`

What mode does **not** do:

- does **not** auto-enable Tokio sampling (`CaptureMode` never auto-starts `RuntimeSampler`)
- does **not** imply sampler cost by itself (core Investigation alone has no sampler startup cost)
- does **not** require Tokio
- does **not** change event types
- does **not** change lifecycle semantics
- does **not** change `strict_lifecycle`; your explicit `strict_lifecycle(...)` setting is preserved

Saturation behavior note:

- Light remains “lower retention defaults,” not “lower evidence density before saturation.”
- The low-overhead improvement for saturated runs comes from a cheaper post-limit drop path, not from collecting fewer evidence types up front.
- After saturation, tailtriage still records exact drop counters and `limits_hit`, and analyzer warnings still state that dropped evidence can reduce completeness/confidence.
- After saturation, dropped events no longer pay the full append/retention path cost; residual cost is mainly branch checks plus truncation/drop accounting.

Artifacts record both selected mode and effective resolved config:

- selected mode: `metadata.mode`
- core effective config: `metadata.effective_core_config`
- Tokio sampler effective config (recorded only by successful sampler startup): `metadata.effective_tokio_sampler_config`

Overhead terminology used in docs and scripts:

- Core mode overhead
- Tokio mode overhead
- Incremental runtime sampler overhead
- Baked-in overhead
- Post-limit / drop-path overhead

Older artifacts may have `metadata.effective_core_config = null` when effective config was not captured.

## Current public status

The repository is public, and the crates are available now on crates.io:

- <https://crates.io/crates/tailtriage-core>
- <https://crates.io/crates/tailtriage-tokio>
- <https://crates.io/crates/tailtriage-axum>
- <https://crates.io/crates/tailtriage-cli>

Use workspace/source onboarding for repository examples and contributor workflows, and use crates.io onboarding for external-project adoption.

## Documentation map

- Docs index: [`docs/README.md`](docs/README.md)
- Detailed onboarding and lifecycle rules: [`docs/user-guide.md`](docs/user-guide.md)
- Live arm/disarm controller usage and config semantics: [`tailtriage-controller/README.md`](tailtriage-controller/README.md)
- Runtime-cost categories and benchmark interpretation: [`docs/runtime-cost.md`](docs/runtime-cost.md)
- Demo walkthrough and CI coverage details: [`docs/getting-started-demo.md`](docs/getting-started-demo.md)
- Diagnostics field contract and interpretation: [`docs/diagnostics.md`](docs/diagnostics.md)
