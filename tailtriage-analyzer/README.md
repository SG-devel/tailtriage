# tailtriage-analyzer

`tailtriage-analyzer` is the in-process analyzer/report crate for `tailtriage`.

It analyzes a completed in-memory `tailtriage_core::Run` (or stable snapshot equivalent) and returns a typed triage report with evidence-ranked suspects and next checks.

## Installation

```bash
cargo add tailtriage-analyzer
```

## In-process API

```rust
use tailtriage_analyzer::{analyze_run, render_text, AnalyzeOptions};
# use tailtriage_core::Run;
# fn example(run: Run) -> Result<(), serde_json::Error> {
let report = analyze_run(&run, AnalyzeOptions::default());
let text = render_text(&report);
let json = serde_json::to_string_pretty(&report)?;
# let _ = (text, json);
# Ok(())
# }
```

## Report contract

- `Report` is the typed analyzer output model.
- `render_text(&Report)` renders a human-readable triage report.
- `serde_json::to_string_pretty(&report)` serializes the same typed report as JSON.

Suspects are investigation leads, not proof of root cause.

## Semantics and boundaries

- Batch/snapshot analysis of one completed run.
- Not streaming analysis.
- Artifact loading from disk is CLI-owned (`tailtriage-cli`).

## Report fields (overview)

`Report` includes request counts, latency percentiles, queue/service share summaries, warnings, evidence quality, ranked suspects, and optional supporting route/temporal sections.

See root docs for interpretation guidance:

- [`docs/diagnostics.md`](../docs/diagnostics.md)
- [`docs/user-guide.md`](../docs/user-guide.md)

## Migration note

```rust
// Old pre-0.1.x API was hosted in the CLI crate.
// Use the analyzer crate directly for in-process analysis/report APIs.

// New:
use tailtriage_analyzer::{analyze_run, render_text, AnalyzeOptions};
```
