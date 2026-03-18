# SPEC.md

This document is the implementation contract for the `tailscope` MVP.

It is written to be both:
- human-readable product documentation
- LLM-friendly execution guidance

## 1. Product summary

`tailscope` is a Rust toolkit for diagnosing tail-latency, queueing, and backpressure problems in Tokio services.

The MVP must answer:

> Given one instrumented run of a Tokio service, is the dominant problem application-level queueing, executor pressure, blocking-pool pressure, or a slow downstream stage?

## 2. Product goals

The MVP must:
1. be easy to integrate
2. work with partial instrumentation
3. produce a readable diagnosis report
4. be grounded in actual run data
5. measure runtime cost rather than guess it

## 3. Non-goals

The MVP must not attempt to be:
- a tracing backend
- a metrics backend
- a profiler
- a distributed tracing system
- an eBPF toolkit
- a distributed root-cause engine
- an auto-remediation system
- a general-purpose observability SaaS

## 4. Intended users

Primary users:
- developers working on Tokio-based services
- performance-oriented engineers
- developers debugging p95/p99 problems

Secondary users:
- engineers writing internal service frameworks
- benchmark authors

## 5. Supported environment

### MVP runtime
- Rust stable
- Tokio runtime

### Not supported in MVP
- non-Tokio runtimes
- cross-process distributed tracing correlation
- language interop

## 6. Workspace layout

Expected workspace members:
- `tailscope-core`
- `tailscope-tokio`
- `tailscope-cli`

Expected extra directories:
- `demos/`
- `benches/`
- `scripts/`
- `docs/`

## 7. Core user-facing API

## 7.1 Initialization

```rust
let _guard = Tailscope::init(
    Config::light()
        .service_name("invoice-api")
        .json_output("tailscope-run.json")
        .tokio_sampling_interval(Duration::from_secs(1)),
)?;
```

Required behavior:
- initializes local collection
- starts optional Tokio sampling if configured
- returns a guard whose lifetime keeps collection active

Constraints:
- should be simple
- should not require external infrastructure

## 7.2 Request-level instrumentation

```rust
#[instrument_request(route = "/invoice", kind = "create_invoice", skip(state, input))]
async fn create_invoice(...) -> ... { ... }
```

Required behavior:
- measure total function/request duration
- record success/failure outcome
- attach metadata fields
- create request-level scope/span context

Constraints:
- ergonomic
- async-friendly
- must not try to infer detailed stage semantics automatically

## 7.3 Manual request scope

```rust
let _req = tailscope::request("create_invoice")
    .field("route", "/invoice")
    .field("tenant", tenant_id);
```

Required behavior:
- create a request scope without macro usage
- support attaching fields

Constraints:
- cheap
- optional
- useful for middleware/job systems

## 7.4 In-flight tracking

```rust
let _inflight = tailscope::inflight("invoice_requests");
```

Required behavior:
- increment named in-flight counter on creation
- decrement on drop
- record enough information for later analysis

Constraints:
- RAII-based
- panic-safe / drop-safe as far as reasonable

## 7.5 Queue timing wrapper

```rust
let permit = tailscope::queue("invoice_worker")
    .await_on(state.invoice_sem.acquire())
    .await?;
```

Required behavior:
- measure wait time around one awaited operation
- classify it as queue wait
- preserve wrapped future result/error

Constraints:
- thin wrapper
- readable
- generic over awaitable result where practical

Why needed:
- a request macro alone cannot know that a given await is “waiting to start work.”
- This wrapper creates that semantic boundary explicitly.

## 7.6 Stage timing wrapper

```rust
let customer = tailscope::stage("fetch_customer")
    .await_on(state.customer_api.fetch(&input.customer_id))
    .await?;
```

Required behavior:
- measure duration around one awaited operation
- classify it as service-stage time
- preserve wrapped future result/error

Constraints:
- thin wrapper
- readable
- low-friction to use

Why needed:
- The tool needs stage-level attribution to distinguish “slow work” from “slow waiting.”

## 7.7 HTTP integration helper

```rust
let app = Router::new()
    .route("/invoice", post(create_invoice))
    .layer(tailscope::http_layer());
```

MVP status:
- Optional if time allows.
- Do not prioritize over core primitives.

If implemented:
- should provide request count, request duration, status, and route metadata
- should not become a full middleware framework

## 7.8 Tokio sampler
```rust
let handle = tokio::runtime::Handle::current();
tailscope::spawn_tokio_sampler(&handle, Duration::from_secs(1));
```

Required behavior:
- periodically capture runtime-level snapshots
- integrate into the same run output
- MVP metrics to capture where available
- alive task count
- global queue depth
- local queue depth
- blocking queue depth
- remote schedule count

Constraints:
- sampling, not exhaustive per-poll tracing
- low overhead in light mode

# 8. Internal data model

The exact JSON format may evolve, but the MVP must support these conceptual record types:

## 8.1 Run metadata

Fields:
- service name
- start timestamp
- end timestamp
- mode
- version
- optional git/revision metadata

## 8.2 Request record

Fields:
- request name
- start/end or elapsed
- success/failure
- metadata fields
- correlation id if available

## 8.3 Stage record

Fields:
- request correlation id if applicable
- stage name
- elapsed
- metadata fields if applicable

## 8.4 Queue record

Fields:
- request correlation id if applicable
- queue name
- elapsed
- metadata fields if applicable

## 8.5 In-flight sample or event

