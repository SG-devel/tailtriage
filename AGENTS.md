# AGENTS.md

This file tells coding agents how to work in this repository.

## Mission

Build `tailtriage`, a small, useful Rust toolkit for **Tokio tail-latency triage** in real services.

The repository exists to produce a **real developer tool**, not a toy lab and not a generic observability platform.

## Product definition

`tailtriage` should answer:

> Is this async Rust service slow because of application-level queueing, executor pressure, blocking-pool pressure, or a slow downstream stage?

The tool should:
- be easy to integrate
- be useful with partial instrumentation
- be low-overhead in light mode
- produce a clear triage report with evidence-ranked suspects and next checks

## Product language and positioning guardrails

When editing docs, prefer language that reinforces the MVP category:
- use **triage** for product/category language
- use **diagnosis** for analyzer/report actions where natural
- describe output as **evidence-ranked suspects** and **next checks**

Always preserve the distinction:
- suspects are leads
- suspects are **not** proof of root cause

Avoid wording drift toward:
- vague “observability platform” framing
- broad automated-causality claims
- comparisons that imply `tailtriage` replaces tokio-console, tokio-metrics, or telemetry stacks

Optimize docs for:
- first-time users
- non-expert readers
- concrete workflows
- short examples
- direct statements of fit and non-fit
- narrow, honest claims over ambitious language

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
- full multi-service diagnosis
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
- macro-heavy magic beyond what is strictly needed
- unnecessary async trait abstraction
- clever but opaque code
- premature optimization
- giant generic frameworks

## Workspace structure

Expected workspace members:
- `tailtriage-core`
- `tailtriage-tokio`
- `tailtriage-cli`

Possible directories:
- `demos/`
- `benches/`
- `scripts/`
- `docs/`

If the request macro crate becomes unnecessary, remove it instead of preserving extra surface area.

## API design rules

The repository is converging on **one unified public API**.

That unified public API should have:
- one builder/setup path
- one request-context model
- explicit queue/stage/inflight instrumentation on that request context
- one lifecycle completion path
- progressive disclosure for advanced tuning on the same conceptual surface

Do not introduce a second competing onboarding path unless an issue explicitly asks for it.

### Preferred public integration style

Preferred style:
- one builder/init path
- one request-context handle started per request/work item
- small explicit wrappers around important awaits
- fractured-code friendly usage across helper layers
- optional runtime sampling integrated on the same conceptual surface
- RAII guards where appropriate

Do not design an API that forces developers to:
- rewrite handlers around a custom framework
- pack all request logic into one monolithic closure
- manage manual request-ID plumbing in normal usage
- adopt a tracing backend they did not ask for

### Fractured-code requirement

The public API must work when request logic is spread across:
- middleware
- handlers
- service layers
- helper modules
- retries
- fanout code
- spawned tasks where applicable

A convenience closure form may exist, but it must be sugar over the same reusable request-context model, not a separate conceptual API.

### Future-proofing requirement

Without expanding current scope into distributed tracing or cross-service diagnosis, do not paint the API into a corner.

Leave room for future evolution such as:
- richer correlation IDs
- parent/child work relationships
- future propagation across boundaries

Do not implement those features unless explicitly asked.

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
5. public API changes are reflected in launch-facing docs
6. examples are updated where needed
7. demos are updated where needed
8. scope did not quietly expand

For public ergonomics changes, work is **not** done until the teaching surface moves too:
- `README.md`
- `docs/user-guide.md`
- relevant crate docs/readmes
- `examples/`
- `demos/` where public usage patterns are shown

## Hard-removal policy during private phase

The repository is not public yet.

If a new unified public API fully supersedes an older public API, hard removal is allowed and preferred over carrying legacy surface area forward.

Do not keep old public APIs “just in case” unless the issue explicitly requires retention.

If a lower-level primitive is kept, there must be a clear reason that the unified API still cannot represent that capability cleanly.

## Performance claims

Do not make unmeasured performance claims.

If the code changes runtime cost or diagnosis behavior:
- add or update a benchmark or fixture
- explain expected trade-offs
- prefer measured evidence over intuition

## Diagnostics philosophy

`tailtriage` should produce:
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

`tailtriage` is the triage/diagnosis layer above these, not a replacement for them.

## File hygiene

Keep files small and readable.

Prefer:
- one responsibility per module
- straightforward names
- module-level docs for public modules

If adding or changing a public API:
- add rustdoc comments
- add one example if practical
- update public examples if the usage story changes

## Tests

Prefer:
- focused unit tests
- fixture-based tests for report generation
- deterministic test inputs

For analyzer logic:
- use explicit sample inputs
- test diagnosis ranking and evidence generation
- avoid brittle string matching unless intended output format is part of the contract

For public API ergonomics:
- test compact usage
- test fractured-code usage
- test parity of important behavior after API migration
- test advanced knobs on the same unified surface

## Benchmarks

For performance-sensitive components:
- keep benchmark inputs simple and reproducible
- store outputs in machine-readable form when useful
- do not merge speculative optimization complexity without evidence

## Demos

Demos are proof cases for the tool, not the product itself.

Keep demos:
- small
- deterministic
- runnable from scripts
- honest about what they do and do not prove

If public integration ergonomics change, update demos that show public usage patterns so they do not teach stale APIs.

## Documentation expectations

If behavior changes, update:
- `README.md`
- `SPEC.md`
- `IMPLEMENTATION_PLAN.md` if milestone or scope changed
- `docs/user-guide.md`
- relevant crate docs/readmes

Public docs should teach the unified public API first.

Do not leave the repository teaching multiple competing onboarding paths after an ergonomics migration.

## How to approach tasks

When given a task:
1. read the issue/task carefully
2. inspect `README.md`, `SPEC.md`, `IMPLEMENTATION_PLAN.md`, and this file
3. make the smallest reasonable change that solves the actual problem
4. add tests
5. update docs/examples/demos where needed
6. run format/lint/test
7. summarize what changed and any remaining limitations

For larger tasks:
- propose a short plan first
- keep PRs reviewable
- do not batch unrelated work

## Preferred implementation order

For unified API work, prefer this order:
1. `AGENTS.md` / repo guidance
2. `tailtriage-core` public API shape
3. `tailtriage-tokio` integration alignment
4. `tailtriage-cli` adjustments if needed
5. examples
6. demos
7. docs/readmes
8. cleanup/removal of superseded APIs

## Public API stability

During MVP:
- prefer correctness and usability over premature stability
- but do not churn names casually

Because the repository is still private, removing superseded APIs is acceptable if it leads to one cleaner final public API.

If changing public APIs:
- update examples and docs in the same change set
- remove or clearly justify retained overlapping surfaces
- do not leave both old and new paths equally endorsed

## If uncertain

If unsure whether a change belongs in MVP:
- default to the smaller scope
- leave a note in docs or TODOs
- do not silently expand the product

If unsure whether an old API should remain:
- prefer the unified public path
- retain old surface only if it still provides genuinely unique capability
- otherwise remove it
