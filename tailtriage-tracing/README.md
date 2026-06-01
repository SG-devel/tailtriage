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

Use this path when your service already uses Rust `tracing` and already has stable per-request correlation IDs. New integrations without existing tracing/correlation should start with native `tailtriage` capture first.

This crate converts tracing-shaped request, stage, and queue evidence into standard `tailtriage_core::Run` artifacts for the normal `tailtriage analyze` workflow. It is not a tracing backend.

For one work item, every request, stage, and queue span must carry the same `tt.request_id`. Child stage/queue evidence is correlated to retained request evidence by `tt.request_id`; missing or inconsistent IDs cause child evidence to be skipped or weakened.

## Feature flags

- Base crate: typed `SpanRecord`, `ImportOptions`, `ImportedRun`, semantic constants, and `run_from_span_records(...)`.
- Default (`jsonl`): JSONL import APIs and stable wrapper parsing.
- `live`: enables `TracingRecorder`, `TailtriageLayer`, and `TracingIntakeSession`.
- `tokio`: enables `TracingTokioSession` runtime-sampler coupling and includes `live` (background sampler on by default; deterministic runs can call `disable_background_sampler()` and inject snapshots manually).

CLI offline import workflows only need JSONL import support and do not require the live `tracing_subscriber` layer dependency.

For live tracing intake sessions, `tailtriage-tracing` enables optional dependencies behind feature flags, but applications that call `tracing::...` or `tracing_subscriber::...` directly still need explicit app dependencies:

```bash
cargo add tailtriage-tracing --features live
cargo add tracing tracing-subscriber
cargo add tokio --features macros,rt-multi-thread
```

If you use Tokio runtime sampler coupling via `TracingTokioSession`, use:

```bash
cargo add tailtriage-tracing --features tokio
cargo add tracing tracing-subscriber
cargo add tokio --features macros,rt-multi-thread
```

## Recommended live session setup (`live` feature)

```rust,no_run
use tailtriage_tracing::TracingIntakeSession;
use tracing::Instrument as _;
use tracing_subscriber::prelude::*;

async fn work() {
    // Your request work goes here.
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let session = TracingIntakeSession::builder("checkout-service")
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

    let imported = session.shutdown()?;
    let _ = imported;
    Ok(())
}
```

Install the tailtriage layer beside your existing tracing layers in the application's normal process-wide subscriber setup; tailtriage augments your tracing pipeline rather than replacing it.

If your service already builds a subscriber in startup code, compose `session.layer()` onto that subscriber and install it once with your normal global install path (for example `.init()` during binary startup, or `tracing::subscriber::set_global_default(...)` with explicit error handling when needed).

`set_default` is scoped to the current thread and guard lifetime; service startup should install the tailtriage layer in the process-wide subscriber setup.

## Timing model

- Imported or live tracing evidence is converted to normal tailtriage Run timing fields.
- Durations are the authoritative elapsed-time evidence when present.
- Unix-ms timestamps are wall-clock anchors and may be coarser than durations.
- Ordinary tracing log timestamps are not enough for completed-span import; completed spans need explicit start/end timing and semantic tt.* fields.

## Direct Run JSON path

Use `run_json_path(...)` when you want to skip a separate import step and write Run JSON through the same robust writer path used by native capture sinks:

```bash
tailtriage analyze target/tailtriage-examples/checkout.run.json
```

For both `TracingIntakeSession` and `TracingTokioSession`, persisted-output shutdown
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
remains a replay/debug export, not trace archival.

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

Record semantic `tt.*` fields (`tt.kind`, `tt.request_id`, `tt.route`, `tt.stage`, `tt.queue`) as plain scalar strings (string literals or display-formatted scalar strings). Do not use debug formatting for semantic `tt.*` fields.

For example: `tt.kind = "request"` works, `tt.kind = %kind` can work when `kind` displays as `request`, but `tt.kind = ?kind` may record `"request"` (debug quoting) and be rejected as unknown.

