# tailtriage-tracing

`tailtriage-tracing` is a narrow tracing intake bridge for completed `tt.*` spans.

It helps existing `tracing` users produce standard `tailtriage_core::Run` artifacts by:
- writing Run JSON on shutdown, and/or
- writing retained, semantically valid completed-span JSONL on shutdown.

It is **not**:
- an observability backend,
- generic tracing log scraping,
- an OTel/OTLP pipeline,
- proof of root cause (output remains triage leads).

## When to use this crate

Use this path when your service already uses Rust `tracing` and already has stable per-work-item IDs that can be converted into unique tailtriage request IDs. New integrations without existing tracing/correlation should start with native `tailtriage` capture first.

This crate converts tracing-shaped request, stage, and queue evidence into standard `tailtriage_core::Run` artifacts for the normal `tailtriage analyze` workflow. It is not a tracing backend.

For one completed logical request/work item, every request, stage, and queue span must carry the same `tt.request_id`. That `tt.request_id` is the unique tailtriage request ID for the completed request within one Run, not necessarily a raw distributed trace ID. Child stage/queue evidence is correlated to request evidence by `tt.request_id` during core normalization; missing, excluded, or ambiguous parent requests produce canonical core findings and may exclude the child from normalized analysis.

External trace/correlation IDs may repeat across retries, fanout branches, batch items, or attempts. When they can repeat, derive a unique tailtriage `tt.request_id` first, for example by adding attempt, span, branch, or item information. Users remain responsible for meaningful instrumentation and request-boundary semantics.

## Feature flags

- Base crate: typed `SpanRecord`, `ImportOptions`, `ImportedRun`, semantic constants, and `run_from_span_records(...)`.
- Default (`jsonl`): JSONL import APIs and stable wrapper parsing.
- `live`: enables the single public live intake path: `TracingSession`, `TracingSessionBuilder`, `TailtriageLayer`, and `RecorderLimits`.
- `tokio`: enables `TracingSession` runtime-sampler coupling and includes `live` (background sampling starts only when configured with `sampler_interval(...)`; deterministic runs can call `manual_runtime_snapshots()` and inject snapshots manually).

CLI offline import workflows only need JSONL import support and do not require the live `tracing_subscriber` layer dependency.

The same APIs are also available through the default `tailtriage` crate when enabling its `tracing`, `tracing-live`, or `tracing-tokio` façade features.

For live tracing intake sessions, `tailtriage-tracing` enables optional dependencies behind feature flags, but applications that call `tracing::...` or `tracing_subscriber::...` directly still need explicit app dependencies:

```bash
cargo add tailtriage-tracing --features live
cargo add tracing tracing-subscriber
cargo add tokio --features macros,rt-multi-thread
```

If you use Tokio runtime sampler coupling via `TracingSession`, use:

```bash
cargo add tailtriage-tracing --features tokio
cargo add tracing tracing-subscriber
cargo add tokio --features macros,rt-multi-thread
```

## Recommended live session setup (`live` feature)

```rust,no_run
use tailtriage_tracing::TracingSession;
use tracing::Instrument as _;
use tracing_subscriber::prelude::*;

async fn work() {
    // Your request work goes here.
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let session = TracingSession::builder("checkout-service")
        .run_json_path("target/tailtriage-examples/checkout.run.json")
        .completed_span_jsonl_path("target/tailtriage-examples/checkout.spans.jsonl")
        .build()?;

    tracing_subscriber::registry()
        .with(session.layer())
        .init(); // startup-only: global subscriber installation for this process

    {
        let request = tracing::info_span!(
            "request",
            tt.kind = "request",
            tt.request_id = "req-1",
            tt.route = "/checkout",
            tt.outcome = "ok"
        );
        work().instrument(request).await;
    } // the request span is closed before shutdown

    let imported = session.shutdown().await?;
    let _ = imported;
    Ok(())
}
```

Install the tailtriage layer beside your existing tracing layers in the application's normal process-wide subscriber setup; tailtriage augments your tracing pipeline rather than replacing it.

If your service already builds a subscriber in startup code, compose `session.layer()` onto that subscriber and install it once with your normal global install path (for example `.init()` during binary startup, or `tracing::subscriber::set_global_default(...)` with explicit error handling when needed).

`set_default` is scoped to the current thread and guard lifetime; service startup should install the tailtriage layer in the process-wide subscriber setup.

