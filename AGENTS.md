# AGENTS.md

This file tells coding agents how to work in this repository.

## Mission

Build `tailscope`, a small, useful Rust toolkit for diagnosing tail-latency, queueing, and backpressure problems in Tokio services.

The repository exists to produce a **real developer tool**, not a toy lab and not a generic observability platform.

## Product definition

`tailscope` should answer:

> Is this async Rust service slow because of application-level queueing, executor pressure, blocking-pool pressure, or a slow downstream stage?

The tool should:
- be easy to integrate
- be useful with partial instrumentation
- be low-overhead in light mode
- produce a clear diagnosis report

## What this repository is NOT building

Do not add these to the MVP unless explicitly asked:
- distributed tracing backend
- GUI / web UI
- Prometheus exporter
- OpenTelemetry exporter
- eBPF integration
- Bayesian/statistical diagnosis engine
- ML model ranking
- auto-remediation
- multi-service correlation engine
- GPU support
- non-Tokio runtime support
- C or C++ components

## Technical direction

Use:
- Rust stable
- Tokio
- tracing
- serde
- clap

Prefer:
- clear APIs
- minimal dependencies
- structured JSON outputs
- small modules
- explicit tests

Avoid:
- macro-heavy magic beyond what is needed
- unnecessary async trait abstraction
- clever but opaque code
- premature optimization
- giant generic frameworks

## Workspace structure

Expected workspace members:
- `tailscope-core`
- `tailscope-tokio`
- `tailscope-cli`

Possible directories:
- `demos/`
- `benches/`
- `scripts/`
- `docs/`

## Build and test requirements

Before considering a task done, run:
- `cargo fmt --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`

If a task touches benchmarks or performance-sensitive code, also include:
- benchmark notes
- before/after evidence if behavior changed materially

## Definition of done

A task is done only if:
1. it satisfies the linked issue or task scope
2. code builds
3. tests pass
4. docs/comments are updated where needed
5. public API changes are reflected in README or spec docs
6. scope did not quietly expand

## Performance claims

Do not make unmeasured performance claims.

If the code changes runtime cost or diagnosis behavior:
- add or update a benchmark or fixture
- explain expected trade-offs
- prefer measured evidence over intuition

## API design rules

The API must stay easy to integrate.

Preferred integration style:
- one init call
- one request macro
- small wrappers around important awaits
- optional middleware/layer
- RAII guards where appropriate

Do not design an API that forces developers to:
- rewrite handlers around a custom framework
- manage dozens of instrumentation objects manually
- adopt a tracing backend they did not ask for

## Diagnostics philosophy

`tailscope` should produce:
- ranked suspects
- supporting evidence
- recommended next checks

It should NOT claim:
- proven root cause
- causal certainty
- full distributed-system diagnosis
- correctness beyond the data it actually observes

## Existing ecosystem stance

Assume the following are existing building blocks:
- tracing handles spans/events
- Tokio exposes runtime metrics
- tokio-console is a local debugging/profiling tool
- tokio-metrics is a runtime/task metrics source

`tailscope` is the diagnosis layer above these, not a replacement for them.

## File hygiene

Keep files small and readable.
Prefer:
- one responsibility per module
- straightforward names
- module-level docs for public modules

If adding a public API:
- add rustdoc comments
- add one example if practical

## Tests

Prefer:
- focused unit tests
- fixture-based tests for report generation
- deterministic test inputs

For analyzer logic:
- use explicit sample inputs
- test diagnosis ranking and evidence generation
- avoid brittle string matching unless intended output format is part of the contract

## Benchmarks

For performance-sensitive components:
- keep benchmark inputs simple and reproducible
- store outputs in machine-readable form when useful
- do not merge speculative optimization complexity without evidence

## Demos

Demos are proof cases for the tool, not the product itself.

MVP demos:
- queue/backpressure case
- blocking contamination case

Keep demos small, deterministic, and runnable from scripts.

## Documentation expectations

If behavior changes, update:
- `README.md`
- `SPEC.md`
- `IMPLEMENTATION_PLAN.md` if milestone or scope changed

## How to approach tasks

When given a task:
1. read the issue/task carefully
2. inspect `README.md`, `SPEC.md`, `PLANS.md`, and this file
3. make the smallest reasonable change
4. add tests
5. run format/lint/test
6. summarize what changed and any remaining limitations

For larger tasks:
- propose a short plan first
- keep PRs reviewable
- do not batch unrelated work

## Preferred implementation order

1. `tailscope-core`
2. `tailscope-tokio`
3. `tailscope-cli`
4. demos
5. benchmarks
6. docs polish

## Public API stability

During MVP:
- prefer correctness and usability over premature stability
- but do not churn names casually

If renaming public APIs:
- update examples and docs in the same change

## If uncertain

If unsure whether a change belongs in MVP:
- default to the smaller scope
- leave a note in docs or TODOs
- do not silently expand the product
