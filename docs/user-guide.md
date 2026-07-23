# User guide

This guide teaches the default `tailtriage` workflow for end users.

For production rollout, capture mode choice, runtime-sampling decisions, artifact sizing, truncation/capture-limit behavior, and weak-signal troubleshooting, see [operations.md](operations.md).

## 1) Default adoption path

For most services, use:

- `tailtriage` for capture instrumentation
- `tailtriage-cli` for artifact analysis/report generation

Install (default CLI path):

```bash
cargo add tailtriage
cargo install tailtriage-cli
```

For embedded/in-process Rust analysis and report generation, add `tailtriage-analyzer`:

```bash
cargo add tailtriage-analyzer
```

Optional integrations:

```bash
cargo add tailtriage --features axum
cargo add tailtriage --features tracing
cargo add tailtriage --features tracing-live
cargo add tailtriage --features tracing-tokio
```

The `controller` and `tokio` namespaces are available with default features; `axum` and tracing intake remain opt-in.

### Using existing tracing spans

Use `tailtriage --features tracing-live` when you want the default crate façade (`tailtriage::tracing`) for live tracing intake, or use `tailtriage-tracing` directly when you want the narrow crate boundary. This path is for services that already use Rust `tracing` and already have stable per-work-item IDs that can be converted into unique tailtriage request IDs. New integrations without existing tracing/correlation should start with native `tailtriage` capture first.

This path converts tracing-shaped request, stage, and queue evidence into standard Run artifacts for the normal `tailtriage analyze` workflow. It is not a tracing backend. For one completed logical request/work item, every request, stage, and queue span must carry the same `tt.request_id`; child stage/queue evidence is correlated to retained request evidence by `tt.request_id`. The `tt.request_id` value must be unique among completed requests in one Run.

Install the façade for typed records plus JSONL import APIs:

```bash
cargo add tailtriage --features tracing
```

Or install the focused crate directly:

```bash
cargo add tailtriage-tracing
```

A) Completed-span JSONL intake path:

```bash
tailtriage import tracing-spans-jsonl completed-spans.jsonl --service checkout --output tailtriage-run.json
tailtriage analyze tailtriage-run.json
```

#### What this imports

- Completed tailtriage tracing span JSONL.
- The stable wrapper shape is `{"format":"tailtriage.tracing-span.v1","span":{...}}`.
- Ordinary `tracing_subscriber::fmt().json()` logs are unsupported and rejected.

#### What this writes

- `tailtriage import tracing-spans-jsonl` writes Run JSON, not Report JSON.
- Analysis is a separate `tailtriage analyze tailtriage-run.json` step.

#### Strict vs non-strict

- `--strict` fails malformed or incomplete `tt.*` spans.
- Non-strict mode skips malformed `tt.*` spans and prints `warning: ...` messages.

#### Retention limits

- Offline import exposes request/stage/queue retention options because those are the imported evidence types.
- It does not expose runtime-snapshot or in-flight-snapshot limit flags.

#### Runtime evidence

- Offline import does not ingest runtime snapshots or in-flight snapshots.
- Tracing-only runs do not fabricate runtime snapshots, so executor/blocking-pressure evidence can be weaker or absent.
- Artifacts with run-relative monotonic offsets give temporal segmentation a more stable within-run ordering; older or partial imported artifacts fall back to Unix-ms timestamp anchors.

#### Zero-request artifacts

- Persisted CLI artifacts require at least one completed request.
- In-process library snapshots may still be zero-request for inspection.

#### Completed-span JSONL caveat

- Completed-span JSONL output contains retained original tracing source records selected after parsing, retention limits, and core normalization.
- Source identity and source fields represented by `SpanRecord` are preserved exactly.
- Direct input order and JSONL input order are preserved through replay; live session output is section-grouped as request records, then stage records, then queue records.
- Replay parity is limited to representable normalized request/stage/queue evidence.
- Completed-span JSONL does not encode Run-only metadata, runtime snapshots, in-flight snapshots, lifecycle warnings, semantic/raw truncation counters, source-line context, omitted-source diagnostics, or output failures.
- Run JSON remains the complete persisted artifact for analysis and operational handoff.

B) Direct Run JSON path with async span instrumentation (`live` feature required):

```bash
cargo add tailtriage --features tracing-live
cargo add tracing tracing-subscriber
cargo add tokio --features macros,rt-multi-thread
```

