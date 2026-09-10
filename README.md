# tailtriage

`tailtriage` is a focused Rust toolkit for **Tokio tail-latency triage**. It helps answer a bounded first question:

> Is this async service slow because of application queue pressure, executor pressure, blocking-pool pressure, or a slow downstream stage?

It turns a captured Run into **evidence-ranked suspects** and **next checks**. Suspects are leads, not proof of root cause.

## Is it a fit?

Use `tailtriage` for bounded Tokio tail-latency triage, especially when you need to distinguish queue, runtime, blocking-pool, and downstream signals through a repeatable `capture -> next check -> re-run` loop. Partial instrumentation can still provide useful evidence.

It is not an observability backend, distributed tracing backend, general telemetry platform, CPU profiler replacement, or root-cause proof engine. `tracing`, `tokio-console`, `tokio-metrics`, and profilers remain complementary tools for the targeted follow-up that a report suggests.

## Install the default path

Use the façade for capture and the CLI for analysis. The example directly uses Tokio, so declare it too:

```bash
cargo add tailtriage
cargo add tokio --features macros,rt,time
cargo install tailtriage-cli
```

## Capture one useful Run

Instrument boundaries that can explain waiting: a queue before work starts and a named stage around downstream or service work are good first choices.

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

Replace the sleep calls with the queue wait and database call, downstream request, or handler/service work you want to measure. The handle records evidence; the completion token owns request completion. Finish exactly once after measured work, then call `shutdown()` to finalize and save the Run. Preserve a fallible workload result until completion and shutdown have both happened, as above. Do not shut down while request tasks or completion tokens remain active.

Runtime sampling is optional and explicitly started; the default `tokio` feature only makes its APIs available.

## Analyze the saved Run

Start with the text report:

```bash
tailtriage analyze tailtriage-run.json
```

The CLI expects a supported finalized Run artifact and strictly validates generic core integrity by default. An error-level finding blocks the report; warning-only precision limits are accepted. `--allow-ambiguous-artifact` is the explicit permissive-normalization escape hatch. Tracing import `--strict` is a separate source-conversion policy.

A tiny sample may validly report `insufficient_evidence`; the example promises only a finalized Run accepted by the analyzer, not a particular diagnosis.

## Read the result and take one next step

1. Read the primary suspect (the strongest lead).
2. Read its supporting evidence and any evidence-quality warnings.
3. Choose one `next_check`.
4. Change or instrument one thing.
5. Capture again under comparable conditions.

If the result is `insufficient_evidence`, add one useful missing boundary—such as a suspected queue/stage or explicit runtime sampling—and rerun. Completed queue/stage distributions use completed evidence. Partial helper observations are lower bounds from first poll until helper Drop; Drop does not prove the underlying work completed, failed, was cancelled, or stopped. Selected lower-bound evidence can cap confidence.

Use the [analyzer guide](docs/analyzer-guide.md) for the practical report-to-next-check workflow. Use the [diagnostics reference](docs/diagnostics.md) only for exact analyzer mechanics.

## Optional paths after first use

- **Controller:** repeated bounded arm/disarm windows in a long-lived service; `disable()` is reversible and `shutdown()` is terminal.
- **Tokio runtime sampling:** add runtime-pressure evidence by explicitly starting `tailtriage::tokio::RuntimeSampler` inside an active Tokio runtime. `CaptureMode` does not start it.
- **Axum:** enable `axum` for request-boundary middleware; instrument inner queues and stages explicitly.
- **Tracing:** enable `tracing` for typed/stable JSONL intake, `tracing-live` for live session APIs, or `tracing-tokio` for Tokio-coupled live sessions.
- **Embedded analysis:** add `tailtriage-analyzer` when code needs typed in-process `Report` values.
- **Tuning:** start with analyzer defaults; tune only after representative evidence justifies it.

Within one Run, use one unique tailtriage `request_id` for each completed logical request/work item. Queue and stage evidence reuses that ID only for that request. Advanced tracing, retry, and fanout correlation must preserve this uniqueness.

## Choose a destination

- [First capture and optional integrations](docs/user-guide.md)
- [Interpret one report](docs/analyzer-guide.md)
- [Operate captures in a long-lived service](docs/operations.md)
- [Understand exact analyzer behavior and evidence limits](docs/diagnostics.md)
- [Complete user documentation index](docs/README.md)

Package READMEs and canonical item Rustdoc own exact API, lifecycle, feature, and integration contracts. Validation evidence is linked from the documentation index and remains machine-, workload-, and profile-scoped rather than a universal performance guarantee.
