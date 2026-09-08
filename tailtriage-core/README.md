# tailtriage-core

`tailtriage-core` is the framework-agnostic capture foundation for `tailtriage`. It records
bounded request, queue, stage, and in-flight evidence and writes a schema-v2 `Run` artifact for
tail-latency triage. It does not analyze runs or provide controller windows, Axum integration, or
Tokio runtime sampling.

Choose this crate for the smallest direct-capture API. Choose the `tailtriage` facade instead when
you want the same core API together with optional integration features.

## Installation

```bash
cargo add tailtriage-core
```

This crate has no optional Cargo features. Construction requires an explicit output strategy:
`.output(...)` selects `LocalJsonSink`, while `.sink(...)` accepts a custom sink such as
`MemorySink` or `DiscardSink`. A builder without either selection returns
`BuildError::MissingSink`; it never chooses an implicit file.

## Complete capture example

```rust,no_run
use tailtriage_core::{RequestOptions, Tailtriage};

async fn capture() -> Result<(), Box<dyn std::error::Error>> {
    let run = Tailtriage::builder("checkout-service")
        .output("tailtriage-run.json")
        .build()?;

    let started = run.begin_request_with(
        "/checkout",
        RequestOptions::new().request_id("req-1").kind("http"),
    );
    let request = started.handle.clone();

    request.queue("ingress").await_on(async {}).await;
    request
        .stage("database")
        .await_on(async { Ok::<(), std::io::Error>(()) })
        .await?;
    let _inflight = request.inflight("checkout_handlers");

    started.completion.finish_ok();
    drop(_inflight);
    run.shutdown()?;
    Ok(())
}
```

## Lifecycle and limits

`begin_request(...)` and `begin_request_with(...)` return a `StartedRequest`: its clonable
`handle` records child evidence, while its single-owner `completion` records the request outcome.
The `begin_owned_request(...)` variants provide the same split lifecycle from an
`Arc<Tailtriage>` for handles that must move across tasks or helper layers.

Finish an admitted completion token once with `finish`, `finish_ok`, or `finish_result`. Dropping
it unfinished while capture is open records one `cancelled` request; instrumentation does not
finish a request. A request refused because capture is finalized or `max_requests` is exhausted
receives inert handle and completion values and records no request or child evidence.
`max_requests` is the admission bound over pending plus retained completed requests, while the
other limits bound their corresponding evidence. Limit refusal and retention drops update
truncation accounting.

Non-strict `shutdown()` finalizes an artifact even with pending requests, recording unfinished
metadata without inventing completions. With `strict_lifecycle(true)`, pending requests produce a
retryable `ShutdownError::UnfinishedRequests` before any sink attempt. Sink serialization or I/O
failure is reported separately as `ShutdownError::Sink`; once a sink attempt begins, shutdown is
terminal and single-shot. Late request activity after finalization is inert.

## Essential artifact constraints

- Request IDs identify one completed logical work item and should be unique within a run; queue
  and stage evidence must use the same ID only for that item.
- Duration fields in microseconds are authoritative elapsed-time evidence. Unix-millisecond
  timestamps are wall-clock correlation anchors; optional run-relative offsets provide more
  precise interval placement.
- Capture modes select retention defaults only. They do not change lifecycle behavior or start
  runtime sampling.
- `snapshot()` is an unfinalized in-memory view. `shutdown()` sets finalization metadata and writes
  through the selected sink; a successful direct shutdown records `RunEndReason::Shutdown` unless
  an integration already supplied a more specific reason.
- `inspect_run`, `validate_run_strict`, and `normalize_run_permissive` inspect completed artifacts.
  Permissive normalization can exclude ambiguous or invalid evidence and clear invalid optional
  precision, but it cannot reconstruct missing truth or guarantee that arbitrary input becomes a
  valid run.
- Captured evidence supports evidence-ranked suspects and next checks. Those suspects are triage
  leads, not proof of root cause; measurements remain workload, machine, and profile scoped.