Direct crate equivalent:

```bash
cargo add tailtriage-tracing --features live
cargo add tracing tracing-subscriber
cargo add tokio --features macros,rt-multi-thread
```


```rust,no_run
use tailtriage::tracing::TracingSession;
use tracing::Instrument as _;
use tracing_subscriber::prelude::*;

async fn work() {
    // Your request work goes here.
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let session = TracingSession::builder("checkout-service")
        .run_json_path("target/tailtriage-examples/checkout.run.json")
        .build()?;
    tracing_subscriber::registry()
        .with(session.layer())
        .init(); // startup-only: global subscriber installation for this process
    {
        let span = tracing::info_span!(
            "request",
            tt.kind = "request",
            tt.request_id = "req-1",
            tt.route = "/checkout",
            tt.outcome = "ok",
        );
        work().instrument(span).await;
    } // the request span is closed before shutdown
    let imported = session.shutdown().await?;
    let _ = imported;
    Ok(())
}
```

If using the focused crate directly, replace `tailtriage::tracing::TracingSession` with `tailtriage_tracing::TracingSession`.

Stage and queue spans use their own `tt.stage` / `tt.queue` fields around the awaited work they measure. Every request, stage, and queue span for one completed logical request/work item must carry the same unique tailtriage `tt.request_id`; missing, inconsistent, or duplicated IDs cause child stage/queue evidence to be skipped, weakened, or reported as ambiguous.

`tt.outcome` on request spans is optional: missing values default to `ok` with a warning; recommended common labels are `ok`, `error`, `timeout`, `cancelled`, and `rejected`; custom non-empty labels are preserved exactly.

Live tracing intake only tracks spans that are tailtriage candidates at span creation time. Declare `tt.*` fields when the span is created. If a value is filled later, declare it with `tracing::field::Empty` and then call `span.record(...)`. Do not add brand-new `tt.*` fields later with `span.record(...)` and expect the span to be tracked.

In service code, add `session.layer()` beside your existing tracing layers and install the resulting subscriber in the application's normal process-wide/global subscriber setup. `set_default` is scoped to the current thread and guard lifetime; service startup should install the tailtriage layer in the process-wide subscriber setup.

Then analyze directly:

```bash
tailtriage analyze target/tailtriage-examples/checkout.run.json
```

Use `.instrument(...)` for async work; `snapshot_run()` is the non-consuming inspection API, while `shutdown()` finalizes the session.

Tokio runtime sampler coupling via `TracingSession` requires `tracing-tokio` on the `tailtriage` façade or `tokio` on the focused `tailtriage-tracing` crate. Background sampling is explicit: configure `sampler_interval(...)` to start it, or call `manual_runtime_snapshots()` for deterministic demos/validation and inject snapshots manually with `record_runtime_snapshot(...)`. Use `run_json_path(...)` to write Run JSON on shutdown, then analyze separately with `tailtriage analyze <run.json>`:

```bash
cargo add tailtriage --features tracing-tokio
cargo add tracing tracing-subscriber
cargo add tokio --features macros,rt-multi-thread
```

Direct crate equivalent:

```bash
cargo add tailtriage-tracing --features tokio
cargo add tracing tracing-subscriber
cargo add tokio --features macros,rt-multi-thread
```

For the full tracing setup details and both flows, see `tailtriage-tracing/README.md`.

## 2) Core workflow: capture -> analyze -> next check -> re-run

### Capture

```rust,no_run
use tailtriage::Tailtriage;

# fn main() -> Result<(), Box<dyn std::error::Error>> {
let run = Tailtriage::builder("checkout-service")
    .output("tailtriage-run.json")
    .build()?;

let started = run.begin_request("/checkout");
started.completion.finish_ok();

run.shutdown()?;
# Ok(())
# }
```

### Analyze

```bash
tailtriage analyze tailtriage-run.json --format json
```

### Decide next check

Read output in this order:

1. `primary_suspect.kind`
2. `primary_suspect.evidence[]`
3. `primary_suspect.next_checks[]`

Then run one targeted check, change one thing, and re-run under comparable load.

For services that already emit `tracing` spans, see “Using existing tracing spans” above for the JSONL import and live recorder paths.

## 3) In-process analysis (embedded Rust)

If you want analysis/report generation inside service code or tests, use `tailtriage-analyzer`:

