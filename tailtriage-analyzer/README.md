# tailtriage-analyzer

`tailtriage-analyzer` is the in-process analyzer/report crate for `tailtriage`.

Use this crate when you already have a completed `tailtriage_core::Run` in memory (or an equivalent stable snapshot) and want a typed triage report, text rendering, and canonical Report JSON rendering in your Rust process.

## What this crate does

- analyzes one completed run/snapshot in batch
- returns a typed `Report` with evidence-ranked suspects and next checks
- renders human-readable output with `render_text(&Report)`
- renders canonical Report JSON with `render_json(&Report)` and `render_json_pretty(&Report)`
- provides analyze+render helpers: `analyze_run_json` and `analyze_run_json_pretty`

Suspects are investigation leads, not proof of root cause.

`tailtriage-analyzer` accepts any `tailtriage_core::Run` value. It is intended for completed/finalized captures or stable snapshots; callers that require finalized artifacts should validate that separately. Default analysis is permissive: duplicate completed `request_id` values produce warnings and evidence-quality limitations rather than rejecting the run. Use `validate_run_artifact_strict(...)` or `analyze_run_strict(...)` when duplicate completed IDs or orphan stage/queue IDs should fail fast.

## Installation

```bash
cargo add tailtriage-analyzer
```

You also need a capture crate that provides `tailtriage_core::Run`, such as `tailtriage` or `tailtriage-core`.

## How to obtain a `Run`

`tailtriage-analyzer` does not capture requests and does not load artifacts from disk.

Typical flow:

- capture/integration crates (`tailtriage`, `tailtriage-core`, `tailtriage-controller`, `tailtriage-tokio`, `tailtriage-axum`, `tailtriage-tracing`) produce completed runs or saved artifacts
- `tailtriage-analyzer` analyzes completed in-memory runs or stable snapshots in process
- `tailtriage-cli` loads saved artifacts from disk and invokes `tailtriage-analyzer`

## In-process API

```rust
use tailtriage_analyzer::{analyze_run, render_json_pretty, render_text, AnalyzeOptions};
use tailtriage_core::Run;

fn render_report(run: &Run) -> Result<String, Box<dyn std::error::Error>> {
    let report = analyze_run(run, AnalyzeOptions::default());
    let text = render_text(&report);
    let json = render_json_pretty(&report)?;
    Ok(format!("{text}\n\n{json}"))
}
```

## Report contract

- `analyze_run` currently returns `Report` directly and is currently infallible
- `AnalyzeOptions::default()` is the normal path today and leaves room for future analyzer options
- `Report` is the typed analyzer output model and should be your primary integration surface
- `render_text` is for human-readable triage output
- `render_json` and `render_json_pretty` are canonical Report JSON renderers
- `analyze_run_json` and `analyze_run_json_pretty` combine analysis + canonical JSON rendering
- Report JSON is analyzer output and is distinct from raw Run artifact JSON input

## Analyzer tuning options

Start with defaults:

```rust
use tailtriage_analyzer::AnalyzeOptions;

let options = AnalyzeOptions::default();
let _ = options;
```

In-process checked custom options:

```rust
use tailtriage_analyzer::{try_analyze_run, AnalyzeOptions};
use tailtriage_core::Run;

fn analyze_checked(run: &Run) -> Result<(), Box<dyn std::error::Error>> {
    let options = AnalyzeOptions::default()
        .with_queueing(|o| o.trigger_permille = 450);
    let report = try_analyze_run(run, options)?;
    let _ = report;
    Ok(())
}
```

TOML parsing example:

```rust
use tailtriage_analyzer::AnalyzeOptions;

let input = r#"
[analyzer]
schema_version = 1

[analyzer.queueing]
trigger_permille = 450
"#;

let options = AnalyzeOptions::from_toml_str(input)?;
# Ok::<(), tailtriage_analyzer::AnalyzeConfigError>(())
```

Report transparency behavior:

- default options omit `analyzer_config` from Report JSON
- non-default options include `analyzer_config` with active non-default overrides
- tuning changes interpretation of captured evidence; it does not change capture artifacts

## Semantics and boundaries

- batch/snapshot analysis of one completed run
- not streaming analysis
- artifact loading from disk is CLI-owned (`tailtriage-cli`)
- CLI `--format json` uses the same canonical pretty Report JSON rendering path

## Report fields (overview)

`Report` includes request counts, latency percentiles, queue/service share summaries, warnings, evidence quality, ranked suspects, and optional supporting route/temporal sections.

## How to interpret a report

- `primary_suspect` is the strongest triage lead for the analyzed run, not proof of root cause.
- `secondary_suspects` are lower-ranked leads worth checking when evidence is close or the primary lead does not explain the incident.
- `evidence[]` explains why a suspect was ranked.
- `next_checks[]` gives targeted follow-up actions.
- `score` ranks suspects inside one report; it is not a probability.
- `confidence` is ranking strength and may be capped by missing, sparse, partial, or truncated evidence.
- `warnings[]` and `evidence_quality` describe interpretation limits, including duplicate completed `request_id` values that can make request-scoped attribution ambiguous.
- `route_breakdowns` and `temporal_segments`, when present, are supporting context only and do not override the global `primary_suspect`.
- Report JSON is analyzer output and is distinct from raw Run artifact JSON.

## Migration note

```rust
// Old pre-0.1.x API was hosted in the CLI crate.
// Use the analyzer crate directly for in-process analysis/report APIs.

use tailtriage_analyzer::{analyze_run, render_text, AnalyzeOptions};
```
