# AGENTS.md

This file tells coding agents how to work in this repository.

## Mission

Maintain and improve `tailtriage`, a focused Rust toolkit for **Tokio tail-latency triage**.

The MVP is implemented. The current phase is to validate whether the tool is useful in real usage, learn from feedback, and improve it without losing cohesion or drifting into a different product.

The repository exists to produce a **real developer tool** with a narrow purpose, not a toy lab and not a generic observability platform.

## Product definition

`tailtriage` should help answer:

> Is this async Rust service slow because of application-level queueing, executor pressure, blocking-pool pressure, or a slow downstream stage?

The tool should:

- be easy to integrate
- be useful with partial instrumentation
- be low-overhead in light mode
- produce a clear triage report with evidence-ranked suspects and next checks
- support a practical capture -> analyze -> next check -> re-run workflow

## Post-MVP operating mode

The MVP is done.

The main question now is not “what else can we add?” The main question is “does this survive real-world usage and help real Tokio developers?”

In this phase:

- prioritize real user pain over speculative expansion
- keep scope narrow and coherent
- improve adoption only where evidence justifies it
- prefer a tighter product over a broader one
- keep docs, demos, examples, and tests aligned with the actual product

## Product language and positioning guardrails

When editing docs, prefer language that reinforces the product category:

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

## Documentation contract

Docs in `docs/` are user-facing product documentation.

- `docs/` pages are for users of `tailtriage`, not repository development workflow.
- Do not write contributor-process narration, issue-history context, or roadmap-history wording in `docs/` pages.
- Keep docs crisp, truthful, present-tense, and aligned with the current product surface.
- Do not claim behavior that is not supported by the code and current public docs.
- Keep `docs/README.md` as a complete user-journey index to current docs.
- Treat `scripts/validate_docs_contracts.py` and related tests as part of the public documentation contract.
- If docs structure, required docs links, or enforced public-doc wording changes, update the docs contract validator and related tests in the same change set.
- Only change docs contract validation when the new docs are more truthful to the code or when the intended public docs contract has actually changed.
- Do not weaken or bypass docs contract validation just to make CI pass.
- If a docs change breaks the docs contract validator, either:
  - update the docs to satisfy the existing contract, or
  - update the validator and related tests so they enforce the new truthful contract.
- When changing the validator, keep checks aligned with code truth and current public guidance, not stale phrasing.


## Validation documentation contract

Validation docs are part of the public trust surface.

When editing validation-related files, preserve these rules:

- Describe `tailtriage` as a triage tool, not a root-cause proof engine.
- Keep validation claims bounded to the evidence actually produced.
- Do not use PR-history wording such as “this PR introduces” in stable public docs.
- Do not call a validation path a CI gate unless CI actually runs it.
- Distinguish deterministic fixture validation, repeated-run validation, mitigation validation, runtime-cost validation, collector-limit validation, and real-service validation.
- State whether each validation path is mandatory CI, manual/local, release-only, or planned.
- Treat generated runtime-cost, collector-limit, repeated-run, and mitigation outputs as machine/workload/profile scoped unless explicitly proven otherwise.
- Never present runtime overhead measurements as universal production guarantees.
- Never present collector-limit validation as “no drops”; it validates visible bounded drops, warnings, and downgrade behavior.
- Never present mitigation movement as formal causal proof; it supports the capture -> analyze -> next check -> re-run workflow.

If validation corpus schema or benchmark semantics change, update together:

- `VALIDATION.md`
- `docs/diagnostic-validation.md`
- `validation/diagnostics/README.md`
- `validation/diagnostics/latest/scorecard.md`
- relevant scripts and tests
- docs contract validation, if public docs requirements changed

Validation scorecards should distinguish:

- covered area
- latest run status
- generated metrics, if available
- manual/local versus CI status
- known non-claims

## Scope guardrails

Only expand scope when at least one of these is true:

1. the missing piece is clearly holding the MVP back in real usage
2. the change provides a clearly high-leverage boost to usefulness, clarity, or adoption without changing the product category
3. the change fixes a severe correctness, reliability, or security problem that would materially undermine trust in the tool if left unresolved

Do not accept work just because it is adjacent, interesting, or requested once.

Do not require repeated requests before acting on a credible, reproducible severe bug.

When reviewing or implementing issues and PRs:

- keep changes within reasonable bounds
- keep related changes together cleanly
- reject scattered additions that pull the project sideways
- preserve one coherent product story across code, docs, demos, examples, and tests

## What this repository is NOT building

Do not add these unless explicitly approved by maintainers and clearly justified against the implementation plan:

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
- general observability-platform features

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
- deterministic demos and fixtures
- simple examples that teach the intended workflow

Avoid:

- macro-heavy magic beyond what is strictly needed
- unnecessary async trait abstraction
- clever but opaque code
- premature optimization
- giant generic frameworks
- dependency growth without strong justification

## Dependency policy

Be conservative when adding dependencies.

Before adding a new dependency, ask:

1. is it necessary for the current scoped problem?
2. is there a simpler way to solve this with existing dependencies or std?
3. is the dependency too heavy relative to the benefit it brings?
4. does it increase compile time, binary size, maintenance burden, or conceptual weight too much?
5. is the license permissive and acceptable for this repository?

Only add a dependency when the value clearly outweighs the cost.

Prefer:

- small, well-maintained crates
- widely used crates with clear ownership and stable maintenance
- permissive licenses such as MIT, Apache-2.0, or BSD-style licenses

Avoid or reject dependencies that are:

- heavy for a narrow benefit
- weakly maintained
- unclear in license status
- copyleft or otherwise not permissive unless explicitly approved
- redundant with existing crates already in the workspace

