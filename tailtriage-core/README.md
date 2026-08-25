# tailtriage-core

`tailtriage-core` is the framework-agnostic capture foundation for `tailtriage`.

Use it when you want explicit request lifecycle instrumentation and bounded JSON artifacts without controller, Axum, or Tokio runtime-sampler APIs unless you add them separately.

## What this crate does

`tailtriage-core` owns capture-side lifecycle semantics:

- request admission
- queue/stage/inflight instrumentation
- explicit request completion
- bounded in-memory retention
- JSON run artifact writing

For in-process analysis/report generation, use `tailtriage-analyzer`.
For command-line analysis of saved artifacts, use `tailtriage-cli`.

## Crate selection

Use `tailtriage-core` when you want the smallest framework-agnostic capture surface.

Use `tailtriage` when you want the recommended default entry point: an aggregator/re-export crate with optional integrations behind features.

## Installation

```bash
cargo add tailtriage-core
```

## Quick start

```rust,no_run
use tailtriage_core::Tailtriage;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let run = Tailtriage::builder("checkout-service")
        .output("tailtriage-run.json")
        .build()?;

    let started = run.begin_request("/checkout");
    started.completion.finish_ok();

    run.shutdown()?;
    Ok(())
}
```

## Request lifecycle

`begin_request(...)` / `begin_request_with(...)` returns `StartedRequest` with:

- `started.handle` for queue/stage/inflight instrumentation
- `started.completion` for explicit finish

For `Arc<Tailtriage>` flows that move request handles across spawned tasks or helper layers, use `begin_request_owned(...)` / `begin_request_with_owned(...)`. Owned handles keep the same lifecycle rule: instrumentation does not finish the request, and the completion token must be finished exactly once.

```rust,no_run
use tailtriage_core::{RequestOptions, Tailtriage};

async fn demo() -> Result<(), Box<dyn std::error::Error>> {
    let run = Tailtriage::builder("checkout-service")
        .output("tailtriage-run.json")
        .build()?;

    let started = run.begin_request_with(
        "/checkout",
        RequestOptions::new().request_id("req-1").kind("http"),
    );
    let req = started.handle.clone();

    req.queue("ingress").await_on(async {}).await;
    req.stage("db")
        .await_on(async { Ok::<(), std::io::Error>(()) })
        .await?;

    started.completion.finish_ok();
    run.shutdown()?;
    Ok(())
}
```

## Output sinks

`tailtriage-core` captures run data and finalizes through a sink. It does not perform analysis/report generation.

- `LocalJsonSink` (or builder `.output(...)`) writes Run artifact JSON to disk.
- `request_id` is the per-run tailtriage identity of one completed logical request/work item. Explicit IDs should be unique among completed requests in one Run; queue and stage events should reuse an ID only for evidence from that same logical request. Duplicate retained completed IDs are allowed for backward compatibility but surface a lifecycle warning because request-scoped attribution can be ambiguous.
- `MemorySink` stores finalized typed `Run` values in memory.
- `DiscardSink` finalizes lifecycle and drops the finalized `Run` without persisting output.

`MemorySink` stores only the last finalized `Run`; each new finalized run replaces the previous stored value.

Use `MemorySink` when you want in-process analysis. `DiscardSink` drops finalized runs; use `MemorySink` instead when the finalized `Run` should be analyzed in process.

```rust,no_run
use tailtriage_core::{MemorySink, Tailtriage};

# fn example() -> Result<(), Box<dyn std::error::Error>> {
let sink = MemorySink::new();
let run = Tailtriage::builder("checkout-service")
    .sink(sink.clone())
    .build()?;

let started = run.begin_request("/checkout");
started.completion.finish_ok();
run.shutdown()?;

let finalized = sink.last_run();
# let _ = finalized;
# Ok(())
# }
```

### Two easy-to-miss helpers

For infallible async work, `StageTimer::await_value(...)` avoids a dummy `Result`:

```rust,no_run
# use tailtriage_core::Tailtriage;
# async fn demo(run: Tailtriage) {
# let req = run.begin_request("/x").handle;
let value = req.stage("cache").await_value(async { 42 }).await;
# let _ = value;
# }
```