```rust
use tailtriage_analyzer::{analyze_run, render_text, AnalyzeOptions};
use tailtriage_analyzer::render_json_pretty;

# use tailtriage::Run;
# fn example(run: Run) -> Result<(), Box<dyn std::error::Error>> {
let report = analyze_run(&run, AnalyzeOptions::default());
let text = render_text(&report);
let json = render_json_pretty(&report)?;
# let _ = (text, json);
# Ok(())
# }
```

Run artifact JSON is capture output and CLI input. Report JSON is analyzer/CLI output. Typed `Report` is the in-process analyzer result.

## Request ID contract

`request_id` is the per-run tailtriage identity of one completed logical request or work item. It must be unique among completed requests in one Run. Stage and queue events must reuse that ID only for the same logical request.

External correlation or distributed trace IDs may repeat across retries, fanout branches, batch items, or attempts. When they can repeat, derive a unique tailtriage `request_id`, such as `trace_id:span_id`, `job_id:attempt`, or `batch_id:item_id`. The analyzer can warn or, with strict artifact validation, fail on mechanical ambiguity, but it cannot infer whether your request boundary, retry model, fanout model, or propagation model is semantically correct. Suspects remain triage leads and next checks, not proof of root cause.

Current analyzer semantics are completed-run or stable-snapshot batch analysis, not live streaming analysis.

### Analyzer tuning examples

Start from defaults, then tune only what you need.

Rust (checked API):

```rust
use tailtriage::Run;
use tailtriage_analyzer::{try_analyze_run, AnalyzeOptions};

fn analyze_with_tuning(run: &Run) -> Result<(), Box<dyn std::error::Error>> {
    let options = AnalyzeOptions::default()
        .with_queueing(|o| o.trigger_permille = 450);
    let report = try_analyze_run(run, options)?;
    let _ = report;
    Ok(())
}
```

TOML (`[analyzer]` schema):

```toml
[analyzer]
schema_version = 1

[analyzer.queueing]
trigger_permille = 450
```

CLI:

```bash
tailtriage analyze tailtriage-run.json \
  --analyzer-config examples/analyzer-config.toml \
  --analyzer-set queueing.trigger_permille=450
```

Use `tailtriage analyze --help-analyzer-options` to list supported override paths and value formats.

## 4) Request lifecycle contract (required)

`begin_request(...)` / `begin_request_with(...)` returns `StartedRequest`:

- `started.handle` (`RequestHandle`) for instrumentation
- `started.completion` (`RequestCompletion`) for explicit completion

```rust,no_run
use tailtriage::Tailtriage;

# async fn demo(run: &Tailtriage) -> Result<(), Box<dyn std::error::Error>> {
let started = run.begin_request("/checkout");
let req = started.handle.clone();

req.queue("checkout_queue").await_on(async {}).await;
let _: Result<(), ()> = req.stage("downstream_call").await_on(async { Ok(()) }).await;

started.completion.finish_ok();
# Ok(())
# }
```

Important semantics:

- finish exactly once (`finish`, `finish_ok`, `finish_result`)
- drop does not auto-finish
- `shutdown()` does not fabricate completion/outcome
- `strict_lifecycle(true)` can fail shutdown when unfinished requests remain

## 5) Direct capture vs controller

Use **direct capture** (`Tailtriage`) when you want a straightforward run lifecycle in app code.

Use **controller** (`TailtriageController`) when your service is long-lived and you need repeated bounded windows over time:

- enable capture window
- collect
- disable/finalize
- re-enable later

Minimal controller window example:

```rust,no_run
use tailtriage::controller::TailtriageController;

# fn demo() -> Result<(), Box<dyn std::error::Error>> {
let controller = TailtriageController::builder("checkout-service")
    .initially_enabled(false)
    .output("tailtriage-run.json")
    .build()?;

let _generation = controller.enable()?;
let started = controller.begin_request("/checkout");
started.completion.finish_ok();
let _ = controller.disable()?;
# Ok(())
# }
```

Controller details: [tailtriage-controller/README.md](../tailtriage-controller/README.md)

## 6) Controller TOML config and reload semantics

Controller config is for repeatable operational settings across environments.

Stay with builder defaults when you are exploring locally or need one straightforward capture setup. Move to TOML when you need consistent operational settings (service identity, output path, capture limits, sampler template) across environments without rebuilding.

