# tailtriage-analyzer

`tailtriage-analyzer` is the in-process analysis/report crate for `tailtriage`.

It analyzes a completed in-memory `tailtriage_core::Run` (or a stable snapshot equivalent) and returns a typed triage report with evidence-ranked suspects and next checks.

Suspects are investigation leads, not proof of root cause.

## Installation

```bash
cargo add tailtriage-analyzer
```

If you also want JSON serialization of reports, add `serde_json` in your own crate:

```bash
cargo add serde_json
```

## How to obtain a `Run`

`tailtriage-analyzer` analyzes completed run data. Typical sources are:

- a completed `Run` captured in process with `tailtriage-core` (or the default `tailtriage` crate)
- a stable in-memory snapshot equivalent produced by your own flow

Artifact loading from disk is CLI-owned. Use `tailtriage-cli` when you want command-line analysis of saved artifacts.

## In-process API

- `analyze_run(&Run, AnalyzeOptions) -> Report`
- `Report` is the primary typed output
- `render_text(&Report)` renders human-readable triage output
- `serde_json` serialization is optional and user-provided

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

## Semantics and boundaries

- Analysis is batch/snapshot oriented over completed run data, not streaming.
- This crate does not capture instrumentation data.
- This crate does not load artifacts from disk.
- CLI artifact loading/validation is owned by `tailtriage-cli`.

## Report fields (overview)

`Report` includes request counts, latency percentiles, queue/service share summaries, warnings, evidence quality, ranked suspects, and optional supporting route/temporal sections.

See root docs for interpretation guidance:

- [`docs/diagnostics.md`](../docs/diagnostics.md)
- [`docs/user-guide.md`](../docs/user-guide.md)
