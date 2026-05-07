# tailtriage-analyzer

`tailtriage-analyzer` is the in-process analyzer/report crate for `tailtriage`.

Use this crate when you already have a completed `tailtriage_core::Run` in memory (or an equivalent stable snapshot) and want a typed triage report, text rendering, and optional JSON serialization in your Rust process.

## What this crate does

- analyzes one completed run/snapshot in batch
- returns a typed `Report` with evidence-ranked suspects and next checks
- renders human-readable output with `render_text(&Report)`
- `Report` implements serde::Serialize; add serde_json or another serde serializer in your application when you want encoded JSON output.

Suspects are investigation leads, not proof of root cause.

`tailtriage-analyzer` accepts any `tailtriage_core::Run` value. It is intended for completed/finalized captures or stable snapshots; callers that require finalized artifacts should validate that separately.

## Installation

```bash
cargo add tailtriage-analyzer
```

If you want JSON serialization output from your Rust code, also add:

```bash
cargo add serde_json
```

## How to obtain a `Run`

`tailtriage-analyzer` does not capture requests and does not load artifacts from disk.

Typical flow:

- capture/integration crates (`tailtriage`, `tailtriage-core`, `tailtriage-controller`, `tailtriage-tokio`, `tailtriage-axum`) produce completed runs or saved artifacts
- `tailtriage-analyzer` analyzes completed in-memory runs or stable snapshots in process
- `tailtriage-cli` loads saved artifacts from disk and invokes `tailtriage-analyzer`

## In-process API

```rust
use tailtriage_analyzer::{analyze_run, render_text, AnalyzeOptions};
use tailtriage_core::Run;

fn render_report(run: &Run) -> Result<String, serde_json::Error> {
    let report = analyze_run(run, AnalyzeOptions::default());
    let text = render_text(&report);
    let json = serde_json::to_string_pretty(&report)?;
    Ok(format!("{text}\n\n{json}"))
}
```

## Report contract

- `analyze_run` currently returns `Report` directly and is currently infallible
- `AnalyzeOptions::default()` is the normal path today and leaves room for future analyzer options
- `Report` is the typed analyzer output model and should be your primary integration surface
- `render_text` is for human-readable triage output
- serde JSON for `Report` is optional and requires user-side `serde_json`

## Semantics and boundaries

- batch/snapshot analysis of one completed run
- not streaming analysis
- artifact loading from disk is CLI-owned (`tailtriage-cli`)

## Report fields (overview)

`Report` includes request counts, latency percentiles, queue/service share summaries, warnings, evidence quality, ranked suspects, and optional supporting route/temporal sections.

See root docs for interpretation guidance:

- [`docs/diagnostics.md`](../docs/diagnostics.md)
- [`docs/user-guide.md`](../docs/user-guide.md)

## Migration note

```rust
// Old pre-0.1.x API was hosted in the CLI crate.
// Use the analyzer crate directly for in-process analysis/report APIs.

use tailtriage_analyzer::{analyze_run, render_text, AnalyzeOptions};
```