Missing request `tt.outcome` defaults to `ok` with a warning.
If present, request `tt.outcome` must be a string and cannot be empty/whitespace-only; accepted custom labels are preserved exactly.
Missing stage `tt.success` defaults to `true` with a warning.

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
- Duration consistency rule: when `duration_us` is supplied, it is preferred as the authoritative elapsed-time evidence. If `duration_us` differs from the timestamp-derived duration by more than `2_000` microseconds, strict import rejects the mismatch, while non-strict import warns and keeps `duration_us`; Unix timestamps remain wall-clock anchors. When `duration_us` is absent, conversion derives duration from wall-clock bounds as `(finished_at_unix_ms - started_at_unix_ms) * 1000`.
- Child stage/queue containment uses a fixed `2` ms tolerance when checking whether child intervals fall inside retained request intervals.
- That `2` ms containment tolerance is not configurable in this release (no CLI/API knob).

## Retention and drop behavior

- `DEFAULT_MAX_OPEN_SPANS` and `DEFAULT_MAX_COMPLETED_CANDIDATE_SPANS` are live-recorder memory caps for in-memory span tracking.
- `max_open_spans` bounds in-flight span tracking.
- `max_completed_candidate_spans` is a raw live-recorder memory cap for closed candidates before semantic conversion.
- Under raw-cap pressure, request roots are preserved preferentially when possible.
- Child stage/queue evidence may be dropped or evicted under that pressure and is surfaced through warnings plus `truncation.limits_hit`.
- Request/stage/queue semantic retention uses `CaptureMode`, `CaptureLimits`, and `CaptureLimitsOverride`.
- `completed_span_jsonl_path(...)` writes retained tailtriage semantic evidence as stable span-shaped JSONL on shutdown only when at least one completed request is retained.
- Completed-span JSONL is retained-evidence replay/debug export for the same request/stage/queue evidence path through `tailtriage import`; it is not a production trace archive.
- It does not preserve lifecycle warnings, truncation counters, original span IDs, parent IDs, original span names, or non-`tt.*` fields.
- For production workflows that need the complete persisted triage artifact including warnings/truncation metadata, prefer `run_json_path(...)`.
- Callers using JSONL-only export should inspect `session.shutdown()?.warnings()` in the same process.
- This completed-span JSONL is a narrow retained-evidence export, not a generic tracing log stream and not OTel/OTLP.

## Runtime-pressure limitation

Tracing intake import and native capture share the same CaptureMode/CaptureLimits semantics for request/stage/queue evidence retention. Offline completed tailtriage tracing span JSONL import does not fabricate runtime snapshots. Runtime-pressure evidence still requires runtime snapshots/Tokio sampler coupling. Runtime-sensitive tracing contract parity uses deterministic/manual runtime snapshots and requires non-empty runtime snapshots, scenario-specific runtime field evidence, and the explicit disabled-background-sampler lifecycle warning (via `disable_background_sampler()` + `record_runtime_snapshot(...)`). It does not rely on ambient sampler metadata/noise.
Persisted Run JSON intended for `tailtriage analyze` must include at least one completed request event; shutdown fails for persisted-output sessions when zero completed requests are retained. When intake/lifecycle warnings are available, that shutdown error includes warning summaries to help tracing-intake setup triage. In-process library snapshots may still be zero-request for inspection.

For `TracingTokioSession`, runtime snapshot retention also uses the same core capture-limit model. Run metadata time bounds cover merged retained tracing evidence plus retained runtime snapshots, which supports triage interpretation but is not root-cause proof:

- configure retention with `mode(...)`, `capture_limits(...)`, or `capture_limits_override(...)`
- there is no tracing-specific `.max_runtime_snapshots(...)` session builder method
- tracing-only runs still do not fabricate runtime snapshots

## Examples

- `tailtriage-tracing/examples/live_session_to_run.rs`
- `tailtriage-tracing/examples/completed_span_jsonl_export.rs`


`TracingTokioSession::builder(...).run_json_path(...)` persists merged Run JSON on `shutdown()`. Analysis remains a separate `tailtriage analyze <run.json>` step, and runtime-pressure evidence is triage input rather than root-cause proof.
