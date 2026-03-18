# IMPLEMENTATION_PLAN.md

This file is the detailed build plan for the `tailscope` MVP.

## Summary

Estimated effort:
- focused full-time: 9–15 working days
- realistic remote GitHub/Codex workflow: 3–6 calendar weeks part-time

The project should be built as a sequence of small, reviewable milestones.

## Phase 0 — bootstrap

### Goals
- create workspace
- define docs
- set up CI
- make first trivial green build

### Tasks
1. create Cargo workspace
2. add crates:
   - `tailscope-core`
   - `tailscope-tokio`
   - `tailscope-cli`
3. add GitHub Actions:
   - fmt
   - clippy
   - test
4. add placeholder docs
5. add one smoke test

### Deliverables
- working workspace
- passing CI
- docs in place

### Estimated effort
0.5–1 day

---

## Phase 1 — core data model and collection

### Goals
Define the internal event model and local collection strategy.

### Design choices
Use a local in-process collector with JSON output for v1.

Avoid:
- network exporters
- external metrics backends
- complex aggregation layers

### Proposed internal concepts
- request start/end records
- stage timing records
- queue timing records
- in-flight gauge updates
- runtime snapshots
- run metadata

### Tasks
1. define event/data structs
2. define run metadata format
3. define collector trait or internal sink abstraction
4. implement local JSON sink
5. add serialization tests

### Deliverables
- stable internal JSON schema for v1
- unit tests for serialization

### Estimated effort
1–2 days

---

## Phase 2 — `tailscope-core`

### Goals
Implement the user-facing instrumentation primitives.

### Public API to implement
- `Tailscope::init`
- `Config`
- `request(...)`
- `inflight(...)`
- `queue(...).await_on(...)`
- `stage(...).await_on(...)`

### Important design decisions
- prefer RAII guards
- wrappers should be thin and readable
- avoid forcing users into a custom framework
- support partial instrumentation

### Suggested modules
- `config`
- `collector`
- `request`
- `inflight`
- `stage`
- `queue`
- `events`
- `output`

### Tasks
1. implement config and init guard
2. implement request scope object
3. implement in-flight guard
4. implement stage wrapper
5. implement queue wrapper
6. connect all primitives to collector
7. add example-based unit tests

### Deliverables
- core instrumentation works
- docs examples compile
- unit tests pass

### Estimated effort
2–3 days

---

## Phase 3 — request macro

### Goals
Provide the easiest possible integration path.

### API
- `#[instrument_request(...)]`

### Approach
Start with a simple proc macro crate only if necessary.
If proc-macro complexity becomes a time sink, consider a first version built on top of `tracing` + explicit request scope helpers, but the end goal remains the request macro.

### Macro responsibilities
- create request scope/span
- record top-level metadata
- measure total function duration
- record result status
- honor skipped parameters

### Non-responsibilities
- infer queue semantics
- infer stage meanings
- instrument every await automatically

### Tasks
1. choose proc-macro crate layout
2. parse simple attribute arguments
3. wrap async fn body
4. connect to core collector
5. test with async examples

### Deliverables
- one realistic handler instrumented by macro
- docs example passes

### Estimated effort
1–2 days

---

## Phase 4 — `tailscope-tokio`

### Goals
Add runtime-level context.

### Runtime signals to capture
Target the stable, useful subset first:
- alive tasks
- global queue depth
- local queue depth if available
- blocking queue depth if available
- remote scheduling count if available

### Design choices
- sample periodically
- write snapshots into the same run output
- do not try to capture every poll event
- keep collection cheap in light mode

### Suggested modules
- `sampler`
- `snapshot`
- `runtime_metrics`
- `serialization`

### Tasks
1. implement sampler loop
2. capture runtime snapshots
3. integrate with config and collector
4. test snapshot collection
5. document platform/runtime caveats if needed

### Deliverables
- periodic runtime snapshots in run output
- sampler start/stop lifecycle works

### Estimated effort
2–3 days

---

## Phase 5 — `tailscope-cli`

### Goals
Turn one run into a diagnosis report.

### Commands for MVP
- `tailscope analyze <run.json>`

Optional later:
- `tailscope summarize`
- `tailscope explain`

