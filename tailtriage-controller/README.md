# tailtriage-controller

`tailtriage-controller` manages repeated, bounded tail-latency capture windows in long-lived Tokio
services. Choose it when a process must enable capture, finalize one generation, and later enable a
fresh generation without restarting. It produces evidence for triage; suspects are leads, not proof
of root cause.

For one explicit `build -> capture -> shutdown` run, use `tailtriage-core` instead.

## Installation

```bash
cargo add tailtriage-controller
```

## Capture one generation

Select an output explicitly. Each generation adds `-generation-N` before the extension, so the
example writes `tailtriage-run-generation-1.json`.

The example is an `async fn` intended to be called from your application's existing async runtime;
the crate does not require you to adopt a particular executor setup.

```rust,no_run
use tailtriage_controller::TailtriageController;

async fn capture_checkout() -> Result<(), Box<dyn std::error::Error>> {
    let controller = TailtriageController::builder("checkout-service")
        .output("tailtriage-run.json")
        .build()?;

    controller.enable()?;
    let started = controller.begin_request("/checkout");
    {
        let _inflight = started.handle.inflight("requests");
        started
            .handle
            .queue("db-pool")
            .with_depth_at_start(3)
            .await_on(async {})
            .await;
        let result: Result<(), ()> = started
            .handle
            .stage("inventory")
            .await_on(async { Ok(()) })
            .await;
        result.map_err(|()| "inventory failed")?;
    }
    // The completion token, not a handle clone, owns request completion.
    started.completion.finish_ok();

    // Reversible: closes admissions and finalizes now, or after admitted work drains.
    controller.disable()?;
    // A later controller.enable()? would start a fresh generation.

    // Terminal process-lifecycle step: later enable/reload calls are rejected.
    controller.shutdown()?;
    Ok(())
}
```

Queue and stage wrappers still execute their futures when admission is inert. Such requests retain
their metadata, record no evidence, and never join a later generation. `is_captured()` reports
historical admission identity, not whether the original generation is still open. Keep the
`ControllerStartedRequest` and its completion token through measured work, then finish explicitly.

## Lifecycle mental model

- `enable()` creates one bounded generation from the current template; only one may be active.
- `disable()` reversibly stops admissions. Finalization is immediate only when admitted captured
  requests have drained; otherwise the generation closes and the last completion triggers it.
- Already-admitted wrappers stay bound to their original generation across closing and re-enable.
- `shutdown()` is terminal and rejects future enable and reload operations.
- Strict lifecycle can return a retryable unfinished-request finalization error before any sink
  attempt. Complete/drop admitted work, then call a controller operation again for the authoritative
  stored result. Persistence/serialization failures are terminal for that generation and replayed
  without another sink write.
- Capture limits can truncate evidence. Queue/stage cancellation may yield bounded lower-bound
  observations; dropping an instrumentation future does not prove its external operation stopped.

## TOML configuration

Use `config_path(...)` for repeatable operational settings:

```toml
[controller]
service_name = "checkout-service"
initially_enabled = false

[controller.activation]
output_path = "tailtriage-run.json"
mode = "light"
run_end_policy = "continue_after_limits_hit"

[controller.activation.runtime_sampler]
enabled = false
interval_ms = 250
```

An explicit TOML `controller.activation.output_path` overrides builder `.output(...)`; omission
falls back to the original builder output. Resolution fails if neither supplies a non-empty path.
TOML service name and mode similarly override their builder values when present. Standalone
`load_config_from_path(...)` has no builder output fallback and therefore requires TOML output.

Reload is transactional: invalid input leaves the prior usable template installed. Successful
reload affects future generations only; it neither changes an active generation nor creates a
generation or sampler.

Rust sampler configuration uses `RuntimeSamplerTemplate` and `Option<Duration>`; TOML uses integer
`interval_ms`. Merely choosing a capture mode, configuring, or reloading never starts sampling.
An enabled sampler starts during `enable()`, which must run inside an active Tokio runtime and can
return sampler-start validation errors. Runtime evidence remains bounded by capture limits.

## Boundaries

Controller artifacts contain bounded evidence for later diagnosis into evidence-ranked suspects
and next checks. The controller is not an analyzer, observability backend, or causal-proof engine.
