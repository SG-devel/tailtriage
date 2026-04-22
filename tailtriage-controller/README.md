# tailtriage-controller

`tailtriage-controller` manages repeated, bounded capture generations in long-lived services.

Use it to arm capture, collect one generation, disarm/finalize, then re-arm later without restarting the process. This crate controls capture; analysis is done by `tailtriage-cli`.

## When to use this crate

Use `tailtriage-controller` when you need repeated arm/disarm windows in a long-lived process.

Use `tailtriage-core` for one explicit `build -> capture -> shutdown` run. Use `tailtriage` if you want the default entry point and optional controller support behind a feature.

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

## Minimal TOML shape

Use TOML for repeatable operational settings, including `mode` selection.

```toml
[controller]
service_name = "checkout-service"

[controller.activation]
mode = "light"

[controller.activation.sink]
type = "local_json"
output_path = "tailtriage-run.json"
```

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

## Generation semantics

- At most one generation is active at a time.
- `enable()` creates a fresh generation.
- A generation keeps the activation-time config it started with.
- Requests remain bound to the generation that admitted them.
- Requests started while disabled or closing are inert wrappers and never join a later generation.
- Each activation writes a per-generation artifact using `-generation-N` in the file name.

## Lifecycle states (plain language)

- **enabled**: capture is active and admitting requests.
- **disabled**: capture is off.
- **closing**: new captured admissions are blocked; the current generation finalizes after already-admitted captured requests drain.
- **inert request**: request wrapper returned while disabled/closing; it preserves request metadata but records no capture events.

`disable()` outcomes:

- `already disabled`: no active generation existed.
- `closing`: admissions were stopped and finalization is waiting on in-flight captured requests.
- `finalized`: generation finalized immediately (or had already drained) and artifact writing completed.

Artifact path generation for `local_json` sinks:

- generated file names append `-generation-N`
- parent directory from the configured path is preserved
- base file stem is reused
- original extension is preserved when present, otherwise `.json` is used

## Reload semantics

`reload_config()` updates the template for future generations only.

- Active generation settings do not change after activation.
- File edits affect the next `enable()`.
- Generation boundaries remain explicit and stable.

## Config precedence

When TOML is loaded with `config_path(...)`:

- `service_name` falls back to the builder value when omitted.
- `initially_enabled` falls back to the builder value when omitted.
- activation template settings come from TOML.
- omitted optional activation subfields use TOML contract defaults.

## Run-end policies

Supported policies:

- `continue_after_limits_hit`
- `auto_seal_on_limits_hit`

Behavior:

- `continue_after_limits_hit` (default): generation stays active after first truncation (`limits_hit`).
- `auto_seal_on_limits_hit`: on first `limits_hit`, admissions stop and the generation moves to closing; finalization happens immediately if no captured requests are in flight, otherwise after they drain.

TOML contract:

- `[controller.activation.run_end_policy]` is optional.
- If that table is present, `kind` is required.
- If the table is omitted, policy defaults to `continue_after_limits_hit`.

Example:

```toml
[controller.activation.run_end_policy]
kind = "auto_seal_on_limits_hit"
```

## Runtime sampler template

The controller can start a Tokio runtime sampler for armed generations when enabled in the runtime sampler template.

- Sampler startup still requires an active Tokio runtime.
- Runtime sampler settings are fixed at activation time.
- Runtime snapshot retention is still bounded by resolved core capture limits.

## TOML field reference

### `[controller]`

- `service_name` *(optional string)*: overrides builder service name when present; must not be empty.
- `initially_enabled` *(optional bool)*: when `true`, `build()` starts generation `1`.

### `[controller.activation]`

- `mode` *(required string)*: `light` or `investigation`.
- `strict_lifecycle` *(optional bool, default `false`)*.

### `[controller.activation.sink]`

- `type` *(required string)*: `local_json`.
- `output_path` *(required string for `local_json`)*: base path template for per-generation files.

### `[controller.activation.capture_limits_override]`

All optional field-level overrides:

- `max_requests`
- `max_stages`
- `max_queues`
- `max_inflight_snapshots`
- `max_runtime_snapshots`

### `[controller.activation.runtime_sampler]`

Optional table. Defaults to disabled.

- `enabled_for_armed_runs`
- `mode_override`
- `interval_ms`
- `max_runtime_snapshots`

### `[controller.activation.run_end_policy]`

Optional table. If present, `kind` is required.

- `kind = "continue_after_limits_hit"`
- `kind = "auto_seal_on_limits_hit"`

## Related crates

- `tailtriage`: default entry point
- `tailtriage-core`: direct instrumentation lifecycle
- `tailtriage-tokio`: runtime-pressure sampling
- `tailtriage-cli`: artifact analysis
