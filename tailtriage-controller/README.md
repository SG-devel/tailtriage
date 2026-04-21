# tailtriage-controller

`tailtriage-controller` is the control layer for **repeated bounded capture windows** in long-lived services.

Use it when you need to arm/disarm capture without restarting the process.

## Quick navigation

- [When to use this crate](#when-to-use-this-crate-vs-others)
- [Config file (TOML)](#config-file-toml)
- [Minimal controller lifecycle example](#minimal-controller-lifecycle-example)
- [Behavioral guarantees](#behavioral-guarantees)
- [Runtime requirements](#runtime-requirements)

## Config file (TOML)

Controller config is intentionally first-class. If you are operating tailtriage in production-like workflows, start here.

- Use `config_path(...)` on the builder to load TOML-backed template settings.
- Use `reload_config()` to refresh **future** generations from the config file.
- Active generations keep their activation-time config.

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

Most important sections to configure in TOML are typically:

1. Service/run identity and artifact output settings
2. Capture defaults and retention limits
3. Runtime sampler template settings (when enabled)

See the full controller API docs and examples for exact keys and template fields.

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
