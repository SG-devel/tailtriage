# tailtriage-controller

`tailtriage-controller` is the control layer for **repeated bounded capture windows** in long-lived services.

Use it when you need to arm/disarm capture without restarting the process.

## Quick navigation

- [When to use this crate](#when-to-use-this-crate-vs-others)
- [Config file (TOML)](#config-file-toml)
- [TOML field reference](#toml-field-reference)
- [Minimal controller lifecycle example](#minimal-controller-lifecycle-example)
- [Behavioral guarantees](#behavioral-guarantees)
- [Runtime requirements](#runtime-requirements)

## Config file (TOML)

Builder defaults are a good place to start for local exploration.

Use TOML when you want repeatable operational settings across environments.

- `config_path(...)` loads TOML-backed template settings during build.
- `reload_config()` refreshes that template from file for **future generations only**.
- Active generations keep their activation-time config.
- With TOML config loaded, `service_name` and `initially_enabled` fall back to builder values when omitted.
- Activation template settings come from TOML when config is loaded.
- Omitted optional activation subfields use TOML contract defaults (for example `strict_lifecycle = false`), not prior builder overrides.

Minimal TOML shape:

```toml
[controller]
service_name = "checkout-service"

[controller.activation]
mode = "light"

[controller.activation.sink]
type = "local_json"
output_path = "tailtriage-run.json"
```

Expanded TOML example:

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

```rust,no_run
use tailtriage_controller::TailtriageController;

# fn demo() -> Result<(), Box<dyn std::error::Error>> {
let controller = TailtriageController::builder("checkout-service")
    .config_path("tailtriage-controller.toml")
    .build()?;
# let _ = controller;
# Ok(())
# }
```

## TOML field reference

### Top-level: `[controller]`

- `service_name` (optional string)
  - If present, overrides the builder service name.
  - If provided, it must not be empty.
- `initially_enabled` (optional boolean)
  - If omitted, builder/default behavior applies.
  - If `true`, build immediately starts generation 1.

### Activation: `[controller.activation]`

- `mode` (required string)
  - Valid values: `light`, `investigation`.
  - This selects core capture retention defaults.
  - It does **not** change request lifecycle semantics.
  - It does **not** automatically start runtime sampling.
- `strict_lifecycle` (optional boolean)
  - Default: `false`.
  - Controls whether unfinished requests can fail finalization/shutdown for that activation.

#### Sink: `[controller.activation.sink]`

- `type` (required string)
  - Supported value: `local_json`.
- `output_path` (required string for `local_json`)
  - Base output path template.
  - Each activation writes a per-generation artifact with a `-generation-N` suffix.

#### Capture limits override: `[controller.activation.capture_limits_override]`

All fields are optional and override selected mode defaults field-by-field:

- `max_requests`
- `max_stages`
- `max_queues`
- `max_inflight_snapshots`
- `max_runtime_snapshots`

#### Runtime sampler template: `[controller.activation.runtime_sampler]`

Template fields for future activations:

- `enabled_for_armed_runs`
- `mode_override`
- `interval_ms`
- `max_runtime_snapshots`

Defaults:

- Table is optional.
- Runtime sampler is disabled by default (`enabled_for_armed_runs = false`).
- When enabled, sampler startup still requires an active Tokio runtime.

#### Run-end policy: `[controller.activation.run_end_policy]`

- `kind` (optional string)
  - Valid values:
    - `continue_after_limits_hit`: keep accepting/dropping after limits are hit until disarm/shutdown.
    - `auto_seal_on_limits_hit`: on first limits-hit transition, stop admissions and finalize that generation.

## When to use this crate vs others

- **Use `tailtriage-controller`:** repeated arm/disarm windows in a long-lived service.
- **Use `tailtriage-core`:** single run lifecycle (`build -> capture -> shutdown`).
- **Use `tailtriage` facade:** default path that includes controller support by default.

## Installation

```bash
cargo add tailtriage-controller
```

## Minimal controller lifecycle example

```rust,no_run
use tailtriage_controller::TailtriageController;

fn demo() -> Result<(), Box<dyn std::error::Error>> {
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

## Behavioral guarantees

- At most one generation is active at a time.
- `enable()` creates a fresh generation (new run ID/artifact path).
- `disable()` stops admissions and finalizes immediately or after admitted requests drain.
- Requests admitted to a generation stay bound to that generation.
- Requests started while disabled/closing are inert wrappers and never migrate into later generations.

## Reload behavior

- `reload_config()` updates only the template for future generations.
- `reload_template(...)` is a compatibility helper that panics on invalid templates.
- Prefer `try_reload_template(...)` for explicit error handling.

## Runtime requirements

- If runtime sampling is enabled in the template, startup requires an active Tokio runtime.
- This crate controls capture windows; analysis/reporting is done by `tailtriage-cli`.

## Deeper docs

- Facade/default integration path: [`../tailtriage/README.md`](../tailtriage/README.md)
- Foundation instrumentation semantics: [`../tailtriage-core/README.md`](../tailtriage-core/README.md)
- Runtime sampler details: [`../tailtriage-tokio/README.md`](../tailtriage-tokio/README.md)
- CLI analyzer/report contract: [`../tailtriage-cli/README.md`](../tailtriage-cli/README.md)