Fields:
- counter name
- timestamp
- count/value

## 8.6 Runtime snapshot

Fields:
- timestamp
- alive tasks
- global queue depth
- local queue depth if available
- blocking queue depth if available
- remote schedule count if available

# 9. Collection modes

The system must support conceptually distinct modes.

## 9.1 Off mode

Behavior:
- no meaningful collection
- effectively no output

## 9.2 Light mode

Purpose:
- always-on or benchmark-friendly low-overhead mode

Behavior:
- request timing
- selected stage/queue timing
- in-flight tracking
- periodic runtime sampling
- local aggregation/output

Constraints:
- should aim for low overhead
- should avoid dense per-event tracing if not needed

## 9.3 Investigation mode

Purpose:
- richer diagnosis during targeted analysis

Behavior:
- same as light mode plus denser detail as configured

Constraints:
- overhead may be higher
- should be used deliberately

# 10. Analyzer CLI

Command

> tailscope analyze <run.json>

Required outputs:
- human-readable report
- JSON report

Required computed values:
- p50/p95/p99 by stage
- p50/p95/p99 by queue
- total request percentiles
- queue share of total latency
- service-stage share of total latency
- runtime pressure summary
- diagnosis ranking

# 11. Diagnosis categories

MVP must implement these categories.

## 11.1 Application-level queue saturation

Evidence pattern:
- queue timings dominate service timings
- in-flight pressure rises with offered load
- runtime queue metrics are not the primary driver

## 11.2 Blocking-pool pressure

Evidence pattern:
- blocking queue signals elevated
- tails track blocking-related pressure
- async stage breakdown alone is insufficient

## 11.3 Executor pressure suspected

Evidence pattern:
- runtime scheduling/queue signals elevated broadly
- no single app queue dominates
- latency inflation is broad

## 11.4 Downstream stage dominates

Evidence pattern:
- one stage dominates p95/p99
- queue wait is secondary
- the same stage repeatedly leads the tail

## 11.5 Insufficient evidence

Evidence pattern:
- sparse instrumentation
- conflicting signals
- no strong ranking possible

## 12. Report structure

The human-readable report should contain:
- primary suspect
- confidence level
- supporting evidence
- optional secondary suspects
- recommended next checks

Example:

Primary suspect: application-level queue saturation
Confidence: medium-high
Evidence:
- queue(invoice_worker) p99 = 148 ms
- service(stage_total) p99 = 32 ms
- inflight(invoice_requests) rises with load
- runtime global queue depth stable

Recommended next checks:
- reduce queue capacity
- add shedding/timeouts
- review worker concurrency cap

# 13. Demo services

The MVP must include two small proof cases.

## 13.1 Queue/backpressure demo

Required behavior:
- bounded concurrency / worker acquisition
- load-induced queue growth
- queue wait becomes dominant under stress

Expected diagnosis:
- application-level queue saturation

## 13.2 Blocking contamination demo

Required behavior:
- blocking work path causes degradation
- blocking-related signals visible in run output

Expected diagnosis:
- blocking-pool pressure

## 14. Benchmarking and runtime cost

The repository must measure:
- baseline service behavior
- light mode behavior
- investigation mode behavior

Measure at minimum:
- throughput
- p50/p95/p99 latency
- relative overhead
- Do not publish unmeasured runtime-cost claims.

# 15. Testing requirements

## 15.1 Unit tests

Required for:
- event serialization
- core wrappers
- collector behavior
- diagnosis rules
- report formatting where structured

## 15.2 Fixture tests

Required for:
- analyzer on synthetic run inputs
- diagnosis ranking

## 15.3 Demo tests

Required for (at minimum):
- scripts or checks that the demos run and produce reports

# 16. Documentation requirements

The repository must include:
- README
- AGENTS
- PLANS
- IMPLEMENTATION_PLAN
- SPEC
- examples in docs or tests

If public APIs change:
- docs must change in the same PR

# 17. API ergonomics requirements

The API must remain easy to integrate.

Preferred experience:
- add one init call
- add one macro to request handlers
- wrap 2–4 important awaits
- run benchmark/load
- inspect report

Avoid designs that require:
- rewriting the service architecture
- adopting a custom executor
- mandatory external infrastructure
- manual timing boilerplate everywhere

# 18. Novelty and positioning

The project must be described honestly.

Allowed positioning:
- diagnosis layer over existing Tokio/tracing observability primitives
- easy-to-integrate async bottleneck diagnosis tool
- gap-filling tool between raw metrics/traces and actionable interpretation

Not allowed positioning:
- “first async observability tool for Rust”
- “replaces tracing”
- “replaces tokio-console”
- “solves root cause analysis”

# 19. Implementation priorities

Priority order:
1. core primitives
2. request macro
3. Tokio sampler
4. analyzer
5. demos
6. runtime-cost measurement
7. polish

# 20. Stop conditions

Stop scope expansion if:
- implementation starts to resemble a full observability platform
- macro work is dominating the schedule
- analyzer has too many vague categories
- runtime support beyond Tokio is being considered
- external backends are being introduced

Return to the MVP definition instead.

# 21. MVP acceptance criteria

The MVP is complete when:
- a small Tokio service can integrate the crate quickly
- request/stage/queue timing works
- runtime snapshots are collected
- the CLI emits a useful diagnosis
- both demo services are correctly diagnosed
- runtime cost is measured and documented
- docs are coherent and accurate
