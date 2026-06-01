# Production operations guide

This guide focuses on operating `tailtriage` in real services.

It is intentionally operational rather than API-centric.

`tailtriage` is a bounded tail-latency triage tool. It produces evidence-ranked suspects and next checks from one captured run. Suspects are triage leads, not proof of root cause.

This guide explains:

* when to enable capture
* how to roll out safely
* when to use light versus investigation capture
* when runtime sampling helps
* how to reason about artifact growth and truncation
* how to interpret weak or ambiguous output
* what the current operational limits and non-fits are

For API-level usage and request lifecycle contracts, see:

- [user guide](user-guide.md)
- [diagnostics guide](diagnostics.md)
- [controller README](../tailtriage-controller/README.md)
- [validation overview](../VALIDATION.md)

## Recommended rollout path

Use a staged rollout.

Do not begin with dense runtime sampling and maximum capture limits in production.

Recommended progression:

1. start with direct capture or controller-managed bounded windows
2. use `light` mode first
3. add queue and stage instrumentation around suspected waits
4. validate that artifacts analyze cleanly
5. enable runtime sampling only when request timing alone is insufficient
6. increase capture density only when the existing evidence is not enough

A conservative rollout usually gives better operational signal than enabling every feature immediately.

## Analyzer tuning in operations

Keep rollout conservative: prefer default analyzer behavior first and tune only after comparing representative runs for your workload profile.

Operational guardrails:

- Do not tune around missing instrumentation; add needed queue/stage/runtime evidence first.
- Do not use tuning to hide truncation or dropped-event warnings; address capture density/limits and re-run.
- Commit analyzer TOML used in production workflows so repeated runs are reproducible.
- Compare runs only when analyzer config is the same, or explicitly account for changed analyzer config when interpreting movement.
- Use tuning to improve workload fit of evidence interpretation after baseline runs, not as a substitute for capture quality.

## Choosing direct capture vs controller capture

### Direct capture

Use `Tailtriage` directly when:

* you want one explicit bounded run
* capture lifetime naturally matches process lifetime
* you are validating instrumentation locally or in staging
* you do not need repeated arm/disarm windows

This model is:

```text
build -> capture -> shutdown
```

### Controller capture

Use `TailtriageController` when:

* the service stays up continuously
* you need repeated bounded capture windows
* you want runtime arm/disarm control
* you want TOML-backed operational configuration
* you want future generations to pick up reloaded config

This model is:

```text
enable -> capture -> disable -> re-enable later
```

Controller capture is usually the better production operational model.

## Capture mode guidance

### `light`

Use `light` mode first.

Recommended for:

* initial production rollout
* lower-risk bounded captures
* validating instrumentation quality
* broad environment coverage
* services where artifact growth must stay conservative

Prefer `light` when:

* you are still deciding where instrumentation belongs
* you only need directional evidence
* you expect many repeated capture windows
* you are operating under tight retention constraints

### `investigation`

Use `investigation` mode when:

* a real tail-latency incident is active
* `light` mode produced ambiguous evidence
* runtime pressure needs deeper separation
* you need more complete stage/queue visibility
* you are intentionally running a denser bounded capture

`investigation` mode is not intended as a permanent always-on telemetry configuration.

## Runtime sampling guidance

Runtime sampling is optional enrichment.

It is most useful when request timing alone cannot clearly separate:

* application queue saturation
* executor pressure
* blocking-pool pressure

Runtime sampling is usually worth enabling when:

* executor pressure is suspected
* blocking-pool contention is suspected
* queue wait alone does not explain the tail
* request timing evidence is ambiguous
* the service already uses Tokio heavily

Runtime sampling is usually unnecessary when:

* downstream stage latency clearly dominates
* queue saturation is already obvious
* the run already produces strong evidence quality
* you only need high-level directional triage

Important operational constraints:

* runtime sampling must start inside an active Tokio runtime
* runtime snapshots are bounded by capture limits
* some runtime fields require `tokio_unstable`
* runtime sampling increases event volume and artifact growth

Start conservatively.

Prefer moderate intervals and bounded runs before increasing density.

## Operating with tracing-based runs

Tracing intake works best when request correlation is already reliable. Every request, stage, and queue span for one work item must carry the same `tt.request_id`; missing or inconsistent `tt.request_id` causes child stage/queue evidence to be skipped or weakened. Native capture is the recommended first path when correlation is not already available.

