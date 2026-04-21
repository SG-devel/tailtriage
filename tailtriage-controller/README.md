# tailtriage-controller

`tailtriage-controller` is the long-lived control layer for **repeated bounded capture windows**.

Use it when your service must stay up and you want to:

- arm capture
- collect one bounded generation
- disarm and finalize
- re-arm later with a fresh generation

This crate is for operational capture control. It does not analyze artifacts; `tailtriage-cli` does that.

## What problem this crate solves

`tailtriage-core` gives you a direct `build -> capture -> shutdown` lifecycle.

That is ideal for one explicit run.

`tailtriage-controller` solves a different problem: a long-lived service that needs multiple bounded generations over time without restarting the process.

Each activation creates a fresh generation with its own run ID and artifact path.

## Installation

```bash
cargo add tailtriage-controller
```

## Quick start

```rust,no_run
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

## How controller generations behave

A controller generation has a fixed activation-time configuration.

Important consequences:

- at most one generation is active at a time
- `enable()` creates a fresh generation
- `disable()` stops new admissions and finalizes immediately or after admitted requests drain
- requests admitted to a generation stay bound to that generation
- requests started while disabled or closing are inert wrappers and never move into a later generation
- each activation writes a per-generation artifact using a `-generation-N` suffix

## When to choose this crate

Choose `tailtriage-controller` when:

- the process is long-lived
- you want repeated bounded windows
- you want config-driven operational capture settings
- you want to reload future capture templates from TOML

Choose `tailtriage-core` instead when one explicit run lifecycle in application code is enough.

Choose `tailtriage` instead when you want the default entry point and may still use controller support through that crate.

## Minimal TOML shape

Use builder configuration when you want the simplest local setup.

Use TOML when you want repeatable operational settings across environments.

Minimal TOML:

```toml
[controller]
service_name = "checkout-service"

[controller.activation]
mode = "light"

[controller.activation.sink]
type = "local_json"
output_path = "tailtriage-run.json"
```

Minimal builder with config file:

```rust,no_run
use tailtriage_controller::TailtriageController;

fn demo() -> Result<(), Box<dyn std::error::Error>> {
    let controller = TailtriageController::builder("checkout-service")
        .config_path("tailtriage-controller.toml")
        .build()?;
    let _ = controller;
    Ok(())
}
```

## Reload semantics

`reload_config()` updates the template for **future generations only**.

An active generation keeps the exact activation-time configuration it started with.

That means:

- changing the TOML file does not mutate the active run
- the next `enable()` uses the reloaded template
- generation boundaries remain explicit and stable

## Run-end policies

The controller supports two run-end policies:

- `continue_after_limits_hit`
- `auto_seal_on_limits_hit`

### `continue_after_limits_hit`

This is the default.

The generation stays active after capture limits are hit. The collector keeps accepting calls and cheaply dropping additional retained data while preserving truncation counters.

Use this when you want the generation to keep running until manual disarm or shutdown.

### `auto_seal_on_limits_hit`

On the first `limits_hit` transition:

- new admissions stop
- the generation becomes closing
- finalization happens immediately if there are no in-flight captured requests
- otherwise finalization waits for the admitted captured requests to drain

Use this when “first truncation ends the window” is the operational rule you want.

## Runtime sampler support

The controller can start a Tokio runtime sampler automatically for armed runs when the runtime sampler template enables it.

Important constraints:

- startup still requires an active Tokio runtime
- runtime sampler config is fixed at activation time for that generation
- runtime snapshot retention is still bounded by the resolved core capture limits

## Detailed reference

### Top-level builder surface

`TailtriageController::builder(service_name)` supports:

- `config_path(...)`
- `initially_enabled(...)`
- `output(...)`
- `capture_limits_override(...)`
- `strict_lifecycle(...)`
- `runtime_sampler(...)`
- `run_end_policy(...)`
- `build()`

### Config file precedence

When TOML is loaded with `config_path(...)`:

- `controller.service_name` falls back to the builder value when omitted
- `controller.initially_enabled` falls back to the builder value when omitted
- activation template settings come from TOML when config is loaded
- omitted optional activation subfields use TOML contract defaults, not prior builder overrides

### Expanded TOML example

```toml
[controller]
service_name = "checkout-service"
initially_enabled = false

[controller.activation]
mode = "investigation"
strict_lifecycle = true

[controller.activation.capture_limits_override]
max_requests = 150000
max_stages = 300000
max_queues = 300000
max_inflight_snapshots = 300000
max_runtime_snapshots = 150000

[controller.activation.sink]
type = "local_json"
output_path = "tailtriage-run.json"

[controller.activation.runtime_sampler]
enabled_for_armed_runs = true
mode_override = "investigation"
interval_ms = 250
max_runtime_snapshots = 20000

[controller.activation.run_end_policy]
kind = "auto_seal_on_limits_hit"
```

## TOML field reference

### `[controller]`

- `service_name` *(optional string)*
  - overrides the builder service name when present
  - must not be empty when provided

- `initially_enabled` *(optional boolean)*
  - builder/default behavior applies when omitted
  - when `true`, `build()` immediately starts generation `1`

### `[controller.activation]`

- `mode` *(required string)*
  - valid values: `light`, `investigation`
  - selects core capture retention defaults
  - does **not** change request lifecycle semantics
  - does **not** automatically start runtime sampling

- `strict_lifecycle` *(optional boolean, default `false`)*
  - controls whether unfinished requests can fail finalization for that activation

### `[controller.activation.sink]`

- `type` *(required string)*
  - supported value: `local_json`

- `output_path` *(required string for `local_json`)*
  - base output path template
  - each generation writes a per-generation artifact with a `-generation-N` suffix

### `[controller.activation.capture_limits_override]`

All fields are optional and override selected mode defaults field by field:

- `max_requests`
- `max_stages`
- `max_queues`
- `max_inflight_snapshots`
- `max_runtime_snapshots`

### `[controller.activation.runtime_sampler]`

Template fields for future activations:

- `enabled_for_armed_runs`
- `mode_override`
- `interval_ms`
- `max_runtime_snapshots`

Defaults:

- the table is optional
- runtime sampling is disabled by default
- when enabled, sampler startup still requires an active Tokio runtime

### `[controller.activation.run_end_policy]`

- `kind` *(optional string)*
  - valid values:
    - `continue_after_limits_hit`
    - `auto_seal_on_limits_hit`

## Related crates

- `tailtriage`: recommended default entry point
- `tailtriage-core`: direct per-run instrumentation lifecycle
- `tailtriage-tokio`: runtime-pressure sampling
- `tailtriage-cli`: artifact analysis
