# tailtriage-analyzer

`tailtriage-analyzer` analyzes an already completed `tailtriage_core::Run` and produces a typed triage report.

It is designed for in-process report generation from in-memory runs. It does not load run artifacts from disk, and it does not write run artifacts.

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

- `Report` is the primary typed output for analyzer/report logic.
- `render_text(&Report)` produces human-readable output.
- `serde_json::to_string_pretty(&report)` produces analysis report JSON.

## Run artifact JSON vs analysis report JSON

These are distinct outputs and both remain supported:

1. **Run artifact JSON**: raw captured run data produced by capture/shutdown or artifact-writing workflows. This remains part of capture/core/CLI artifact workflows and can still be analyzed later by the CLI.
2. **Analysis report JSON**: output from analyzing a `Run`, represented by `tailtriage_analyzer::Report` and serialized with serde.

Direct analyzer usage does not replace artifact generation and does not require parsing CLI stdout.

## Execution model

Current analyzer semantics are batch/snapshot based for one completed run, not streaming.
