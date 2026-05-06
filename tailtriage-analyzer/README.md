# tailtriage-analyzer

`tailtriage-analyzer` is the in-process analyzer/report crate for `tailtriage`.

It analyzes a completed in-memory `tailtriage_core::Run` (or a stable snapshot you already loaded in process) and returns a typed triage report with evidence-ranked suspects and next checks.

Suspects are leads, not proof of root cause.

## Installation

```bash
cargo add tailtriage-analyzer
```

## In-process API example

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

## Typed report model

`Report` is the analyzer contract for Rust code users.

It includes:

- latency percentiles and queue/service share summaries
- warnings and `evidence_quality`
- `primary_suspect` and `secondary_suspects`
- optional `inflight_trend`
- supporting `route_breakdowns` and `temporal_segments`

## Text rendering and JSON

- `render_text(&Report)` emits human-readable triage output.
- `serde_json::to_string_pretty(&report)` emits structured report JSON.

JSON is optional for code users; the primary API is typed Rust data.

## Batch/snapshot semantics

Analyzer semantics are currently batch/snapshot based:

- input is one completed run or stable snapshot
- output is one report for that input
- this crate does not do streaming analysis

## Scope boundary with CLI

Artifact loading/validation from files is owned by `tailtriage-cli`.

Use `tailtriage-cli` when you want command-line artifact loading and report emission. Use `tailtriage-analyzer` when you want in-process Rust analysis.
