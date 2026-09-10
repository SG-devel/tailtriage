# User guide

This guide completes the default native capture journey before introducing optional integrations. `tailtriage` produces evidence-ranked suspects and next checks; suspects are leads, not proof of root cause.

## 1) Install

Use the façade for capture and the CLI for saved-Run analysis. This guide's example names Tokio directly, so Tokio must also be a direct dependency:

```bash
cargo add tailtriage
cargo add tokio --features macros,rt,time
cargo install tailtriage-cli
```

## 2) Instrument one meaningful request

Put queue instrumentation around a real wait before work starts and stage instrumentation around a database call, downstream request, or meaningful handler/service work.

```rust,no_run
use std::time::Duration;
use tailtriage::Tailtriage;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let run = Tailtriage::builder("checkout-service")
        .output("tailtriage-run.json")
        .build()?;

    let started = run.begin_request("/checkout");
    let request = started.handle.clone();

    request
        .queue("checkout_worker")
        .await_on(tokio::time::sleep(Duration::from_millis(6)))
        .await;

    let workload = request
        .stage("inventory_lookup")
        .await_on(async {
            tokio::time::sleep(Duration::from_millis(8)).await;
            Ok::<(), std::io::Error>(())
        })
        .await;

    let workload = started.completion.finish_result(workload);
    run.shutdown()?;
    workload?;
    Ok(())
}
```

`started.handle` records evidence; `started.completion` owns completion. Finish exactly once after measured work. `shutdown()` finalizes and persists the capture but does not invent completion. Keep fallible work in a value until completion and shutdown have occurred, rather than using an early `?` that can skip either step. Do not finalize while spawned request tasks or owned completion tokens remain active.

Within a Run, each completed logical request/work item needs one unique tailtriage `request_id`; its queue and stage evidence must reuse that ID only for the same logical request.

## 3) Analyze the finalized Run

Start with text output:

```bash
tailtriage analyze tailtriage-run.json
```

Saved-Run CLI analysis uses strict generic core validation: an error-level core finding blocks report generation, while warning-only precision limitations are accepted. `--allow-ambiguous-artifact` explicitly selects permissive canonical normalization and reports the original issues. Tracing import `--strict` separately controls tracing-source conversion. In-process analyzer APIs use permissive generic Run normalization by default.

A small capture may legitimately yield `insufficient_evidence`. The workflow guarantees neither a particular suspect nor a root cause.

## 4) Interpret evidence and limitations

Read the result in this order:

1. primary suspect—the strongest lead
2. supporting evidence
3. warnings and evidence-quality limitations
4. one `next_check`

Completed queue/stage distributions use completed observations. A partial helper observation is a lower bound from first poll until helper Drop; Drop does not prove the underlying operation completed, failed, was cancelled, or stopped. When selected evidence materially relies on a lower bound, confidence can be capped.

For a practical walkthrough, use the [analyzer guide](analyzer-guide.md). Consult the [diagnostics reference](diagnostics.md) only for exact scoring, ordering, fields, and evidence-limit mechanics.

## 5) Pick one next check and rerun

Change or instrument one thing suggested by the strongest lead, then capture again under comparable conditions. If the report says `insufficient_evidence`, add one useful missing boundary: a suspected queue, a downstream stage, or explicit runtime sampling when runtime pressure is the open question. Compare evidence movement; do not treat it as formal causal proof.

This completes the default journey: install, instrument, complete, shut down, analyze, interpret, choose one check, and rerun.

## Optional paths

### Controller for long-lived services

Choose `tailtriage::controller::TailtriageController` for repeated bounded arm/disarm capture windows. `disable()` finalizes the current generation and is reversible; `shutdown()` is terminal. The [operations guide](operations.md) owns rollout and lifecycle choices, and the [`tailtriage-controller` README](../tailtriage-controller/README.md) owns configuration details.

### Tokio runtime sampling

Choose runtime sampling when request timing alone cannot distinguish executor or blocking-pool pressure. The default `tokio` feature makes `tailtriage::tokio` available, but sampling never starts automatically: `CaptureMode` and `Tailtriage::builder` do not start it. Start `RuntimeSampler` explicitly inside an active Tokio runtime. Exact cadence, retention, and startup contracts live in the [`tailtriage-tokio` README](../tailtriage-tokio/README.md) and Rustdoc.

### Axum request boundaries

Enable the façade's `axum` feature for middleware-owned request start/finish and request-handle extraction:

```bash
cargo add tailtriage --features axum
```

The adapter does not infer inner waits; queue, stage, and in-flight instrumentation remains explicit. See the [`tailtriage-axum` README](../tailtriage-axum/README.md).

### Existing Rust tracing instrumentation

Choose tracing only when the application already has suitable Rust `tracing` instrumentation and correlation:

```bash
cargo add tailtriage --features tracing
cargo add tailtriage --features tracing-live
cargo add tailtriage --features tracing-tokio
```

- `tracing` provides typed records and stable completed-span JSONL intake.
- `tracing-live` includes `tracing` and adds live recorder/session APIs.
- `tracing-tokio` includes `tokio` plus `tracing-live` and adds Tokio-coupled live session support.

Offline import converts supported Tailtriage completed-span JSONL into the standard Run analyzed by the same CLI:

```bash
tailtriage import tracing-spans-jsonl completed-spans.jsonl --service checkout --output tailtriage-run.json
tailtriage analyze tailtriage-run.json
```

This is not arbitrary tracing-log ingestion. One completed logical work item needs one unique tailtriage request ID; retries, fanout branches, and batch items must not reuse an ambiguous ID. The [`tailtriage-tracing` README](../tailtriage-tracing/README.md) owns fields, wrappers, import policy, session lifecycle, and Tokio coupling.

### Embedded analysis

Add `tailtriage-analyzer` when a consumer needs typed in-process `Report` values and renderers. Capture remains owned by the façade, while analysis remains a separate package step. Its [package README](../tailtriage-analyzer/README.md) and Rustdoc own the API.

### Analyzer tuning

Start with defaults. Only after representative captures justify tuning, use `--analyzer-config`, `--analyzer-set`, and `tailtriage analyzer-options`. Keep options stable across a comparable rerun. Exact options remain in the [diagnostics reference](diagnostics.md) and analyzer package contract.

### Focused package boundaries

The façade is the default new-integration entry point. Choose `tailtriage-core`, `tailtriage-controller`, `tailtriage-tokio`, `tailtriage-axum`, or `tailtriage-tracing` directly only when an intentionally narrower dependency/API boundary is useful. The [documentation index](README.md) maps their owners.

## Next destinations

- [Analyzer guide](analyzer-guide.md): turn one report into one next check.
- [Production operations guide](operations.md): rollout, bounded captures, limits, truncation, and comparable reruns.
- [Diagnostics reference](diagnostics.md): exact analyzer behavior.
- [Documentation index](README.md): integrations, CLI artifacts, evidence, security, and all user-facing references.
