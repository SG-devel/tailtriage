# tailtriage-tracing

`tailtriage-tracing` converts completed tracing-shaped `tt.*` evidence into a
standard Tailtriage `Run`. Choose the intake job that matches the evidence you
already have:

| Reader job | Path | Feature selection |
| --- | --- | --- |
| I have custom completed records | typed `SpanRecord` conversion | `--no-default-features` |
| I have saved Tailtriage completed-span JSONL | stable-wrapper JSONL import | default / `jsonl` |
| I have live supported `tracing` spans | `TracingSession` | `live` (implies `jsonl`) |

The optional `tokio` feature adds runtime sampler/manual runtime-snapshot
coupling to the live path; it implies `live`, and therefore `jsonl`. It is not a
fourth tracing-intake model.

## 1. Custom typed completed records

Install only the always-available typed conversion surface:

```bash
cargo add tailtriage-tracing --no-default-features
```

```rust
use tailtriage_tracing::{
    run_from_span_records, ImportOptions, SpanRecord, TT_KIND, TT_REQUEST_ID, TT_ROUTE,
};

let request = SpanRecord::new("request", 1_700_000_000_000, 1_700_000_000_005)
    .with_field(TT_KIND, "request")
    .with_field(TT_REQUEST_ID, "req-1")
    .with_field(TT_ROUTE, "/checkout")
    .with_duration_us(5_000);
let imported = run_from_span_records(
    [request],
    ImportOptions::new("checkout-service"),
)?;
assert_eq!(imported.run().requests.len(), 1);
for warning in imported.warnings() {
    eprintln!("{}", warning.message());
}
# Ok::<(), tailtriage_tracing::ImportError>(())
```

Request, stage, and queue evidence for one logical work item must use the same
`tt.request_id`, unique within the `Run`. A distributed trace ID is not
automatically suitable when it repeats across retries, attempts, fanout
branches, or batch items. `SpanRecord` is the caller-built input;
`ImportedRun` and `ImportWarning` are result/accessor-owned outputs.

## 2. Saved stable-wrapper completed-span JSONL

The default configuration enables `jsonl`:

```bash
cargo add tailtriage-tracing
```

```rust
# #[cfg(feature = "jsonl")]
# fn example() -> Result<(), tailtriage_tracing::ImportError> {
use std::io::Cursor;
use tailtriage_tracing::{import_jsonl_reader, ImportOptions};

let input = concat!(
    r#"{"format":"tailtriage.tracing-span.v1","span":{"id":null,"parent_id":null,"name":"request","fields":{"tt.kind":"request","tt.request_id":"req-1","tt.route":"/checkout"},"started_at_unix_ms":1700000000000,"finished_at_unix_ms":1700000000005,"duration_us":5000}}"#,
    "\n",
);
let imported = import_jsonl_reader(
    Cursor::new(input.as_bytes()),
    ImportOptions::new("checkout-service"),
)?;
assert_eq!(imported.run().requests.len(), 1);
let _warnings = imported.warnings();
# Ok(())
# }
# #[cfg(feature = "jsonl")]
# example().unwrap();
```

Only the `tailtriage.tracing-span.v1` wrapper is accepted. Raw `SpanRecord`
JSON, unversioned compatibility envelopes, ordinary tracing formatter JSON,
and generic tracing logs are unsupported. This is completed-span intake, not a
log stream, and timing is never inferred from line receipt.

## 3. Live supported tracing spans

The example uses Tokio as its executor, but does **not** enable Tailtriage's
Tokio runtime coupling. Every crate named by the application is direct:

```bash
cargo add tailtriage-tracing --no-default-features --features live
cargo add tracing tracing-subscriber
cargo add tokio --features macros,rt
```

```rust
# #[cfg(feature = "live")]
use tailtriage_tracing::TracingSession;
# #[cfg(feature = "live")]
use tracing_subscriber::prelude::*;

# #[cfg(feature = "live")]
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let session = TracingSession::builder("checkout-service").build()?;
    let subscriber = tracing_subscriber::registry().with(session.layer());

    // Scoped installation is useful for a local example. Applications normally
    // compose the layer into their one process-wide, startup-only subscriber.
    tracing::subscriber::with_default(subscriber, || {
        let request = tracing::info_span!(
            "request",
            tt.kind = "request",
            tt.request_id = "req-1",
            tt.route = "/checkout",
            tt.outcome = "ok",
        );
        let _entered = request.enter();
        // request work
    }); // the entered guard and final span handle close here

    let imported = session.shutdown().await?;
    assert_eq!(imported.run().requests.len(), 1);
    let _warnings = imported.warnings();
    Ok(())
}
# #[cfg(not(feature = "live"))]
# fn main() {}
```

Candidate discovery happens when a span is created. Declare every later-filled
`tt.*` field at creation (for example with `tracing::field::Empty`) before using
`span.record(...)`; `record` cannot introduce a brand-new field. Completion is
span close/drop, not enter/exit. Close intended evidence before `shutdown()`.

## Evidence, timing, retention, and output boundaries

The semantic protocol retains completed request, stage, and queue evidence.
When explicit `duration_us` is absent, conversion prefers a complete ordered
run-relative microsecond interval, then falls back to the saturating Unix-ms
delta. Optional run-relative intervals—not coarse wall-clock anchors—enable
core precision and containment validation.

Live `RecorderLimits` bound raw open candidates and closed candidates before
conversion. `CaptureMode`, `CaptureLimits`, and `CaptureLimitsOverride` bound
semantic `Run` evidence. These independent limits can produce warnings and
truncation; increasing one does not repair drops at the other.

`run_json_path(...)` writes the complete retained `Run`, including applicable
Run-only warnings, truncation, metadata, and runtime evidence.
`completed_span_jsonl_path(...)` writes retained original `SpanRecord` sources
in the stable wrapper. It is not a complete Run, generic trace archive, tracing
log, OTel/OTLP stream, or replay of evidence never retained. If both outputs are
configured, shutdown writes completed-span JSONL first and Run JSON second as
independent temp/rename transactions; a later failure does not roll back an
earlier file.

Tracing request/stage/queue intake does not fabricate runtime-pressure
evidence, and offline completed-span JSONL contains no runtime snapshots.
Runtime evidence requires native runtime snapshots or the live session's
`tokio`-feature coupling. Missing runtime evidence limits runtime-pressure
diagnosis. Evidence-ranked suspects and next checks are leads, not proof of root
cause.

With the crate's **`tokio` feature only**,
`TracingSessionBuilder::sampler_interval(...)` enables background sampling,
`manual_runtime_snapshots()` enables explicit storage without automatic
sampling, and `TracingSession::record_runtime_snapshot(...)` supplies a
snapshot. Merely running a plain `live` session on a Tokio executor does not
enable those APIs or runtime collection.