### Core report calculations
- per-stage count
- p50/p95/p99 by stage
- queue wait share vs total time
- service time share vs total time
- in-flight trends
- runtime metrics trends

### Diagnosis rules for MVP

#### Rule A — application queue saturation
Conditions:
- queue wait dominates service time
- queue-related timing rises strongly under load
- app in-flight rises
- runtime global queue pressure is not the primary mover

#### Rule B — blocking-pool pressure
Conditions:
- blocking queue depth elevated
- long tails correlate with blocking pressure
- async stage timings alone do not explain the tail fully

#### Rule C — executor pressure suspected
Conditions:
- runtime/global scheduling pressure elevated
- no single app-level queue dominates
- broad latency inflation across stages

#### Rule D — downstream stage dominates
Conditions:
- one stage dominates p95/p99
- queue wait is secondary
- tails correlate with that stage

#### Rule E — insufficient evidence
Conditions:
- data too sparse
- instrumentation too incomplete
- conflicting signals

### Output formats
- human-readable text
- structured JSON

### Tasks
1. define report structs
2. implement percentile computations
3. implement diagnosis rules
4. implement text renderer
5. implement JSON renderer
6. add fixture tests

### Deliverables
- diagnosis output from fixture data
- tested rule ranking

### Estimated effort
2–4 days

---

## Phase 6 — demo services

### Demo A — queue/backpressure service
Purpose:
- prove queue diagnosis works

Behavior:
- bounded concurrency
- queue/permit acquisition
- downstream async work with controllable latency

Expected diagnosis:
- application-level queue saturation

Possible fix:
- reduce queue growth
- tune concurrency
- add shedding / timeout

### Demo B — blocking contamination service
Purpose:
- prove blocking diagnosis works

Behavior:
- too much work on blocking pool or equivalent bad pattern

Expected diagnosis:
- blocking-pool pressure

Possible fix:
- remove or reduce blocking work
- isolate blocking work
- change concurrency policy

### Tasks
1. build minimal services
2. add simple load generators or scripts
3. capture run artifacts
4. validate analyzer output
5. implement one fix per demo

### Deliverables
- reproducible demo scripts
- sample before/after outputs

### Estimated effort
2–3 days

---

## Phase 7 — runtime cost evaluation

### Goals
Measure and document overhead honestly.

### Modes to measure
- off
- light
- investigation

### Metrics
- throughput
- p50/p95/p99 latency
- CPU time if feasible
- relative overhead vs baseline

### Design target
- light mode should be low single-digit overhead if practical
- investigation mode may cost more and that is acceptable

### Tasks
1. define benchmark scenario
2. run baseline
3. run light mode
4. run investigation mode
5. summarize results in docs

### Deliverables
- measured overhead section
- benchmark script outputs

### Estimated effort
1–2 days

---

## Phase 8 — polish and sample-quality documentation

### Goals
Make the repository strong enough to function as:
- a work sample
- an analysis sample
- a writing sample

### Tasks
1. improve README
2. refine examples
3. document limitations honestly
4. add architecture docs
5. write a memo-style narrative
6. ensure issue/PR history is coherent

### Deliverables
- polished docs
- coherent repo story

### Estimated effort
1–2 days

---

## Recommended issue breakdown

Suggested first 12 issues:

1. bootstrap workspace and CI
2. define run/event JSON schema
3. implement config + init
4. implement request scope
5. implement in-flight guard
6. implement stage wrapper
7. implement queue wrapper
8. add request macro
9. add Tokio runtime sampler
10. build CLI report skeleton
11. implement queue saturation diagnosis rule
12. implement blocking pressure diagnosis rule

Then:
13. queue demo
14. blocking demo
15. overhead measurement
16. docs polish

---

## Risk management

### Biggest risks
1. macro complexity grows too much
2. runtime metrics API differences complicate sampler
3. analyzer becomes vague instead of useful
4. scope creep into a full observability platform

### Responses
- keep macro MVP simple
- start with minimal stable runtime signals
- prefer few explicit rules over a grand diagnosis engine
- enforce non-goals aggressively

---

## Success criteria

The implementation is successful if:
- the API is easy to integrate
- the demos are correctly diagnosed
- the CLI output is useful to a developer
- the overhead is measured
- the repository reads like a coherent product, not a pile of experiments
