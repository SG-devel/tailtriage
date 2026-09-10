# tailtriage

`tailtriage` is the default façade package for focused Tokio tail-latency triage. Choose it for one coherent capture API plus optional controller, Tokio, Axum, and tracing namespaces. Use a focused sibling crate directly only when you intentionally want a narrower dependency or API boundary.

It helps distinguish application queue pressure, executor pressure, blocking-pool pressure, and slow downstream stages. Analysis produces evidence-ranked suspects and next checks; suspects are leads, not proof of root cause. This crate is not an observability backend, distributed tracing backend, general telemetry platform, CPU profiler replacement, or root-cause proof engine.

## Install

For the representative saved-Run workflow, install the façade and CLI. The example directly names Tokio, so declare Tokio directly with the features it uses:

```bash
cargo add tailtriage
cargo add tokio --features macros,rt,time
cargo install tailtriage-cli
```

## Capture meaningful work

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

Replace the sleeps with a real queue wait and database call, downstream request, or handler/service operation. The handle records evidence; the completion token owns request completion. Finish exactly once after work, then call `shutdown()` to finalize and write `tailtriage-run.json`. Keep a fallible result until completion and shutdown have happened so `?` cannot bypass lifecycle work. Do not finalize while request tasks or completion tokens remain active.

Within a Run, one completed logical request/work item needs one unique tailtriage `request_id`; queue and stage evidence must reuse it only for that request.

The example does not start runtime sampling. The default `tokio` feature makes the `tailtriage::tokio` sampler and Tokio helper APIs available; runtime sampling itself is optional and starts explicitly inside an active Tokio runtime.

## Analyze and act

```bash
tailtriage analyze tailtriage-run.json
```

Text is the easiest first output. Read the primary suspect, its supporting evidence and warnings, then choose one next check and rerun under comparable conditions. A tiny capture may validly produce `insufficient_evidence`; add one useful missing queue/stage boundary or explicit runtime sampling, then rerun. No report proves root cause.

The CLI accepts supported finalized Run artifacts and applies strict generic core validation by default. Error-level findings block report generation; warning-only precision limitations are accepted. `--allow-ambiguous-artifact` is the explicit permissive-normalization escape hatch. Tracing import `--strict` is a separate tracing-source policy. The `tailtriage-analyzer` package is the separate choice for typed, permissive-by-default in-process analysis.

Completed queue/stage distributions use completed observations. Partial helper observations are lower bounds through helper Drop, not proof that underlying work completed, failed, was cancelled, or stopped; selected lower-bound evidence can cap confidence.

## Feature matrix

| Selection | Available façade surface |
| --- | --- |
| `default-features = false` | Core capture API at the crate root only |
| defaults | Core root API plus `controller` and `tokio` namespaces |
| `axum` | Adds the Axum namespace independently |
| `tracing` | Adds typed records and stable completed-span JSONL tracing intake |
| `tracing-live` | Includes `tracing`; adds live recorder/session APIs |
| `tracing-tokio` | Includes `tokio` and `tracing-live`; adds Tokio-coupled live session support |
| `full` | Combines `controller`, `tokio`, `axum`, and `tracing-tokio` (therefore tracing); it is not a distinct runtime mode |

Example dependency selections:

```toml
[dependencies]
tailtriage = "0.3"
# Core-only:
# tailtriage = { version = "0.3", default-features = false }
# Axum in addition to defaults:
# tailtriage = { version = "0.3", features = ["axum"] }
```

Published crate documentation is generated with all façade features enabled, so feature-gated namespaces can appear in generated API documentation. Downstream availability still follows the features selected by that crate: core root exports are always present, defaults enable only `controller` and `tokio`, and other namespaces require their features.

## Choosing optional paths

- `tailtriage::controller`: repeated bounded windows in a long-lived service; disable is reversible and shutdown is terminal.
- `tailtriage::tokio`: explicitly started runtime-pressure sampling and Tokio helper APIs.
- `tailtriage::axum`: request-boundary middleware; inner queue/stage instrumentation stays explicit.
- `tailtriage::tracing`: supported typed/JSONL or live tracing intake for applications with suitable existing correlation.
- `tailtriage-analyzer`: typed in-process Report values and renderers.
- `tailtriage-cli`: saved-artifact analysis and supported tracing import.

Focused packages are `tailtriage-core`, `tailtriage-controller`, `tailtriage-tokio`, `tailtriage-axum`, and `tailtriage-tracing`. Their Rustdoc owns exhaustive item contracts, including lifecycle, errors, defaults, limits, and feature conditions.
