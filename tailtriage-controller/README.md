# tailtriage-controller

`tailtriage-controller` is the **long-lived control layer** for `tailtriage` capture.

Use it when your service must stay running while you repeatedly arm/disarm bounded capture generations.

## When to use this crate vs others

- Use `tailtriage-core` for one run lifecycle (`build -> capture -> shutdown`).
- Use `tailtriage-controller` for repeated live capture windows with enable/disable control.
- Add `tailtriage-tokio` integration through controller runtime sampler template when runtime pressure evidence is needed.

## Installation

```bash
cargo add tailtriage-controller
```

## Example availability (published crate vs repo checkout)

- **Run examples from a repository checkout/workspace**
  - Run packaged examples with an explicit package target, for example:
    `cargo run -p tailtriage-controller --example controller_minimal`.
- **Published crate source includes examples for reference**
  - `tailtriage-controller` example source is included in crate source views
    (for example docs.rs source and crate tarballs), so consumers can copy
    the same snippets.
- **Consumer-project path**
  - In an arbitrary project that only added the dependency, use this README's
    Rust snippet (or copied example source) in your own crate targets.
  - Do **not** expect `cargo add tailtriage-controller` followed by
    `cargo run --example controller_minimal` to work unless that example file
    exists in your project.

## Minimal example

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

## Config/reload summary

- Optional TOML config can be provided via `config_path(...)`.
- `reload_config()` updates only the template for **future** generations.
- Active generations keep their original activation config.
- `reload_template(...)` is a compatibility helper that panics on invalid templates.
- Prefer `try_reload_template(...)` for explicit error handling.

## Runtime requirements

- Runtime sampler startup (if enabled in template) requires an active Tokio runtime.
- This crate does not prove root cause; it preserves evidence-ranked suspects and next checks downstream in `tailtriage-cli`.
