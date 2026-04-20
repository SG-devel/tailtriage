# tailtriage-controller

Long-lived controller scaffolding for live arm/disarm capture workflows in `tailtriage`.

## Layering

- `tailtriage-core`: per-run collector and artifact model.
- `tailtriage-controller`: helper/control layer for repeated bounded activations.

This crate provides:

- controller builder/template/status types
- enable/disable arm-disarm lifecycle with one-active-generation invariant
- per-generation admission gating and drain-aware finalization
- generation-specific artifact paths and run IDs
- run-end policy modeling
- controller-owned inert request wrappers for disabled/closing periods

## When to use the controller vs `Tailtriage::builder(...)`

Use ordinary `tailtriage-core` builder usage when you want one process-scoped run lifecycle:

- build one `Tailtriage`
- run workload
- call `shutdown()`

Use `tailtriage-controller` when your service should stay up while you repeatedly arm/disarm
bounded capture runs for triage windows.

The controller is a control layer on top of core; it does not replace direct builder usage.

## Live controller semantics

- At most one active generation can exist at a time.
- `enable()` starts a fresh generation with its own run ID/artifact path.
- `disable()` stops new admissions for that generation.
  - If no admitted requests remain, finalize now.
  - If admitted requests are still in flight, generation enters closing state and finalizes after drain.
- Requests admitted into a generation remain bound to that generation for completion.
  - They do not migrate into later generations during disable/re-enable churn.
- Requests started while disabled/closing are inert controller-owned wrappers and are never later
  attached to a new generation.

## Minimal usage

```rust
use tailtriage_controller::TailtriageController;

fn demo() -> Result<(), Box<dyn std::error::Error>> {
let controller = TailtriageController::builder("checkout-service")
    .initially_enabled(false)
    .output("tailtriage-run.json")
    .build()?;

let generation = controller.enable()?;
let started = controller.begin_request("/checkout");
started.completion.finish_ok();
let _ = controller.disable()?;

# let _ = generation;
# Ok(())
# }
```

### Runnable example (workspace checkout)

`controller_minimal` is bundled in the repository/workspace and in the published crate package.
Run it from a repository checkout:

`cargo run --manifest-path tailtriage-controller/Cargo.toml --example controller_minimal`

### Published crate examples (reference/adoption source)

`tailtriage-controller` packages `examples/**` in the published crate so consumers can read/copy
the exact example source from docs.rs or the crate source package.

Important: dependency examples are **not** runnable in an arbitrary consumer project by first
adding `tailtriage-controller` as a dependency and then running
`cargo run --example controller_minimal`. `cargo run --example ...` runs examples defined by the
current package.

You can also copy the minimal snippet directly into your service:

```rust
use tailtriage_controller::TailtriageController;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let controller = TailtriageController::builder("checkout-service")
        .initially_enabled(false)
        .output("tailtriage-run.json")
        .build()?;

    let _generation = controller.enable()?;
    let started = controller.begin_request("/checkout");
    started.completion.finish_ok();
    let _ = controller.disable()?;
    Ok(())
}
```

### Disabled-path expectations

When the controller is disabled (or an active generation is closing), `begin_request(...)`
and `begin_request_with(...)` still return request tokens with the same non-branching
ergonomics, but those tokens are inert/no-op wrappers owned by this crate.

- queue/stage/inflight wrappers are no-op
- completion methods are no-op lifecycle markers on the inert wrapper
- inert requests do not write capture events and do not join later generations
- inert metadata preserves explicit `request_id`/`kind`; if `request_id` is omitted,
  controller assigns a non-empty local fallback ID (`inert-{N}`)

This path is intended to be cheap and predictable, but users should still validate overhead in
their own workload/environment.

## Enable/disable/reload/status snippet

```rust
use tailtriage_controller::TailtriageController;

fn controller_demo() -> Result<(), Box<dyn std::error::Error>> {
    let controller = TailtriageController::builder("checkout-service")
        .output("tailtriage-run.json")
        .config_path("tailtriage-controller.toml")
        .initially_enabled(false)
        .build()?;

    // Arm one bounded generation.
    let active = controller.enable()?;

    let started = controller.begin_request("/checkout");
    started.completion.finish_ok();

    // Status reports template + generation snapshot.
    let _status_during_run = controller.status();

    // Disarm (finalizes immediately or after in-flight drain).
    let _disable = controller.disable()?;

    // Reload updates template for NEXT activation only.
    controller.reload_config()?;
    let _status_after_reload = controller.status();

    // Next enable uses reloaded template.
    let _next = controller.enable()?;

    # let _ = active;
    # Ok(())
    # }
```

## TOML config and manual reload

`tailtriage-controller` accepts a TOML file via `TailtriageController::builder(...).config_path(...)`.

```toml
[controller]
# optional; falls back to builder service name if omitted
service_name = "checkout-service"
# optional; falls back to builder.initially_enabled(...) if omitted
initially_enabled = false

[controller.activation]
mode = "light"                 # "light" | "investigation"
strict_lifecycle = false

[controller.activation.capture_limits_override]
max_requests = 100000
max_stages = 200000
# max_queues = ...
# max_inflight_snapshots = ...
# max_runtime_snapshots = ...

[controller.activation.sink]
type = "local_json"
output_path = "tailtriage-run.json"

[controller.activation.runtime_sampler]
enabled_for_armed_runs = true
mode_override = "investigation"  # optional
interval_ms = 200                # optional
max_runtime_snapshots = 100000   # optional

[controller.activation.run_end_policy]
# "continue_after_limits_hit" | "auto_seal_on_limits_hit"
kind = "continue_after_limits_hit"
```

Reload in v1 is explicit and manual:

- `controller.reload_config()?` re-reads TOML from `config_path`.
- Reload updates only the controller template for **future** activations.
- `reload_config()` validates the reloaded template immediately and returns an error
  instead of deferring invalid-template failures to the next `enable()`.
- If a generation is already active, that generation keeps the exact activation config it started with.
- The reloaded template is applied the next time `enable()` starts a new generation.

Direct template replacement has two forms:

- `try_reload_template(...) -> Result<_, ReloadTemplateError>` validates immediately and
  returns errors.
- `reload_template(...)` remains as a compatibility helper and panics on invalid templates.

Poisoned internal controller mutexes are recovered by taking ownership of the poisoned
state, so controller methods do not panic solely because a previous panic poisoned an
internal lock.

### Run-end policy behavior on limits hit

`[controller.activation.run_end_policy]` controls what happens when capture limits are hit:

- `kind = "continue_after_limits_hit"`: keep the generation active; additional data can be dropped after saturation
  until manual disarm/shutdown.
- `kind = "auto_seal_on_limits_hit"`: on the first transition to `limits_hit` in any capture path
  (request events, runtime snapshots, stage events, queue events, or in-flight snapshots),
  controller immediately stops new admissions and moves the current generation into
  sealing/finalization. If admitted requests are still in flight, the generation remains closing
  and finalizes as soon as those admitted requests drain.

## What this feature does not do

- Does **not** mutate an already-active generation when config is reloaded.
- Does **not** move admitted requests between generations.
- Does **not** auto-prove root cause; it preserves the same evidence-ranked suspect model.
- Does **not** force runtime sampling by default; sampler startup is still explicit via
  `enabled_for_armed_runs`.