## Timing model

- Imported or live tracing evidence is converted to normal tailtriage Run timing fields.
- Live tracing samples finish wall time when the span closes.
- Newer live tracing output includes run-relative monotonic offsets for request, stage, and queue spans when those offsets are available; these offsets improve temporal grouping inside a captured run.
- Imported JSONL may omit run-relative offsets. Missing offsets remain supported: core emits the warning-only `precise_interval_validation_unavailable` finding, retains the duration evidence unchanged, and does not check the authoritative duration against coarse Unix-ms bounds as a generic validation fallback.
- `duration_us` remains the authoritative elapsed-time evidence when supplied or recorded by live tracing.
- Complete run-relative offsets provide optional precision for temporal grouping and precise parent/child containment. Partial offsets, inverted run-relative offsets, and duration-versus-offset mismatches beyond the shared core `2_000` microsecond tolerance are error-level core findings.
- In permissive normalization, core retains the event and authoritative duration for those invalid optional-precision findings while clearing both optional offsets. In strict validation, core rejects those error-level findings.
- Precise child containment is evaluated by core only when both the retained parent request and child stage/queue have complete valid normalized run-relative intervals. No Unix-ms wall-clock containment fallback is used.
- Unix-ms timestamps are wall-clock anchors and may be coarser than durations.
- Ordinary tracing log timestamps are not enough for completed-span import; completed spans need explicit start/end timing and semantic tt.* fields.

## Direct Run JSON path

Use `run_json_path(...)` when you want to skip a separate import step and write Run JSON through the same robust writer path used by native capture sinks:

```bash
tailtriage analyze target/tailtriage-examples/checkout.run.json
```

For both `TracingSession` and `TracingSession`, persisted-output shutdown
returns a zero-request error when no request is retained. When tracing intake warnings
exist (for example malformed `tt.*` fields), shutdown includes those warning messages in
the same error to guide setup corrections before rerunning capture.

## Completed-span JSONL path

Use `completed_span_jsonl_path(...)` when you want an offline import workflow:

```bash
tailtriage import tracing-spans-jsonl target/tailtriage-examples/checkout.spans.jsonl \
  --service checkout-service \
  --output target/tailtriage-examples/checkout.run.json

tailtriage analyze target/tailtriage-examples/checkout.run.json
```

If you configure both `completed_span_jsonl_path(...)` and `run_json_path(...)`, each
configured file is written independently through its own temp/rename path. The two
outputs are not committed as one atomic transaction: if the second write fails, the
first output may already exist as a finalized artifact. For production workflows that
need one canonical shutdown artifact, prefer `run_json_path(...)`. Completed-span JSONL
is written from retained original source spans. It preserves source span identity and
fields represented by `SpanRecord`, but remains replay/debug evidence rather than a
complete Run artifact or trace archive.

## Stable JSONL wrapper format

Stable completed-span JSONL records use this wrapper:

```json
{"format":"tailtriage.tracing-span.v1","span":{...}}
```

`format` is a wrapper-level field (not a `SpanRecord` field).
The simple library import APIs (`import_jsonl_reader` / `import_jsonl_path`) default to this wrapper-only mode and return an error for any non-empty non-wrapper JSON record.
Use `*_with_mode(..., JsonlParseMode::Compatible)` only for pre-stable/internal normalized completed-span records with explicit unix-ms start/end timestamps.

Close-event/fmt-like tracing log envelopes are not supported import input.
Ordinary tracing log JSON (for example `tracing_subscriber::fmt().json()` output) is unsupported and rejected by import.
Import does not guess span timing from line receive time: provide explicit unix-ms start/end timestamps on completed spans.

## `tt.*` field convention

| Span kind | Required fields | Optional fields |
| --- | --- | --- |
| request | `tt.kind="request"`, `tt.request_id`, `tt.route` | `tt.outcome` (optional non-empty string; recommended common labels: `ok`, `error`, `timeout`, `cancelled`, `rejected`) |
| stage | `tt.kind="stage"`, `tt.request_id`, `tt.stage` | `tt.success` |
| queue | `tt.kind="queue"`, `tt.request_id`, `tt.queue` | `tt.depth_at_start` |

Accepted field types:

- `tt.success`: optional bool; strings `"true"` and `"false"` are also accepted case-insensitively.
- `tt.depth_at_start`: optional non-negative integer. Do not record it with debug formatting.
- `tt.outcome`: optional non-empty string.
- `tt.kind`, `tt.request_id`, `tt.route`, `tt.stage`, and `tt.queue`: scalar strings.

