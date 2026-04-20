# tailtriage-controller

Long-lived controller scaffolding for live arm/disarm capture workflows in `tailtriage`.

## Layering

- `tailtriage-core`: per-run collector and artifact model.
- `tailtriage-controller`: helper/control layer for repeated bounded activations.

This crate currently provides scaffold types only:

- controller builder/template/status types
- generation state and one-active-generation invariant
- run-end policy modeling

It intentionally does **not** implement the full run lifecycle yet.

## Minimal usage

```rust
use tailtriage_controller::TailtriageController;

let controller = TailtriageController::builder("checkout-service")
    .initially_enabled(true)
    .output("tailtriage-run.json")
    .build()?;

let status = controller.status();
# let _ = status;
# Ok::<(), tailtriage_controller::ControllerBuildError>(())
```
