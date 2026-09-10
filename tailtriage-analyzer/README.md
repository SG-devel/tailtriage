# tailtriage-analyzer

`tailtriage-analyzer` turns one typed in-memory `tailtriage_core::Run` into a typed triage `Report`. It performs batch/snapshot analysis, ranks evidence-backed suspects, suggests next checks, and renders Report output. Suspects are investigation leads, not proof of root cause.

The analyzer does not capture requests or load saved Run files. Capture/import code supplies the Run; saved-artifact loading is a CLI responsibility.

## Installation

```bash
cargo add tailtriage-analyzer tailtriage-core
```

Both are direct dependencies because the in-process API names types from both crates.

## In-process analysis

This complete example builds a deliberately small typed Run, adjusts an option directly, performs checked analysis, and renders both human-readable and canonical JSON output:

```rust
use tailtriage_analyzer::{
    analyze_run, render_json, render_json_pretty, render_text, AnalyzeOptions,
};
use tailtriage_core::{
    CaptureMode, EffectiveCoreConfig, Run, RunMetadata, UnfinishedRequests,
};

fn render_report() -> Result<(), Box<dyn std::error::Error>> {
    let metadata = RunMetadata {
        run_id: "run-1".into(),
        service_name: "checkout".into(),
        service_version: None,
        started_at_unix_ms: 1,
        finalized_at_unix_ms: None,
        mode: CaptureMode::Light,
        effective_core_config: Some(EffectiveCoreConfig {
            mode: CaptureMode::Light,
            capture_limits: CaptureMode::Light.core_defaults(),
            strict_lifecycle: false,
        }),
        effective_tokio_sampler_config: None,
        host: None,
        pid: None,
        lifecycle_warnings: Vec::new(),
        unfinished_requests: UnfinishedRequests::default(),
        run_end_reason: None,
    };
    let run = Run::new(metadata);

    let mut options = AnalyzeOptions::default();
    options.queueing.trigger_permille = 450;

    let report = analyze_run(&run, options)?;
    println!("{}", render_text(&report));
    println!("{}", render_json(&report)?);
    println!("{}", render_json_pretty(&report)?);
    Ok(())
}

fn main() {
    if let Err(error) = render_report() {
        eprintln!("analysis or rendering failed: {error}");
    }
}
```

`analyze_run` is the checked analysis call. It validates `AnalyzeOptions`; invalid direct mutation returns `AnalyzeConfigError`. Direct assignment itself is not checked. Start from `AnalyzeOptions::default()` and mutate nested fields when tuning is necessary. Non-default semantic options are surfaced in the Report's optional `analyzer_config` summary.

`render_text` produces a human-readable triage view. `render_json` and `render_json_pretty` serialize the same typed Report in compact and pretty canonical JSON. Rendering is separate from analysis, and Report JSON is analyzer output—not Run artifact JSON input.

## Input and validation boundary

Library analysis permissively normalizes generic Run evidence through `tailtriage-core`. Normalization can exclude or canonicalize invalid evidence and exposes stable validation warnings and limitations; it does not turn arbitrary input into strict validity. Missing optional precision is a limitation, not by itself an error-level strict failure.

Call `tailtriage_core::validate_run_strict(&run)` before `analyze_run` when your application requires explicit strict generic Run acceptance. A saved-artifact CLI has its own strict-by-default loading boundary; that boundary is not behavior of `analyze_run`.

A `request_id` identifies one logical request/work item within a Run. Completed request IDs should be unique, and queue/stage events should reuse an ID only for that request. Callers remain responsible for meaningful request boundaries and instrumentation; repeating retry, fanout, trace, or batch identifiers should be made unique before capture or analysis.

## Reading the Report

- `primary_suspect` is the strongest global lead; `secondary_suspects` are alternatives.
- `score` ranks retained evidence inside this report. It is neither probability nor cross-run severity.
- `confidence` describes evidence/ranking support, not causal certainty.
- `evidence` explains the ranking; `next_checks` proposes targeted follow-up checks.
- `warnings` and `evidence_quality` expose sparse, missing, partial, or truncated evidence limits.
- Route and temporal sections are supporting context and do not override the global primary suspect.

Completed queue/stage distributions use completed evidence. A partial queue/stage observation is only a lower bound from first poll until Drop. Drop does not prove external operation completion, failure, cancellation, or that underlying work stopped. A selected queue/downstream candidate that materially relies on lower-bound evidence cannot exceed Medium confidence under current policy.

Use the output to choose one next check and compare a follow-up capture. Do not treat a suspect or mitigation movement as proof of root cause.