When queue depth is known at enqueue time, `QueueTimer::with_depth_at_start(...)` records it directly:

```rust,no_run
# use tailtriage_core::Tailtriage;
# async fn demo(run: Tailtriage) {
# let req = run.begin_request("/x").handle;
req.queue("ingress")
    .with_depth_at_start(12)
    .await_on(async {})
    .await;
# }
```

## Lifecycle contract

- `queue(...)`, `stage(...)`, and `inflight(...)` do **not** finish requests.
- Every admitted request must be finished exactly once.
- Dropping an admitted completion token while capture is open records one request with outcome `cancelled`.
- Non-strict lifecycle: `shutdown()` writes the artifact and records unfinished-request warnings/metadata.
- `strict_lifecycle(true)`: unfinished requests cause `shutdown()` to return an error and no artifact is written.

Finalization timestamps:

- Active `snapshot()` output is not finalized (`metadata.finalized_at_unix_ms == None`).
- `shutdown()` writes final artifacts with both:
  - `metadata.finalized_at_unix_ms` set to shutdown time
- Schema-v1 Run JSON is not accepted by the current CLI and must be regenerated with a current tailtriage version.

## Timing model

- Duration fields in microseconds (`latency_us`, `wait_us`, stage `latency_us`) are authoritative for elapsed-time analysis.
- Unix millisecond timestamps are wall-clock anchors for log correlation, artifact readability, and coarse temporal grouping.
- Wall-clock timestamps can be coarse and can move if the system clock changes.
- Analyzer scoring uses duration fields for latency, queue wait, and stage duration.
- Temporal segmentation prefers run-relative monotonic offsets when present and falls back to Unix-ms wall-clock anchors for artifacts without complete run-relative timing.

## Capture modes

Modes change retention defaults only. They do not change lifecycle semantics and do **not** auto-start runtime sampling.

- `CaptureMode::Light`
- `CaptureMode::Investigation`

Override limits with:

- `capture_limits(...)` (full override)
- `capture_limits_override(...)` (field-level override)

`max_requests` bounds completed-retained requests plus currently pending admitted requests. Once
that capacity is exhausted, refused request instrumentation and completion are inert and create no
child evidence. Configured request, queue, stage, in-flight, and runtime evidence limits
independently bound their retained or live evidence as implemented. Reaching a limit is surfaced
through the existing drop, truncation, and limits-hit accounting rather than silently expanding
collector state.

## Advanced: assembling completed run artifacts

Most users should use `Tailtriage::builder(...)` for live request instrumentation.
Use `RunBuilder` only when you already have completed request, stage, queue, in-flight, or runtime evidence and need to assemble a standard `Run` artifact.

`RunBuilder` is intended for import/conversion paths. It does not perform live lifecycle tracking. `RunBuilder::new` validates top-level run timestamp ordering. Each `push_*` call validates required event/snapshot shape and timestamp ordering before retention is applied. Completed request latency, stage latency, and queue wait fields are authoritative evidence; `RunBuilder` does not synthesize, repair, or reject those durations based on wall-clock timestamp deltas. It applies the same bounded retention/truncation semantics as live core capture, so events beyond configured `CaptureLimits` are dropped without error and `Run.truncation` counters are updated. `RunBuilder` does not validate cross-event correlation (for example a stage without a matching request) and does not synthesize lifecycle completions. It surfaces a lifecycle warning when retained completed requests contain duplicate request IDs. For assembled/imported artifacts, host and pid default to `None`, and generated run IDs use the same core run-id semantics as live capture.

```rust,no_run
use tailtriage_core::{RequestEvent, RunBuilder, RunBuilderOptions};

fn assemble_run() -> Result<(), Box<dyn std::error::Error>> {
    let mut builder = RunBuilder::new(RunBuilderOptions::new("checkout-service"))?;
    builder.push_request(RequestEvent {
        request_id: "req-1".into(),
        route: "/test".into(),
        kind: Some("http".into()),
        started_at_unix_ms: 1,
        started_at_run_us: None,
        finished_at_unix_ms: 2,
        finished_at_run_us: None,
        latency_us: 1_000,
        outcome: "ok".into(),
    })?;

    let run = builder.finish();
    assert_eq!(run.requests.len(), 1);
    Ok(())
}
```

