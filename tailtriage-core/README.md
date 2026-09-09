# tailtriage-core

`tailtriage-core` is the framework-agnostic capture foundation for Tailtriage, a focused toolkit
for Tokio tail-latency triage. It records bounded request, queue, stage, in-flight, and optional
runtime evidence in a standard `Run` artifact. The evidence supports suspects and next checks; it
does not prove root cause.

Choose this crate for the smallest direct-capture and artifact-schema surface. Choose the
`tailtriage` crate instead when you want its default aggregator/re-export surface and optional
integrations. For typed in-process diagnosis use `tailtriage-analyzer`; for command-line diagnosis
of a saved `Run`, install and use `tailtriage-cli`.

## Installation and output choice

```bash
cargo add tailtriage-core
```

The crate has no optional Cargo features. Construction requires an explicit output strategy:

- `.output(path)` selects `LocalJsonSink` and writes one finalized Run JSON document at shutdown.
- `.sink(MemorySink)` retains a clone of the last finalized typed `Run` in memory, replacing any
  earlier value.
- `.sink(DiscardSink)` finalizes capture without retaining or persisting the `Run`.
- `.sink(custom_sink)` accepts another `RunSink` implementation.

A builder with neither `.output(...)` nor `.sink(...)` returns `BuildError::MissingSink`; there is
no implicit file or default sink.

## Complete direct-capture example

Call this async function from your application's existing async runtime. The placeholder futures
show lifecycle wiring only; their result does not promise a diagnosis.

```rust,no_run
use tailtriage_core::Tailtriage;

async fn capture_checkout() -> Result<(), Box<dyn std::error::Error>> {
    let run = Tailtriage::builder("checkout-service")
        .output("tailtriage-run.json")
        .build()?;

    let started = run.begin_request("/checkout");
    let request = started.handle.clone();

    request.queue("ingress").await_on(async {}).await;
    let in_flight = request.inflight("checkout_handlers");
    let result = request
        .stage("database")
        .await_on(async { Ok::<_, std::io::Error>(()) })
        .await;
    drop(in_flight);

    started.completion.finish_result(result)?;
    run.shutdown()?;
    Ok(())
}
```

## Lifecycle and limits

`begin_request(...)` and `begin_request_with(...)` return a borrowed `StartedRequest` containing
an instrumentation `handle` and a single-owner `completion` token. With `Arc<Tailtriage>`, the
`begin_owned_request(...)` variants return owned forms that can cross helper or task boundaries.
Options set request identity/kind but do not alter admission or completion.

Admission is allowed only while capture is open and
`retained requests + pending admitted requests < max_requests`. Thus `max_requests` bounds pending
plus retained requests at admission, not only the final request vector. A refused start returns
an inert handle and disarmed completion token: its wrapped async work still runs, but it records no
request, queue, stage, or in-flight evidence. Refusal while capture is open updates request
truncation accounting; starts after finalization are inert without changing the finalized Run.
Other configured limits independently bound their retained or live evidence and update truncation
accounting when reached.

Instrumentation never completes a request. Explicitly consume each admitted completion token with
`finish`, `finish_ok`, or `finish_result` when the outcome is known. If an armed token drops while
capture is open—including during panic unwinding—Drop records exactly one `cancelled` completion
without panicking. Explicit finish disarms that Drop path. If non-strict shutdown finalizes first,
the request appears only in unfinished metadata and late finish or Drop is inert.

Non-strict `shutdown()` finalizes and sends the normalized artifact to the selected sink, recording
unfinished-request metadata without inventing completions. With `strict_lifecycle(true)`, pending
requests instead produce retryable `ShutdownError::UnfinishedRequests`; no sink attempt or
finalization mutation occurs, so complete/drop the outstanding tokens and retry. Once an eligible
shutdown attempts the sink, success or `ShutdownError::Sink` is terminal and no later call retries
the write. Request completion, shutdown lifecycle failure, and sink failure are distinct results.

Before finalization, a snapshot has no finalization timestamp. A snapshot obtained after shutdown
may contain the finalized timestamp. `run_end_reason` can already be present before finalization
when an integration has closed admissions; direct shutdown supplies `shutdown` only when no more
specific reason is present. Older or manually assembled artifacts can omit the reason.

## Essential evidence constraints

- Capture modes choose bounded defaults; overrides can replace the full limit set or selected
  fields. Modes do not change lifecycle semantics or start runtime sampling.
- Queue and stage helper timing starts on first poll. Never-polled Drop records no event. Dropping
  a polled pending helper while capture remains open may retain one bounded partial event ending at
  observed Drop. That duration is a lower bound, not proof that the operation stopped.
- An `InflightGuard` increments its named gauge when created and decrements it on Drop while
  capture is open; keep it around exactly the work being measured. It does not finish a request.
- Duration fields in microseconds are authoritative elapsed evidence. Unix-millisecond fields are
  coarse wall-clock anchors; complete run-relative offsets provide monotonic precision.
- Request IDs identify completed logical work within one Run and should be unique. Queue and stage
  evidence must use the same ID only for its parent request.
- `RunBuilder` assembles already completed/imported evidence; it does not track live lifecycle or
  synthesize missing request completions.
- Strict validation rejects error-level integrity issues. Permissive normalization preserves
  duration-authoritative evidence where possible, but can clear invalid optional precision and
  exclude invalid or ambiguous evidence. It does not reconstruct missing truth or make every
  metadata/schema input valid; inspect its returned report and dispositions.
- Schema version 2 is current. Current CLI analysis requires a finalized Run with at least one
  completed request; active snapshots and empty in-process Runs remain useful for inspection.

Public item Rustdoc contains the exhaustive units, defaults, errors, ownership, normalization,
Drop, and side-effect contracts for each API.