Tracing import expects completed tailtriage `tt.*` tracing span JSONL, not ordinary tracing log JSON (`fmt().json` output is a common non-supported example). Import writes Run JSON (not Report JSON), and analysis is a separate step after import (`tailtriage analyze`). Completed-span JSONL is not a production trace archive and does not preserve warning/truncation context; prefer Run JSON when the artifact itself must carry that context. Persisted Run JSON intended for `tailtriage analyze` must include at least one completed request event; in-process library snapshots may still be zero-request for inspection. Timing is not guessed from line receive time, so completed spans must include explicit unix-ms start/end timestamps. OTel/OTLP intake remains out of scope on this path.

For live tracing sessions, `tt.*` fields must be declared when the span is created. If a value is filled later, declare it with `tracing::field::Empty` and then call `span.record(...)`; adding brand-new `tt.*` fields later is not supported.

Important limits for production interpretation:

* tracing-only runs do not fabricate runtime snapshots
* without runtime snapshots, executor-pressure and blocking-pool suspects can be weaker or absent
* runtime-pressure evidence remains Tokio-specific and requires runtime snapshots or Tokio sampler coupling

`TracingTokioSession` uses the same core capture-limit model as native Tokio sampling for runtime snapshot retention. For `TracingTokioSession`, run metadata time bounds cover both retained tracing evidence and retained runtime snapshots. There is no tracing-specific `max_runtime_snapshots(...)` builder method; configure explicit caps with `capture_limits_override(CaptureLimitsOverride { max_runtime_snapshots: Some(...), ..Default::default() })`. Tracing-only runs still do not fabricate runtime snapshots. `TracingTokioSession` starts background sampling by default, but deterministic/manual runtime-sensitive workflows can call `disable_background_sampler()` and inject snapshots via `record_runtime_snapshot(...)`; runtime-sensitive tracing contract parity requires non-empty runtime snapshots, scenario-specific runtime field evidence, and the explicit disabled-background-sampler lifecycle warning (not ambient sampler metadata/noise). These are repeatable triage leads, not root-cause proof.

Treat tracing-based reports the same way as other reports: evidence-ranked suspects and next checks are triage leads, not proof.

## Artifact sizing and retention expectations

Artifact size depends on:

* request count
* queue event count
* stage event count
* runtime snapshot density
* in-flight snapshot density
* capture duration
* truncation state

Artifact growth is workload-shaped and machine-scoped.

The repository intentionally does not claim universal production artifact sizing.

Use:

* [runtime cost measurement](runtime-cost.md)
* [collector limits and stress guidance](collector-limits.md)
* [`scripts/measure_collector_limits.py`](../scripts/measure_collector_limits.py)

when establishing local operational expectations.

## Capture limits and truncation

Capture limits are expected operational controls, not exceptional failures.

When limits are hit:

* retained data becomes partial
* dropped counters become non-zero
* evidence quality can downgrade
* warnings can appear
* interpretation confidence should become more conservative

Treat truncation as a signal that:

* the capture window was too dense
* the run duration was too large
* limits were too small for the workload
* runtime sampling density may be too aggressive

Do not treat truncation as proof the analyzer is wrong.

Instead:

1. inspect dropped counters
2. inspect warnings
3. reduce capture scope or increase limits
4. rerun under comparable load

For controller-managed runs, consider:

* `continue_after_limits_hit`
* `auto_seal_on_limits_hit`

based on whether bounded retention or uninterrupted capture matters more operationally.

## Operational guidance for bounded runs

Prefer bounded investigative windows over continuous long-lived capture.

Good operational patterns:

* arm during a suspected incident window
* collect enough traffic to produce stable evidence
* disarm and analyze
* compare before/after mitigation runs
* rerun with one changed variable

Avoid:

* indefinite always-recording operation
* continuously increasing limits without understanding growth
* treating one run as causal proof
* enabling every instrumentation surface immediately

## How to interpret common output patterns

### `application_queue_saturation`

Usually indicates:

* queue residence time dominates tail latency
* work is delayed before execution begins
* admission pressure or producer burst behavior is likely relevant

Next checks often include:

* queue limits
* concurrency limits
* worker availability
* producer burst patterns
* request fan-in

### `blocking_pool_pressure`

Usually indicates:

* `spawn_blocking` backlog pressure
* blocking work saturation
* blocking pool queue growth

Next checks often include:

