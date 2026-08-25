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
- [validation overview](dev/VALIDATION.md)

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
* Tokio sampling records optional `worker_count` evidence directly from the runtime; current-thread runtimes report one
* some runtime fields require `tokio_unstable`
* runtime sampling increases event volume and artifact growth

Start conservatively.

Worker count remains optional in schema-v2 artifacts, so older artifacts may omit
it. A zero value is invalid: strict validation rejects it, while permissive
normalization clears only the invalid field and retains the runtime snapshot and
typed validation finding. Complete, consistent, positive worker-count evidence
enables per-worker runnable-queue scoring. Historical absence preserves legacy
absolute-depth scoring; partial, inconsistent, or invalid evidence uses that
fallback without inventing a worker count and limits confidence as documented.
When local queue depth is missing, normalized runnable-queue evidence is a lower
bound. See the [executor-pressure reference](diagnostics.md#executor-pressure)
for the scoring and confidence details.

Prefer moderate intervals and bounded runs before increasing density.

## Operating with tracing-based runs

Tracing intake works best when request correlation is already reliable and can be mapped to unique tailtriage request IDs. Every request, stage, and queue span for one completed logical request/work item must carry the same `tt.request_id`, and that ID must be unique among completed requests in one Run. External trace IDs that repeat across retries, fanout branches, batch items, or attempts should be expanded with attempt/span/branch/item information before becoming `tt.request_id`. Missing, inconsistent, or duplicated IDs cause child evidence to be skipped, weakened, or reported as ambiguous. Native capture is the recommended first path when correlation is not already available. Users remain responsible for meaningful instrumentation and request-boundary semantics.

Tracing import expects completed tailtriage `tt.*` tracing span JSONL, not ordinary tracing log JSON (`fmt().json` output is a common non-supported example). Import writes Run JSON (not Report JSON), and analysis is a separate step after import (`tailtriage analyze`). Tracing-specific source parsing and retention happen before core normalization, and private provenance joins retained core evidence back to original source records. Completed-span JSONL contains only retained original source records, preserves source identity and fields, and replays equivalently only for normalized request/stage/queue evidence that JSONL can represent. Direct and JSONL imports preserve supplied source order; live output is section-grouped as requests, then stages, then queues, preserving recorder order within each section. Completed-span JSONL is not a production trace archive and does not preserve Run-only metadata, runtime/in-flight snapshots, lifecycle warnings, semantic truncation counters, raw-recorder drop counters, source file/line context, omitted-source diagnostics, or output-path failures; prefer Run JSON when the artifact itself must carry that complete persisted triage context. Configured Run JSON and completed-span JSONL outputs are independent file transactions. Persisted Run JSON intended for `tailtriage analyze` must include at least one completed request event; in-process library snapshots may still be zero-request for inspection. Timing is not guessed from line receive time, so completed spans must include explicit Unix-ms start/end timestamps; complete run-relative monotonic offsets are optional and, when present, are preferred for elapsed-duration derivation and validation. OTel/OTLP intake remains out of scope on this path.

Live tracing intake only tracks spans that are tailtriage candidates at span creation time. Declare `tt.*` fields when the span is created. If a value is filled later, declare it with `tracing::field::Empty` and then call `span.record(...)`; adding brand-new `tt.*` fields later with `span.record(...)` is not supported.

Important limits for production interpretation:

* tracing-only runs do not fabricate runtime snapshots
* without runtime snapshots, executor-pressure and blocking-pool suspects can be weaker or absent
* runtime-pressure evidence remains Tokio-specific and requires runtime snapshots or Tokio sampler coupling

`TracingSession` uses the same core capture-limit model as native Tokio sampling for runtime snapshot retention. For `TracingSession`, run metadata time bounds cover both retained tracing evidence and retained runtime snapshots. There is no tracing-specific `max_runtime_snapshots(...)` builder method; configure explicit caps with `capture_limits_override(CaptureLimitsOverride { max_runtime_snapshots: Some(...), ..Default::default() })`. Tracing-only runs still do not fabricate runtime snapshots. `TracingSession` starts background sampling when configured with `sampler_interval(...)`; deterministic/manual runtime-sensitive workflows can call `manual_runtime_snapshots()` and inject snapshots via `record_runtime_snapshot(...)`; runtime-sensitive tracing contract parity requires non-empty runtime snapshots, scenario-specific runtime field evidence, and the explicit manual-runtime lifecycle warning (not ambient sampler metadata/noise). These are repeatable triage leads, not root-cause proof.

Treat tracing-based reports the same way as other reports: evidence-ranked suspects and next checks are triage leads, not proof.

## Current artifact and analyzer contracts

Run JSON schema version 2 is the current Run JSON schema version. `metadata.finalized_at_unix_ms` is the sole run-level finalization timestamp; this is `RunMetadata::finalized_at_unix_ms` in Rust. Active snapshots have `None`, finalized Runs have `Some(timestamp)`, and Event-level completion timestamps remain unchanged. Active in-memory snapshots serialize `metadata.finalized_at_unix_ms` as `null`, while persisted CLI artifacts require numeric finalization. Schema-v1 Run JSON is rejected by the CLI and must be regenerated with a current tailtriage version.

CLI Run-artifact analysis is strict by default. Error-level canonical core findings stop report generation before stdout; warning-only findings remain accepted. `--allow-ambiguous-artifact` explicitly requests canonical permissive normalization, discloses every original issue on stderr, and analyzes normalized evidence only. Tracing import `--strict` is a separate malformed/incomplete `tt.*` parser/import policy and does not control saved-Run validation. Analyzer library defaults remain permissive, and core exposes explicit strict and permissive APIs. Reports provide evidence-ranked suspects and next checks as triage leads, not proof of root cause.

Suspect ranking selects the primary only after every eligible candidate receives final evidence-aware confidence. The deterministic order is final confidence descending, then raw score descending, then stable suspect-kind rank, with InsufficientEvidence last; raw-score proximity still controls ambiguity membership, and all ambiguity-cluster members are capped uniformly. These rankings remain triage leads, not proof of root cause.

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

### Review artifacts before sharing

Run, Report, and validation artifacts can contain operational or environment metadata, including host/PID, routes, queue/stage/in-flight labels, warnings, service or run identifiers, paths, and workflow-specific details. Review artifacts and, where appropriate, redact sensitive values before sharing them outside the intended trust boundary. Tailtriage does not automatically sanitize these artifacts. Structured JSON remains lossless data; human-readable output visibly escapes artifact-controlled control characters at human-output sinks.

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

## Report interpretation during operations

Use the [analyzer guide](analyzer-guide.md) to select a suspect-led next check and compare one controlled rerun. The [analyzer behavior reference](diagnostics.md) owns exact report fields, `evidence_quality` semantics, scoring, confidence, and fallback behavior. Operationally, treat every suspect as a lead rather than proof and preserve the capture conditions and analyzer configuration across the comparison.

When the result is `insufficient_evidence`, use the guide's capture-improvement next check rather than treating abstention as evidence that no bottleneck exists.

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

* [validation overview](dev/VALIDATION.md)
* [runtime cost measurement](runtime-cost.md)
* [collector limits and stress guidance](collector-limits.md)
* [`scripts/measure_runtime_cost.py`](../scripts/measure_runtime_cost.py)
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

### Request completion, cancellation, and shutdown lifecycle

Explicit completion remains preferred whenever the application knows the request outcome. Dropping an admitted unfinished completion token while capture is still open records one completed request with outcome `cancelled`; Drop is non-panicking, including during panic unwinding. If shutdown wins before a held token finishes or drops, that request is recorded only as unfinished metadata and a late finish or Drop is inert. A finalized Run is immutable to late request admission, completion, stage, queue, in-flight, runtime-snapshot, sampler-metadata, and end-reason mutations.

Strict lifecycle shutdown with pending requests returns a retryable lifecycle error, performs no sink attempt, leaves pending requests open, and does not add finalization timestamps, unfinished metadata, or lifecycle warnings. Once an eligible shutdown attempts the sink, that finalization is terminal and single-shot on both success and failure; repeated or concurrent shutdown callers observe the same terminal attempt rather than writing again. Controller completion Drop participates in admitted-generation drain accounting exactly once, so a closing generation can finalize after the last admitted token is dropped. Dropping an admitted request-completion token while capture is open records one request outcome `cancelled` and does not itself fabricate child evidence. Independently, any queue/stage helper that was polled and then dropped while capture remains open records one bounded partial child event; tracing spans remain completed-only; late Drop after finalization is inert.



Overlap-safe queue and same-name stage attribution use request-scoped bounded attribution and do not double-count overlap. Complete run-relative intervals are unioned within the request scope; duration-only fallback remains capped by the parent request duration.

### Partial queue and stage events

Queue and stage Rust structs include `completed: bool`. Constructors default to completed evidence, and `into_partial()` intentionally constructs partial evidence. Schema-v2 JSON without `completed` is interpreted as completed evidence, and completed events omit `completed` when serialized.

Timing starts on first poll. Dropping a never-polled helper records no event. Dropping a polled pending helper while capture is open records one bounded partial event whose duration ends at observed helper Drop; late Drop after collector finalization is inert. Partial evidence is a lower-bound observation and does not prove that the underlying operation stopped. For partial stages, `success` is forced to `false`; it is not a completed operation result, so completion-aware consumers must inspect `completed`. Tracing spans remain completed-only. Analyzer reports keep completed queue/stage distributions completed-only, surface partial helper durations as observed lower-bound evidence, and apply evidence-aware confidence before final ranking.



## Partial queue/stage evidence

Completed queue/stage distributions exclude partial observations. Partial durations are an observed lower bound: tailtriage observed the helper from first poll until Drop, not proof that the underlying operation completed, failed, or stopped. Partial evidence remains visible in event totals, evidence-quality limitations, top-level warnings, and suspect evidence.

Queue/service public p95 fields remain completed-only. Materially partial-reliant queue/stage candidates cannot exceed medium confidence; partial evidence that does not affect selected eligibility or score does not automatically cap a completed candidate. Partial stage `success = false` is not interpreted as a completed operation failure.

Global, route, and temporal projections share this policy. Tracing intake remains completed-only. Completed-only Report JSON and text remain unchanged; mixed or partial Runs may change scores or ranking only when explicitly labeled lower-bound evidence is selected and qualified. Suspects remain triage leads, not root-cause proof.
