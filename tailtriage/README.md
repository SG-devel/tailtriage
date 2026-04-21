# tailtriage

`tailtriage` is the **default facade crate** for Tokio tail-latency triage.

If you are adopting tailtriage for the first time, start here.

## What this crate is for

Use `tailtriage` when you want one dependency that provides the main integration surface:

- direct capture lifecycle (`tailtriage::Tailtriage`)
- controller-driven bounded windows (`tailtriage::controller::TailtriageController`)
- optional Tokio runtime sampling (`tailtriage::tokio`, feature `tokio`)
- optional Axum ergonomics (`tailtriage::axum`, feature `axum`)

## When to use this crate vs others

- **Use `tailtriage` (recommended):** default onboarding path with cohesive API surface.
- **Use focused crates directly:** only when you need tighter dependency control or a narrower API surface.
- **Use `tailtriage-cli`:** separately, for analysis/report generation after capture.

## Installation

```bash
cargo add tailtriage
```

Enable optional integrations:

```bash
cargo add tailtriage --features tokio
cargo add tailtriage --features "tokio,axum"
```

Install analysis CLI separately:

```bash
cargo install tailtriage-cli
```

## Feature flags and namespaces

- `controller` *(default)*: enables `tailtriage::controller` (`tailtriage-controller`)
- `tokio`: enables `tailtriage::tokio` (`tailtriage-tokio`)
- `axum`: enables `tailtriage::axum` (`tailtriage-axum`)
- `full`: enables `controller`, `tokio`, and `axum`

## Minimal examples

### Direct capture

```rust,no_run
use tailtriage::Tailtriage;

# fn demo() -> Result<(), Box<dyn std::error::Error>> {
let run = Tailtriage::builder("checkout-service")
    .output("tailtriage-run.json")
    .build()?;

let started = run.begin_request("/checkout");
started.completion.finish_ok();

run.shutdown()?;
# Ok(())
# }
```

### Controller window (long-lived service)

```rust,no_run
use tailtriage::controller::TailtriageController;

# fn demo() -> Result<(), Box<dyn std::error::Error>> {
let controller = TailtriageController::builder("checkout-service")
    .initially_enabled(false)
    .output("tailtriage-run.json")
    .build()?;

let _generation = controller.enable()?;
let started = controller.begin_request("/checkout");
started.completion.finish_ok();
let _ = controller.disable()?;
# Ok(())
# }
```

## Key constraints

- Library capture and CLI analysis are separate installation/runtime steps.
- Runtime sampling is optional and requires the `tokio` feature.
- Axum helpers are optional and require the `axum` feature.

## Deeper docs

- User workflow: [`../docs/user-guide.md`](../docs/user-guide.md)
- Controller details and config: [`../tailtriage-controller/README.md`](../tailtriage-controller/README.md)
- Runtime sampler semantics: [`../tailtriage-tokio/README.md`](../tailtriage-tokio/README.md)
- Analyzer/report contract: [`../tailtriage-cli/README.md`](../tailtriage-cli/README.md)
