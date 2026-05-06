# tailtriage-analyzer

`tailtriage-analyzer` is the in-process analyzer/report library for `tailtriage`.

Use it to analyze a completed `tailtriage_core::Run` (or a stable in-memory snapshot of one) and produce a typed triage report with evidence-ranked suspects and next checks.

Suspects are leads, not proof of root cause.

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
let report_json = serde_json::to_string_pretty(&report)?;
# let _ = (text, report_json);
# Ok(())
# }
```

## Typed report model

`Report` is the analyzer contract for downstream Rust code:

- latency and share summaries
- evidence-quality summary
- primary and secondary suspects
- optional route breakdowns
- optional temporal segments
- warnings and interpretation limits

Use `Report` directly for typed integration and tooling.

## Rendering and JSON

- `render_text(&Report)` emits human-readable report text.
- `serde_json::to_string_pretty(&report)` emits structured report JSON.

JSON is optional for Rust code users; the typed `Report` model is the primary in-process API.

## Batch/snapshot semantics

Analyzer execution is batch/snapshot based for one completed run (or stable snapshot). It is not live streaming analysis.

## Artifact loading ownership

Artifact loading/validation is CLI-owned. `tailtriage-analyzer` does not load run artifacts from disk.

For file-based analysis from artifacts, use `tailtriage-cli`.

## Migration note

```rust
// Old pre-0.1.x API, no longer the supported library analyzer path:
use tailtriage_cli::analyze::{analyze_run, render_text};

// New:
use tailtriage_analyzer::{analyze_run, render_text, AnalyzeOptions};
```
