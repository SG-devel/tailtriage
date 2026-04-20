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

When the controller is disabled (or an active generation is closing), `begin_request(...)`
and `begin_request_with(...)` still return request tokens with the same non-branching
ergonomics, but those tokens are inert/no-op wrappers owned by this crate. They do not
interact with `tailtriage-core` state until a generation is actively admitting requests.

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
# "manual" | "max_requests" | "max_duration_ms" | "first_limit_hit"
kind = "manual"
# max_requests = 50000      # for kind = "max_requests"
# max_duration_ms = 30000   # for kind = "max_duration_ms"
```

Reload in v1 is explicit and manual:

- `controller.reload_config()?` re-reads TOML from `config_path`.
- Reload updates only the controller template for **future** activations.
- If a generation is already active, that generation keeps the exact activation config it started with.
- The reloaded template is applied the next time `enable()` starts a new generation.
