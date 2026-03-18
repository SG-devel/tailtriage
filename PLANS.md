# PLANS.md

This file defines the execution plan for the `tailscope` MVP.

## Objective

Deliver a working MVP of `tailscope` that can:
1. instrument a Tokio service with low effort
2. collect request/stage/queue timings
3. sample Tokio runtime metrics
4. analyze one run and emit a ranked diagnosis
5. prove usefulness on two small demo services

## Core product promise

> Add one macro for request-level visibility. Wrap a few important awaits. Get a report telling you whether your tails are dominated by queueing, blocking, executor pressure, or a slow downstream stage.

## MVP scope

### In scope
- Tokio-only
- local JSON run output
- request macro
- stage and queue wrappers
- in-flight guard
- runtime sampler
- analyzer CLI
- two demo services
- benchmark scripts
- docs

### Out of scope
- tracing backend
- metrics backend
- distributed tracing
- live UI
- OpenTelemetry
- Prometheus
- eBPF
- auto-remediation
- ML diagnosis
- multi-service diagnosis

## Milestones

## M0 — Repository bootstrap
Goal:
- workspace exists
- CI exists
- docs define the product clearly

Deliverables:
- workspace skeleton
- fmt/clippy/test GitHub Actions
- README
- AGENTS
- plans/spec docs

Exit criteria:
- repo builds
- CI passes
- docs are coherent

## M1 — `tailscope-core`
Goal:
- core instrumentation primitives exist

Deliverables:
- `Tailscope::init`
- `Config`
- `request(...)`
- `inflight(...)`
- `queue(...).await_on(...)`
- `stage(...).await_on(...)`
- local JSON sink/collector

Exit criteria:
- example code compiles
- unit tests pass
- basic event aggregation works

## M2 — request macro
Goal:
- easiest integration path exists

Deliverables:
- `#[instrument_request(...)]`
- minimal metadata support
- skip fields support or equivalent
- docs/examples

Exit criteria:
- one handler can be instrumented with macro only
- request timing appears in output

## M3 — `tailscope-tokio`
Goal:
- runtime context is available

Deliverables:
- Tokio runtime sampler
- periodic snapshots
- queue depth / alive tasks / blocking metrics where available
- JSON export integration

Exit criteria:
- runtime snapshots collected during a run
- tests validate snapshot serialization and shape

## M4 — `tailscope-cli`
Goal:
- turn collected data into a diagnosis report

Deliverables:
- `tailscope analyze`
- p50/p95/p99 computation
- queue/service share computation
- initial diagnosis rules
- text + JSON output

Initial diagnosis families:
- application-level queue saturation
- blocking-pool pressure
- executor pressure suspected
- downstream stage dominates
- insufficient evidence

Exit criteria:
- CLI works on fixture data
- diagnosis tests pass

## M5 — demo services
Goal:
- prove the tool finds real pathologies

Deliverables:
- queue/backpressure demo
- blocking contamination demo
- load scripts
- sample outputs

Exit criteria:
- each demo produces a useful diagnosis
- at least one fix improves behavior

## M6 — runtime cost and polish
Goal:
- make the MVP presentable and honest

Deliverables:
- light mode overhead measurements
- investigation mode overhead measurements
- docs on cost and trade-offs
- cleaned README/examples

Exit criteria:
- runtime cost documented with measured evidence
- docs ready for external readers

## Guiding constraints

- Small PRs
- No casual scope expansion
- Keep the integration ergonomic
- Prefer explicitness over magic
- Use existing ecosystem primitives where sensible
- Measure before making performance claims

## Diagnosis philosophy

The analyzer should output:
- one primary suspect
- optional secondary suspects
- supporting evidence
- recommended next checks

The analyzer should not output:
- fake certainty
- causal claims
- overconfident root-cause declarations

## Minimum acceptance for MVP

The MVP is successful if:

1. a developer can integrate it into a small Tokio service quickly
2. the queue/backpressure demo is correctly diagnosed
3. the blocking contamination demo is correctly diagnosed
4. the report is readable and useful
5. light mode has acceptable measured overhead
6. the docs make the integration and limits clear

## Stretch goals after MVP

Only after MVP is solid:
- HTTP layer integration helpers
- richer labels/fields
- optional sampled tracing mode
- nicer report formatting
- improved rule ranking
- additional demo scenarios
