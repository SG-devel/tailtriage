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

## Minimal usage

```rust
use tailtriage_controller::TailtriageController;

fn demo() -> Result<(), Box<dyn std::error::Error>> {
let controller = TailtriageController::builder("checkout-service")
    .initially_enabled(false)
    .output("tailtriage-run.json")
    .build()?;

let generation = controller.enable()?;
if let Some(started) = controller.try_begin_request("/checkout") {
    started.completion.finish_ok();
}
let _ = controller.disable()?;

# let _ = generation;
# Ok(())
# }
```