Minimal TOML shape:

```toml
[controller]
service_name = "checkout-service"

[controller.activation]
mode = "light"

[controller.activation.sink]
type = "local_json"
output_path = "tailtriage-run.json"
```

At contract level:

- set config file path with `config_path(...)`
- call `reload_config()` to refresh the template from file
- reload applies to **future generations only**
- active generation keeps activation-time config

See crate README for the full TOML field reference and expanded starter example: [tailtriage-controller/README.md](../tailtriage-controller/README.md)

## 7) Runtime sampler: when and why

Add runtime sampling when request timing alone does not clearly separate:

- queueing saturation
- executor pressure
- blocking-pool pressure

With default features, `tailtriage::tokio` is available out of the box. Start `RuntimeSampler` explicitly for each run when needed; `CaptureMode` does not auto-start sampling.

Key constraints:

- start inside an active Tokio runtime
- one successful sampler start per run
- runtime snapshot retention is bounded by core limits
- Tokio tracing sessions use the same core `CaptureMode`/`CaptureLimits`/`CaptureLimitsOverride` model (no tracing-specific retention knob)
- some runtime fields require `tokio_unstable`

Sampler details: [tailtriage-tokio/README.md](../tailtriage-tokio/README.md)

## 8) Axum adapter: what it is and is not

`tailtriage-axum` is a framework-boundary ergonomics layer:

- middleware handles request start/finish at Axum boundary
- extractor passes request handle into handlers

It is not automatic diagnosis. Queue/stage/inflight instrumentation is still explicit in handler/helper code.

Adapter details: [tailtriage-axum/README.md](../tailtriage-axum/README.md)

## 9) What to do when result is `insufficient_evidence`

When `primary_suspect.kind` is `insufficient_evidence`:

1. add at least one queue wrapper around suspected waits
2. add at least one stage wrapper around suspected downstream work
3. optionally add runtime sampler if runtime pressure is unclear
4. rerun with comparable load and compare evidence movement

Use [diagnostics.md](diagnostics.md) for interpretation details.

## 10) Tokio primitive helpers

Import via default crate path:

```rust
use tailtriage::tokio::TokioRequestHandleExt;
```

These helpers are shorthand for explicit `queue(...).await_on(...)`, `stage(...).await_on(...)`, and `inflight(...)` instrumentation; they do not finish requests. For a compact end-to-end helper example, see the Tokio helper example in [`tailtriage-tokio/README.md`](../tailtriage-tokio/README.md).

| Use case | Helper | Records |
|---|---|---|
| DB pool / capacity wait | `semaphore(...).acquire()` | queue |
| owned permit wait | `owned_semaphore(...).acquire_owned()` | queue |
| bounded channel backpressure | `mpsc_send(...)` | queue |
| async mutex contention | `mutex_lock(...)` | queue |
| async rwlock contention | `rwlock_read(...)` / `rwlock_write(...)` | queue |
| spawned task result | `join_task(...)` | stage |
| timeout-wrapped work | `timeout_stage(...)` | stage |
| blocking pool work | `blocking_stage(...)` | stage |
| active bounded section | `inflight_guard(...)` | in-flight |

Semantics notes:

- Queue/stage helper timing begins on first poll: dropping a never-polled helper records no event, while dropping a polled pending helper records one bounded partial event if capture remains open. Partial duration ends at observed helper Drop and does not prove the underlying operation stopped.
- The helper API intentionally does not include a generic mpsc receive wait helper. Receiver-side recv wait cannot distinguish idle workers from queued work residence time. For worker intake, start request/work-item capture after receiving the item unless you have explicit enqueue timestamps.
- `join_task(...)` records await time for the supplied `JoinHandle`, not necessarily the full task runtime.
- `join_task(...)`, `timeout_stage(...)`, and `blocking_stage(...)` preserve nested `Result`s; recorded stage success/failure comes from the outer Tokio wrapper result, so `Ok(Err(_))` is preserved and records as successful.
- `blocking_stage(...)` is lazy: it submits `spawn_blocking` only when awaited. Use `tokio::task::spawn_blocking` plus `join_task(...)` when you need eager overlap.
- `timeout_stage(...)` is lazy: timeout budget starts when the returned future is polled/awaited, not when the helper is constructed.
- If you need blocking work to start immediately or overlap with other work, call `tokio::task::spawn_blocking(...)` directly and instrument its `JoinHandle` with `join_task(...)`.