If a new dependency is added, document briefly in the PR or task summary:

- why it is needed
- why lighter alternatives were not used
- what its license is

## Workspace structure

Expected workspace members:

- `tailtriage`
- `tailtriage-core`
- `tailtriage-controller`
- `tailtriage-tokio`
- `tailtriage-axum`
- `tailtriage-analyzer`
- `tailtriage-cli`

Possible directories:

- `demos/`
- `examples/`
- `benches/`
- `scripts/`
- `docs/`

If an extra crate or layer becomes unnecessary, remove it instead of preserving extra surface area.

## API design rules

The repository should keep **one coherent public usage story**.

That usage story should have:

- one builder/setup path
- one request-context model
- explicit queue/stage/inflight instrumentation on that request context
- one lifecycle completion path
- progressive disclosure for advanced tuning on the same conceptual surface

Do not introduce a second competing onboarding path unless maintainers explicitly approve it in an accepted issue, PR plan, or implementation-plan update, and the reason is strong.

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

Do not implement those features unless maintainers explicitly approve them.

## Build and test requirements

Before considering a task done, run:

- `cargo fmt --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `python3 scripts/validate_docs_contracts.py`

If a task touches benchmarks or performance-sensitive code, also include:

- benchmark notes
- before/after evidence if behavior changed materially

If a task changes public docs, crate READMEs, rustdoc, docs structure, or documentation contract wording:

- docs contract validation must pass
- any required updates to `scripts/validate_docs_contracts.py` and related tests must be included in the same change set
- docs contract changes must be justified by code truth and intended public behavior

## Definition of done

A task is done only if:

1. it satisfies the linked issue or task scope
2. code builds
3. tests pass
4. docs/comments are updated where needed
5. public API changes are reflected in public-facing docs
6. docs contract validation passes when docs or public guidance changed
7. examples are updated where needed
8. demos are updated where needed
9. scope did not quietly expand

For public ergonomics or behavior changes, work is **not** done until the teaching surface moves too:

- `README.md`
- `docs/user-guide.md`
- relevant crate docs/readmes
- `examples/`
- `demos/`
- relevant tests and fixtures

## Documentation, demos, examples, and tests are first-class

Keep the teaching and validation surface focused and current.

### Docs

Docs should:

- teach the actual current workflow
- reinforce the narrow product definition
- avoid stale roadmap language
- avoid teaching multiple conflicting onboarding paths
- stay aligned with the implementation plan

### Demos

Demos are proof cases for the tool, not the product itself.

Keep demos:

- small
- deterministic
- runnable from scripts
- honest about what they do and do not prove
- focused on core tail-triage cases rather than side ambitions

Do not let demos become a playground for unrelated features.

### Examples

Examples should:

- be minimal
- teach the intended integration path
- help first-time users reach value quickly
- stay close to realistic usage without becoming a framework of their own

Do not keep stale examples that teach superseded APIs or broaden the story unnecessarily.

### Tests

Prefer:

- focused unit tests
- fixture-based tests for report generation
- deterministic test inputs
- tests that protect the narrow behavior contract

For analyzer logic:

- use explicit sample inputs
- test diagnosis ranking and evidence generation
- avoid brittle string matching unless intended output format is part of the contract

For public API ergonomics:

- test compact usage
- test fractured-code usage
- test parity of important behavior after API migration
- test advanced knobs on the same unified surface

## Performance claims

Do not make unmeasured performance claims.

If the code changes runtime cost or diagnosis behavior:

- add or update a benchmark, fixture, or comparison artifact
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

## Documentation expectations

If behavior, scope, or public guidance changes, update as needed:

- `README.md`
- `SPEC.md`
- `IMPLEMENTATION_PLAN.md` if milestones or operating mode changed
- `docs/user-guide.md`
- relevant crate docs/readmes
- relevant demos/examples/tests

Public docs should teach the intended current path first.

Changes to public docs are not complete until the docs contract validator passes, and any validator changes are justified by code truth.

Do not leave the repository teaching multiple competing onboarding stories after a change.

## How to approach tasks

When given a task:

1. read the issue/task carefully
2. inspect `README.md`, `SPEC.md`, `IMPLEMENTATION_PLAN.md`, and this file
3. make the smallest reasonable change that solves the actual problem
4. add or update tests
5. update docs/examples/demos where needed
6. run format/lint/test
7. run docs contract validation when docs or public guidance changed
8. summarize what changed and any remaining limitations

For larger tasks:

- propose a short plan first
- keep PRs reviewable
- do not batch unrelated work

## Preferred implementation order

For most scoped work, prefer this order:

1. repository guidance if scope/rules changed
2. core implementation
3. integration alignment
4. tests and fixtures
5. examples
6. demos
7. docs/readmes
8. cleanup/removal of superseded surface area

## Public API stability

Post-MVP:

- prefer correctness, clarity, and cohesion over accumulating compatibility baggage
- do not churn names casually
- do not preserve overlapping surfaces without a good reason

If changing public APIs:

- update examples and docs in the same change set
- remove or clearly justify retained overlapping surfaces
- do not leave both old and new paths equally endorsed

## If uncertain

If unsure whether a change belongs:

- default to the smaller scope
- default to the more cohesive option
- check whether it aligns with the implementation plan
- do not silently expand the product

If unsure whether a dependency belongs:

- default to not adding it until its value is clear
- prefer lighter and more permissively licensed options
- document the trade-off explicitly

If unsure whether a doc/demo/example/test should be updated:

- assume yes if user-facing behavior, guidance, or product understanding changed
