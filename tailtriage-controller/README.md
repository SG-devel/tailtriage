# tailtriage-controller

`tailtriage-controller` manages repeated, bounded capture windows for long-lived services.

Use it to arm capture, collect one generation, disarm, and later start a fresh generation without restarting the process. Analysis is still done by `tailtriage-cli`.

## Crate selection

Use `tailtriage-controller` for repeated arm/disarm windows in one process.

Use `tailtriage-core` for a single explicit `build -> capture -> shutdown` run.

Use `tailtriage` when you want the default entry point with optional controller support.

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

## Mental model

A controller owns a **template** plus at most one **active generation**.

- `enable()` creates a fresh generation from the current template.
- `disable()` stops new admissions for that generation.
- If no captured requests are still in flight, finalization is immediate.
- Otherwise the generation enters **closing** and finalizes after already-admitted captured requests drain.

Requests started while disabled or closing are **inert**: they keep request metadata, record no capture events, and never join a later generation.

Each activation writes a per-generation artifact whose file name includes `-generation-N`.

## Minimal TOML example

```toml
[controller]
service_name = "checkout-service"

[controller.activation]
mode = "light"

[controller.activation.sink]
type = "local_json"
output_path = "tailtriage-run.json"
```

## Expanded TOML example

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

## Config precedence and reload rules

When TOML is loaded with `config_path(...)`:

- `service_name` falls back to the builder value when omitted.
- `initially_enabled` falls back to the builder value when omitted.
- activation template settings come from TOML.
- omitted optional activation subfields use TOML contract defaults.

`reload_config()` updates the template for **future** generations only. It does not mutate an already-active generation.

## Run-end policies

Supported policies:

- `continue_after_limits_hit` _(default)_
- `auto_seal_on_limits_hit`

Behavior:

- `continue_after_limits_hit`: generation stays active after first truncation.
- `auto_seal_on_limits_hit`: on first `limits_hit`, new admissions stop and generation moves to closing; finalization is immediate when no captured requests are in flight, otherwise after drain.

TOML contract:

- `[controller.activation.run_end_policy]` is optional.
- if present, `kind` is required.

## Runtime sampler template

The controller can auto-start a Tokio runtime sampler for armed generations.

Constraints:

- sampler startup still requires an active Tokio runtime
- sampler settings are fixed at activation time
- runtime snapshot retention is bounded by the resolved core capture limits

## TOML field reference

### `[controller]`

- `service_name` _(optional string)_: overrides builder service name when present; must not be empty
- `initially_enabled` _(optional bool)_: when `true`, `build()` starts generation `1`

### `[controller.activation]`

- `mode` _(required string)_: `light` or `investigation`
- `strict_lifecycle` _(optional bool, default `false`)_

### `[controller.activation.sink]`

- `type` _(required string)_: `local_json`
- `output_path` _(required string for `local_json`)_: base path template for per-generation files

### `[controller.activation.capture_limits_override]`

All fields are optional:

- `max_requests`
- `max_stages`
- `max_queues`
- `max_inflight_snapshots`
- `max_runtime_snapshots`

### `[controller.activation.runtime_sampler]`

Optional table. Default is disabled.

- `enabled_for_armed_runs`
- `mode_override`
- `interval_ms`
- `max_runtime_snapshots`

### `[controller.activation.run_end_policy]`

Optional table. If present, `kind` is required.

- `kind = "continue_after_limits_hit"`
- `kind = "auto_seal_on_limits_hit"`