## 11) Next docs

- [Documentation index](README.md)
- [Diagnostics guide](diagnostics.md)
- [Getting started demos](getting-started-demo.md)
- [Architecture](architecture.md)

## Live tracing session migration

Use `TracingSession` as the current live tracing entry point. Older names may appear in changelog history, but current setup should use the unified session builder.

| Old usage | Final usage |
| --- | --- |
| `TracingRecorder::builder(...)` | `TracingSession::builder(...)` |
| `TracingIntakeSession::builder(...)` | `TracingSession::builder(...)` |
| `TracingTokioSession::builder(...).start()` | `TracingSession::builder(...).sampler_interval(...).build()` |
| `recorder_limits(...)` | `limits(...)` |
| synchronous `shutdown()?` | `shutdown().await?` |
| deterministic manual mode | `manual_runtime_snapshots()` plus `record_runtime_snapshot(...)?` |

A plain live session still captures request, stage, and queue evidence. Background runtime sampling is opt-in through `sampler_interval(...)`; compiling Tokio support does not automatically start runtime sampling. Manual runtime collection is opt-in through `manual_runtime_snapshots()`, and manual recording without runtime collection returns a configuration error. Manual snapshots may coexist with background sampling.

Run JSON is the complete persisted artifact. Completed-span JSONL output contains retained original tracing source records and preserves the retained original source identity for replayable tracing evidence, but it omits runtime snapshots and other Run-only state. Each output file is an independent transaction, so completed-span JSONL and Run JSON are written, flushed, and renamed separately.

### Request completion, cancellation, and shutdown lifecycle

Explicit completion remains preferred whenever the application knows the request outcome. Dropping an admitted unfinished completion token while capture is still open records one completed request with outcome `cancelled`; Drop is non-panicking, including during panic unwinding. If shutdown wins before a held token finishes or drops, that request is recorded only as unfinished metadata and a late finish or Drop is inert. A finalized Run is immutable to late request admission, completion, stage, queue, in-flight, runtime-snapshot, sampler-metadata, and end-reason mutations.

Strict lifecycle shutdown with pending requests returns a retryable lifecycle error, performs no sink attempt, leaves pending requests open, and does not add finalization timestamps, unfinished metadata, or lifecycle warnings. Once an eligible shutdown attempts the sink, that finalization is terminal and single-shot on both success and failure; repeated or concurrent shutdown callers observe the same terminal attempt rather than writing again. Controller completion Drop participates in admitted-generation drain accounting exactly once, so a closing generation can finalize after the last admitted token is dropped. Completion-token Drop records the cancelled request and does not itself fabricate child evidence. Independently, any queue or stage helper that was polled and then dropped while capture was open records one partial child event.


### Partial queue and stage events

Completed queue and stage JSON remains wire-compatible: schema version stays `2`, older schema-v2 JSON without `completed` reads as completed evidence, and completed events omit `completed` when serialized. The Rust structs now include `completed: bool`, which is an intentional pre-1.0 source break for external exhaustive `StageEvent` and `QueueEvent` struct literals. Prefer `StageEvent::new(...)` and `QueueEvent::new(...)`; constructors default to completed evidence and `into_partial()` should be used only when intentionally constructing partial evidence.

Timing starts on first poll. Dropping a never-polled helper records no event. Dropping a polled pending helper while capture is open records one bounded partial event whose duration ends at observed helper Drop; late Drop after collector finalization is inert. Partial evidence is a lower-bound observation and does not prove that the underlying operation stopped. For partial stages, `success` is forced to `false`; it is not a completed operation result, so completion-aware consumers must inspect `completed`. Tracing spans remain completed-only, and analyzer interpretation is unchanged in this release.

Migration example:

```rust
# use tailtriage_core::StageEvent;
// Old exhaustive struct literal (now must include `completed`).
let _old = StageEvent {
    request_id: "req".into(),
    stage: "db".into(),
    started_at_unix_ms: 1,
    started_at_run_us: None,
    finished_at_unix_ms: 2,
    finished_at_run_us: None,
    latency_us: 10,
    success: true,
    completed: true,
};

// Recommended: constructors default to completed evidence.
let completed = StageEvent::new("req", "db", 1, 2, 10, true);
let partial = completed.clone().into_partial();
```