* blocking pool sizing
* synchronous I/O paths
* CPU-heavy blocking sections
* accidental blocking in async code

### `executor_pressure_suspected`

Usually indicates:

* runtime scheduling contention
* runnable-task pressure
* executor queue growth

Next checks often include:

* task fan-out
* task explosion
* runtime saturation
* busy-loop or starvation behavior
* over-fragmented async work

### `downstream_stage_dominates`

Usually indicates:

* one downstream stage materially dominates request latency
* queue pressure is not the strongest lead in that run

Next checks often include:

* database latency
* external API latency
* cache misses
* retry amplification
* downstream concurrency constraints

### `insufficient_evidence`

This usually means the run lacks enough explanatory signal.

It does not necessarily mean nothing is wrong.

Most common causes:

* too little instrumentation
* missing queue wrappers
* missing stage wrappers
* insufficient runtime visibility
* very small request sample count
* heavily truncated capture

Recommended progression:

1. add queue instrumentation around waits
2. add stage instrumentation around downstream work
3. optionally add runtime sampling
4. rerun under comparable load

## Evidence quality and operational trust

Use `evidence_quality` as an operational interpretation boundary.

### `strong`

Usually means:

* enough requests were captured
* important evidence families are present
* truncation is not active

This supports stronger next-check confidence.

### `partial`

Usually means:

* some evidence families are missing
* truncation occurred
* runtime visibility is incomplete
* interpretation limits are material

Treat conclusions more conservatively.

### `weak`

Usually means:

* request evidence is sparse
* critical evidence families are absent
* request retention was truncated heavily

Use the run mainly to decide what instrumentation or capture shape to improve next.

## Operational troubleshooting

### Analyzer output feels ambiguous

Most common causes:

* multiple bottleneck families overlap
* runtime evidence is incomplete
* queue/stage instrumentation coverage is sparse
* the workload is phase-changing during capture

Recommended actions:

* add one more instrumentation surface
* shorten the capture window
* compare multiple bounded runs
* rerun after one targeted mitigation

### Artifacts are too large

Reduce:

* runtime sampling density
* capture duration
* request volume per run
* unnecessary instrumentation breadth

Or:

* lower capture concurrency
* split captures into smaller bounded windows
* use controller-managed operational windows

### Runtime sampling overwhelms the run

Use:

* longer sample intervals
* lower runtime snapshot limits
* shorter capture windows
* `light` mode instead of `investigation`

### Strict lifecycle shutdown fails

This usually means requests were started but not completed.

Common causes:

* missing completion calls
* early returns
* canceled tasks
* dropped completion handles

Use stricter request lifecycle review before increasing capture density.

## Operational validation workflow

The repository includes local operational validation paths.

Use these when evaluating:

* runtime overhead
* collector stress behavior
* truncation onset
* artifact growth
* memory trends

Primary references:

* [validation overview](../VALIDATION.md)
* [runtime cost measurement](runtime-cost.md)
* [collector limits and stress guidance](collector-limits.md)
* [`scripts/run_operational_validation.py`](../scripts/run_operational_validation.py)
* [`scripts/measure_collector_limits.py`](../scripts/measure_collector_limits.py)

These measurements are:

* synthetic
* workload-scoped
* machine-scoped
* intentionally conservative

They are not universal production guarantees.

## Current known limits and non-fits

`tailtriage` is intentionally not:

* a distributed tracing backend
* a metrics platform
* a permanent telemetry pipeline
* a root-cause proof engine
* a replacement for profiling
* a replacement for `tokio-console`
* a universal observability system

Current operational limits include:

* runtime sampling density can materially increase event volume
* truncation can reduce evidence quality under heavy load
* runtime-field visibility varies depending on Tokio capabilities
* diagnosis quality depends heavily on instrumentation quality
* one run provides bounded triage guidance, not certainty
* repeated comparative runs are often more useful than one dense run

## Recommended operational workflow

A practical production loop:

1. identify a slow window
2. arm a bounded capture
3. collect one representative run
4. analyze the report
5. choose one next check
6. apply one targeted mitigation or instrumentation improvement
7. rerun under comparable load
8. compare suspect movement and p95 share movement

Treat the workflow as iterative triage.

Do not treat one report as final proof.


## Tracing operations cross-reference

For tracing import and tracing-session operations guidance, see the canonical section above: [Operating with tracing-based runs](#operating-with-tracing-based-runs).
