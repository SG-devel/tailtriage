# tailtriage

`tailtriage` is a Rust toolkit for **Tokio tail-latency triage**.

## What problem this solves

When a Tokio service gets slow, `tailtriage` helps you answer a first practical question quickly:

> Is this slow because of application-level queueing, executor pressure, blocking-pool pressure, or a slow downstream stage?

It produces **evidence-ranked suspects** with **next checks**. Suspects are leads, not proof of root cause.

## Fastest first run

Use the source/workspace path from this public repository:

```bash
cargo run -p tailtriage-tokio --example minimal_checkout
cargo run -p tailtriage-cli -- analyze tailtriage-run.json --format json
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
| “I have a Tokio service”                          | Start with `tailtriage-core`; add `tailtriage-cli` for analysis              |
| “I need executor vs blocking evidence”            | Add `tailtriage-tokio`                                                       |
| “I use axum”                                      | Add `tailtriage-axum`; add `tailtriage-tokio` only if runtime snapshots help |
| “I only need to read artifacts from CI/incidents” | Install `tailtriage-cli` only                                                |


#### Dependency / adoption matrix:

| Goal                                                            | Add these crates                                         | Optional                             | Why                                                                                                                          |
| --------------------------------------------------------------- | -------------------------------------------------------- | ------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------- |
| Instrument a Tokio service, no runtime snapshots                | `tailtriage-core`                                        | `tailtriage-cli`                     | Core request/queue/stage/inflight instrumentation and JSON artifact writing live in `tailtriage-core`                        |
| Instrument a Tokio service and capture runtime pressure signals | `tailtriage-core`, `tailtriage-tokio`                    | `tailtriage-cli`                     | `tailtriage-tokio` provides `RuntimeSampler` and runtime snapshot capture on top of the core artifact model                  |
| Use with axum                                                   | `tailtriage-core`, `tailtriage-axum`                     | `tailtriage-tokio`, `tailtriage-cli` | `tailtriage-axum` is the framework ergonomics layer: middleware + extractor. It depends on core, not on `tailtriage-tokio`   |
| Use with axum plus runtime snapshots                            | `tailtriage-core`, `tailtriage-axum`, `tailtriage-tokio` | `tailtriage-cli`                     | Axum request-boundary wiring plus optional runtime evidence enrichment                                                       |
| Analyze artifacts only                                          | `tailtriage-cli`                                         | none                                 | CLI loads run JSON, validates schema version, analyzes, and renders text/JSON reports                                        |
| Minimal first run from repo                                     | none beyond workspace                                    | none                                 | Recommended launch path today is source/workspace use, not crates.io-first onboarding                                        |

## What this is not

`tailtriage` is not:

- an observability backend
- a distributed tracing system
- a general telemetry platform
- a root-cause proof engine

## Why not just tokio-console or tokio-metrics?

Those tools are complementary building blocks. `tailtriage` is the triage layer that converts one run artifact into evidence-ranked suspects and next checks.

## RuntimeSampler note (short)

`RuntimeSampler` works on stable Tokio, but some runtime fields (`local_queue_depth`, `blocking_queue_depth`, `remote_schedule_count`) require `tokio_unstable`. See [`docs/user-guide.md`](docs/user-guide.md) for details.

## Request lifecycle shape (public API)

`Tailtriage::begin_request(...)` / `begin_request_with(...)` returns `StartedRequest { handle, completion }`:

- `started.handle` (`RequestHandle`) is instrumentation-only (`queue`, `stage`, `inflight`)
- `started.completion` (`RequestCompletion`) is the only finish path (`finish`, `finish_ok`, `finish_result`)

`shutdown()` validates unfinished pending requests and records warnings/metadata. It does not fabricate completion timing. With `strict_lifecycle(true)`, `shutdown()` fails when unfinished requests remain.

## Current public status

The repository is public and ready to use **from source/workspace now**.

Today, the recommended onboarding path is the source path in this repo. Crates.io install snippets are treated as **post-publish** guidance and are not the primary launch path yet.

## Documentation map

- Docs index: [`docs/README.md`](docs/README.md)
- Detailed onboarding and lifecycle rules: [`docs/user-guide.md`](docs/user-guide.md)
- Demo walkthrough and CI coverage details: [`docs/getting-started-demo.md`](docs/getting-started-demo.md)
- Diagnostics field contract and interpretation: [`docs/diagnostics.md`](docs/diagnostics.md)