Use a plain integer value for queue depth:

```text
tt.depth_at_start = depth_u64
```

`tt.depth_at_start = ?depth` may produce a debug-formatted value and be rejected.

For semantic string fields, `tt.kind = "request"` works, and `tt.kind = %kind` can work when `kind` displays as `request`. `tt.kind = ?kind` may record `"request"` with debug quoting and be rejected as unknown.

Missing request `tt.outcome` defaults to `ok` with a warning. Missing stage `tt.success` defaults to `true` with a warning.

Live tracing intake only tracks spans that are tailtriage candidates at span creation time. Declare `tt.*` fields when the span is created. If a value is filled later, declare the field with `tracing::field::Empty` and then call `span.record(...)`; adding brand-new `tt.*` fields later with `span.record(...)` is not supported.

```rust
let span = tracing::info_span!(
    "request",
    tt.kind = "request",
    tt.request_id = "req-1",
    tt.route = "/checkout",
    tt.outcome = tracing::field::Empty,
);
span.record("tt.outcome", "timeout");
```

## Strict vs non-strict

- Strict mode: malformed/incomplete `tt.*` span records fail import/session conversion.
- Non-strict mode: malformed/incomplete records are warned and skipped where implemented.
- Tracing owns source parsing, source warnings, raw recorder retention, semantic capture limits, and JSONL line/context reporting.
- Core owns generic completed-Run timing, duplicate request, parent-state, orphan, and containment policy after tracing has built source-valid candidate events.
- Strict tracing import preserves tracing source-format errors first, then delegates generic Run validation to core. Error-level core findings such as partial run-relative intervals, inverted run-relative intervals, duration mismatches, duplicate request IDs, orphan children, excluded parents, ambiguous parents, and precise containment failures reject strict import.
- Missing optional run-relative offsets are warning-only core findings and do not make strict import fail.
- Permissive tracing import delegates to core normalization. Core may exclude invalid generic evidence or clear invalid optional offsets while retaining authoritative durations; tracing does not apply a separate permissive mismatch or containment policy.

## Retention and drop behavior

- `DEFAULT_MAX_OPEN_SPANS` and `DEFAULT_MAX_COMPLETED_CANDIDATE_SPANS` are live-recorder memory caps for in-memory span tracking.
- `max_open_spans` bounds in-flight span tracking.
- `max_completed_candidate_spans` is a raw live-recorder memory cap for closed candidates before semantic conversion.
- Under raw-cap pressure, request roots are preserved preferentially when possible.
- Child stage/queue evidence may be dropped or evicted under that pressure and is surfaced through warnings plus `truncation.limits_hit`.
- Request/stage/queue semantic retention uses `CaptureMode`, `CaptureLimits`, and `CaptureLimitsOverride`.
- `completed_span_jsonl_path(...)` writes retained original source spans as stable span-shaped JSONL on shutdown only when at least one completed request is retained.
- Completed-span JSONL contains only retained original source records selected after tracing-specific source parsing, semantic retention, and core normalization. Private provenance joins core dispositions back to original source records; excluded, semantically dropped, and raw-unavailable records are absent and never revived.
- Direct conversion and JSONL import preserve supplied source order. Live completed-span output is section-grouped as requests, then stages, then queues, preserving recorder order within each section.
- The writer preserves source span identity and fields represented by `SpanRecord`: original span name, span ID, parent ID, non-`tt.*` fields, `tt.*` fields, Unix-ms bounds, optional run-relative offsets, and optional explicit duration.
- Completed-span JSONL replay is equivalent to direct conversion for normalized request/stage/queue evidence that JSONL can represent; it is not a complete Run artifact or production trace archive.
- It does not encode Run-only metadata, runtime/in-flight snapshots, lifecycle warnings, semantic truncation counters, raw-recorder drop counters, source file/line context, omitted-source diagnostics, or output-path failures. Run JSON remains the complete persisted triage artifact.
- For production workflows that need the complete persisted triage artifact including warnings/truncation metadata, prefer `run_json_path(...)`.
- Callers using JSONL-only export should inspect `session.shutdown().await?.warnings()` in the same process.
- This completed-span JSONL is a narrow retained-evidence export, not a generic tracing log stream and not OTel/OTLP.

