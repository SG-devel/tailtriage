# tailtriage-analyzer

`tailtriage-analyzer` is the in-process analysis/report crate for `tailtriage`.

Use it when you already have a completed `tailtriage_core::Run` in memory (or a stable snapshot equivalent) and want a typed triage report with evidence-ranked suspects and next checks.

## What this crate does

- analyzes one completed run/snapshot in batch
- returns a typed `Report`
- renders human-readable report text with `render_text`
- supports optional serde JSON serialization of the typed report

Suspects are investigation leads, not proof of root cause.

## Installation

```bash
cargo add tailtriage-analyzer
```

If you want JSON serialization in your app, also add:

```bash
cargo add serde_json
```

## How to obtain a `Run`

`Run` values come from capture/integration crates such as:

- `tailtriage` (recommended default crate)
- `tailtriage-core`
- `tailtriage-controller`
- `tailtriage-tokio`
- `tailtriage-axum`

Those crates produce completed runs and optional saved artifacts. This crate analyzes completed in-memory runs/snapshots.

## In-process API

Use `AnalyzeOptions::default()` as the normal path today; it keeps call sites stable while leaving room for future analyzer options.

`analyze_run` is currently infallible and returns `Report` directly.

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

## Output model and rendering

- `Report` is the primary typed output for Rust users.
- `render_text(&Report)` is for human-readable triage output.
- JSON is optional and uses serde serialization of the same typed `Report`.

## Semantics and boundaries

- batch/snapshot analysis of one completed run
- not streaming analysis
- does not load artifacts from disk
- artifact loading from disk is CLI-owned (`tailtriage-cli`)

For command-line analysis of saved artifacts, use `tailtriage-cli`.

## Report fields (overview)

`Report` includes request counts, latency percentiles, queue/service share summaries, warnings, evidence quality, ranked suspects, and optional supporting route/temporal sections.

See interpretation guidance:

- [`docs/diagnostics.md`](../docs/diagnostics.md)
- [`docs/user-guide.md`](../docs/user-guide.md)