## What this crate does not do

This crate does not provide:

- repeated arm/disarm controller windows
- Tokio runtime sampling
- Axum middleware/extractors
- analysis/report generation

Use sibling crates for those surfaces: `tailtriage-controller`, `tailtriage-tokio`, `tailtriage-axum`, `tailtriage-analyzer`, and `tailtriage-cli`.

## Run validation

`tailtriage-core` exposes `inspect_run`, `validate_run_strict`, and `normalize_run_permissive` as the canonical generic completed-`Run` integrity APIs. Strict validation checks the original unnormalized candidate and rejects error-level integrity issues; warning-only missing run-relative precision remains accepted. Permissive normalization retains duration-authoritative evidence where possible, clears invalid optional run-relative offsets without rewriting durations, excludes ambiguous duplicated requests and invalid request-scoped child evidence deterministically, and provides stable issue-code summaries for analyzer, CLI, tracing, and native lifecycle surfaces. `RuntimeSnapshot::worker_count` is optional schema-v2 evidence and may be absent; positive values round-trip, zero fails strict validation, and permissive normalization clears only the zero value while retaining the snapshot. The analyzer uses complete, consistent, positive worker evidence to normalize executor runnable-queue scoring per worker; missing, partial, inconsistent, or invalid evidence uses absolute-depth fallback scoring with the documented confidence policy. Exhaustive Rust `RuntimeSnapshot` literals must specify `worker_count: None` when the value is unknown.

### Request completion, cancellation, and shutdown lifecycle

Explicit completion remains preferred whenever the application knows the request outcome. Dropping an admitted unfinished completion token while capture is still open records one completed request with outcome `cancelled`; Drop is non-panicking, including during panic unwinding. If shutdown wins before a held token finishes or drops, that request is recorded only as unfinished metadata and a late finish or Drop is inert. A finalized Run is immutable to late request admission, completion, stage, queue, in-flight, runtime-snapshot, sampler-metadata, and end-reason mutations.

Strict lifecycle shutdown with pending requests returns a retryable lifecycle error, performs no sink attempt, leaves pending requests open, and does not add finalization timestamps, unfinished metadata, or lifecycle warnings. Once an eligible shutdown attempts the sink, that finalization is terminal and single-shot on both success and failure; repeated or concurrent shutdown callers observe the same terminal attempt rather than writing again. Controller completion Drop participates in admitted-generation drain accounting exactly once, so a closing generation can finalize after the last admitted token is dropped. Completion-token Drop records the cancelled request and does not itself fabricate child evidence. Independently, any queue or stage helper that was polled and then dropped while capture was open records one partial child event.

Accepted in-flight count transitions remain in the bounded Run artifact, including a final zero when retention permits. While capture is open, live collector state retains only gauges with positive counts: a one-to-zero Drop first attempts the snapshot through normal bounded retention and then removes the live label. Thus a saturated artifact can drop that snapshot and update truncation accounting while live-state cleanup still completes. In-flight Drop after finalization is inert.


### Partial queue and stage events

Queue and stage Rust structs include `completed: bool`. Constructors default to completed evidence, and `into_partial()` intentionally constructs partial evidence. Schema-v2 JSON without `completed` is interpreted as completed evidence, and completed events omit `completed` when serialized.

Timing starts on first poll. Dropping a never-polled helper records no event. Dropping a polled pending helper while capture is open records one bounded partial event whose duration ends at observed helper Drop; late Drop after collector finalization is inert. Partial evidence is a lower-bound observation and does not prove that the underlying operation stopped. For partial stages, `success` is forced to `false`; it is not a completed operation result, so completion-aware consumers must inspect `completed`. Tracing spans remain completed-only. Analyzer reports keep completed queue/stage distributions completed-only, surface partial helper durations as observed lower-bound evidence, and apply evidence-aware confidence before final ranking.