## Runtime-pressure limitation

Tracing intake import and native capture share the same CaptureMode/CaptureLimits semantics for request/stage/queue evidence retention. Offline completed tailtriage tracing span JSONL import does not fabricate runtime snapshots. Runtime-pressure evidence still requires runtime snapshots/Tokio sampler coupling. Runtime-sensitive tracing contract parity uses deterministic/manual runtime snapshots and requires non-empty runtime snapshots, scenario-specific runtime field evidence, and the explicit disabled-background-sampler lifecycle warning (via `manual_runtime_snapshots()` + `record_runtime_snapshot(...)`). It does not rely on ambient sampler metadata/noise.
Persisted Run JSON intended for `tailtriage analyze` must include at least one completed request event; shutdown fails for persisted-output sessions when zero completed requests are retained. When intake/lifecycle warnings are available, that shutdown error includes warning summaries to help tracing-intake setup triage. In-process library snapshots may still be zero-request for inspection.

For `TracingSession`, runtime snapshot retention also uses the same core capture-limit model. Run metadata time bounds cover merged retained tracing evidence plus retained runtime snapshots, which supports triage interpretation but is not root-cause proof:

- configure retention with `mode(...)`, `capture_limits(...)`, or `capture_limits_override(...)`
- there is no tracing-specific `.max_runtime_snapshots(...)` session builder method
- tracing-only runs still do not fabricate runtime snapshots

## Examples

- `tailtriage-tracing/examples/live_session_to_run.rs`
- `tailtriage-tracing/examples/completed_span_jsonl_export.rs`


`TracingSession::builder(...).run_json_path(...)` persists merged Run JSON on `shutdown()`. Analysis remains a separate `tailtriage analyze <run.json>` step, and runtime-pressure evidence is triage input rather than root-cause proof.


## Migration from recorder/intake/Tokio split

Use `TracingSession::builder(service_name)` for every live tracing mode. Old recorder-only creation:

```rust,no_run
# use tailtriage_tracing::TracingSession;
let session = TracingSession::builder("checkout-service").build()?;
# Ok::<_, tailtriage_tracing::ImportError>(())
```

Old intake-session creation and output paths now use the same builder:

```rust,no_run
# use tailtriage_tracing::TracingSession;
let session = TracingSession::builder("checkout-service")
    .run_json_path("run.json")
    .completed_span_jsonl_path("completed-spans.jsonl")
    .build()?;
# Ok::<_, tailtriage_tracing::ImportError>(())
```

Optional Tokio-assisted capture stays on the same builder when the `tokio` feature is enabled:

```rust,no_run
# use std::time::Duration;
# use tailtriage_tracing::TracingSession;
# async fn example() -> Result<(), tailtriage_tracing::ImportError> {
let session = TracingSession::builder("checkout-service")
    .sampler_interval(Duration::from_millis(100))
    .build()?;
let snapshot = session.snapshot_run()?;
let final_run = session.shutdown().await?;
# let _ = (snapshot, final_run);
# Ok(())
# }
```

For deterministic Tokio validation, call `manual_runtime_snapshots()` and record explicit snapshots with `record_runtime_snapshot(...)`; shutdown is still `shutdown().await`.

## Live tracing session migration

Use `TracingSession` as the sole current live entry point for capture-to-Run workflows.

| Old usage | Final usage |
| --- | --- |
| `TracingRecorder::builder(...)` | `TracingSession::builder(...)` |
| `TracingIntakeSession::builder(...)` | `TracingSession::builder(...)` |
| `TracingTokioSession::builder(...).start()` | `TracingSession::builder(...).sampler_interval(...).build()` |
| `recorder_limits(...)` | `limits(...)` |
| synchronous `shutdown()?` | `shutdown().await?` |
| deterministic manual mode | `manual_runtime_snapshots()` plus `record_runtime_snapshot(...)?` |

Background runtime sampling is opt-in through `sampler_interval(...)`. Manual runtime collection is opt-in through `manual_runtime_snapshots()`. A plain live session still captures request, stage, and queue evidence without runtime collection, and `record_runtime_snapshot(...)?` returns a configuration error when runtime collection is not enabled. Manual snapshots may coexist with background sampling.

Run JSON remains the complete persisted artifact. Completed-span JSONL preserves retained original tracing sources for completed spans, but omits runtime snapshots and other Run-only state. Each output file is an independent transaction.
