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
