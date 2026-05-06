# tailtriage-analyzer

`tailtriage-analyzer` is the in-process analysis/report crate for `tailtriage`.

It analyzes a completed in-memory `tailtriage_core::Run` (or stable snapshot equivalent) and returns a typed triage `Report` with evidence-ranked suspects and next checks.

Suspects are investigation leads, not proof of root cause.

## When to use this crate

Use `tailtriage-analyzer` when you already have a completed `Run` in Rust code and want to analyze it in process.

Use `tailtriage-cli` when you want command-line analysis of saved run artifacts from disk.

## Installation

```bash
cargo add tailtriage-analyzer
```

If you also want JSON serialization in your application, add `serde_json` in your own crate:

```bash
cargo add serde_json
```

## How to obtain a `Run`

Capture/integration crates produce completed runs or saved artifacts:

- `tailtriage` (default entry point)
- `tailtriage-core`
- `tailtriage-controller`
- `tailtriage-tokio`
- `tailtriage-axum`

Typical in-process flow:

1. capture one bounded run in memory;
2. finish lifecycle and obtain a stable snapshot/completed run;
3. call `tailtriage-analyzer` on that completed data.

## In-process API

`AnalyzeOptions::default()` is the normal path today and leaves room for future analyzer options.

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

## Report outputs

- `Report` is the typed analyzer output model and should be your first integration target.
- `render_text(&Report)` renders a human-readable triage report.
- `serde_json::to_string_pretty(&report)` can serialize the same typed report as JSON when your app needs it.

JSON is optional for in-process code users.

## Semantics and boundaries

- Batch/snapshot analysis of one completed run.
- Not streaming analysis.
- Artifact loading from disk is CLI-owned (`tailtriage-cli`).

## Report fields (overview)

`Report` includes request counts, latency percentiles, queue/service share summaries, warnings, evidence quality, ranked suspects, and optional supporting route/temporal sections.

See root docs for interpretation guidance:

- [`docs/diagnostics.md`](../docs/diagnostics.md)
- [`docs/user-guide.md`](../docs/user-guide.md)
