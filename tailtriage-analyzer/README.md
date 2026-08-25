# tailtriage-analyzer

`tailtriage-analyzer` is the in-process analyzer/report crate for `tailtriage`.

Use this crate when you already have a completed `tailtriage_core::Run` in memory (or an equivalent stable snapshot) and want a typed triage report, text rendering, and canonical Report JSON rendering in your Rust process.

## What this crate does

- analyzes one completed run/snapshot in batch
- returns a typed `Report` with evidence-ranked suspects and next checks
- renders human-readable output with `render_text(&Report)`
- renders canonical Report JSON with `render_json(&Report)` and `render_json_pretty(&Report)`
- keeps analysis separate from Report rendering so configuration and serialization errors remain distinct

Suspects are investigation leads, not proof of root cause.

`tailtriage-analyzer` accepts any `tailtriage_core::Run` value. It is intended for completed/finalized captures or stable snapshots; callers that require finalized artifacts should validate that separately.

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
use tailtriage_analyzer::{render_json_pretty, render_text, analyze_run, AnalyzeOptions};
use tailtriage_core::Run;

fn render_report(run: &Run) -> Result<String, Box<dyn std::error::Error>> {
    let report = analyze_run(run, AnalyzeOptions::default())?;
    let text = render_text(&report);
    let json = render_json_pretty(&report)?;
    Ok(format!("{text}\n\n{json}"))
}
```

## Report contract

- `analyze_run` validates `AnalyzeOptions` and returns `AnalyzeConfigError` when they are semantically invalid
- `AnalyzeOptions` is the supported configuration model for current analyzer behavior
- `Report` is the typed analyzer output model and should be your primary integration surface
- `render_text` is for human-readable triage output
- `render_json` and `render_json_pretty` are canonical Report JSON renderers
- analysis and JSON rendering compose explicitly: call `analyze_run`, then `render_json` or `render_json_pretty`
- Report JSON is analyzer output and is distinct from raw Run artifact JSON input
- analyzer library default analysis is permissive: it analyzes core-normalized evidence and surfaces stable core issue-code warnings for excluded, repaired, or precision-limited completed-Run evidence; `tailtriage_core::validate_run_strict` rejects error-level generic core integrity failures when explicitly composed before analysis. The saved-artifact CLI independently enforces strict validation by default.

## Request ID contract

`request_id` is the per-run tailtriage identity of one completed logical request/work item. It must be unique among completed requests in one `Run`; stage and queue events must reuse that ID only for the same logical request. Duplicate completed IDs make request-scoped queue attribution, route breakdowns, temporal segmentation, and downstream-stage matching ambiguous.

Use `tailtriage_core::validate_run_strict` before `analyze_run` when you want to reject error-level generic core integrity failures before producing evidence-ranked suspects and next checks. Default analysis is permissive and warns instead of failing; missing optional run-relative precision is a warning-only limitation.

Users remain responsible for meaningful instrumentation and request-boundary semantics. The analyzer cannot know whether an external trace ID, retry ID, fanout ID, or batch ID identifies the correct logical request; convert repeating external IDs into unique tailtriage request IDs before analysis.

## Analyzer tuning options

Start with defaults:

```rust
use tailtriage_analyzer::AnalyzeOptions;

let options = AnalyzeOptions::default();
let _ = options;
```

In-process checked custom options:

```rust
use tailtriage_analyzer::{analyze_run, AnalyzeOptions};
use tailtriage_core::Run;

fn analyze_checked(run: &Run) -> Result<(), Box<dyn std::error::Error>> {
    let options = AnalyzeOptions::default()
        .with_queueing(|o| o.trigger_permille = 450);
    let report = analyze_run(run, options)?;
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
- `warnings[]` and `evidence_quality` describe interpretation limits.
- `route_breakdowns` and `temporal_segments`, when present, are supporting context only and do not override the global `primary_suspect`.
- Report JSON is analyzer output and is distinct from raw Run artifact JSON.

## Partial queue/stage evidence

Completed queue and stage distributions exclude partial observations. Partial durations are observed lower bounds: tailtriage observed the helper from first poll until Drop, not proof that the underlying operation completed, failed, or stopped. Partial evidence remains visible in event totals, evidence-quality limitations, top-level warnings, and suspect evidence.

Queue/service public p95 fields remain completed-only. A queue or downstream-stage suspect materially relying on an observed-lower-bound path cannot exceed medium confidence; partial evidence that does not affect selected eligibility or score does not automatically cap a completed candidate. Partial stage `success = false` is not interpreted as a completed operation failure.

Global, route, and temporal projections share this policy. Tracing imports remain completed-only. Completed-only Report JSON and text remain unchanged; mixed or partial Runs may change scores or ranking only when explicitly labeled lower-bound evidence is selected and qualified. Suspects remain triage leads, not root-cause proof.
