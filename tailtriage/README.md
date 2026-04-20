# tailtriage

`tailtriage` is the official facade crate and umbrella entry point for Tokio tail-latency triage.

It always re-exports `tailtriage-core` as the foundation API and exposes optional integration crates behind feature-gated namespaces.

## Installation

```bash
cargo add tailtriage
```

Optional integrations:

```bash
cargo add tailtriage --features tokio
cargo add tailtriage --features "tokio,axum"
```

`controller` is enabled by default and re-exported at `tailtriage::controller`.

## Feature flags

- `controller` (default): enables `tailtriage::controller` (from `tailtriage-controller`)
- `tokio`: enables `tailtriage::tokio` (from `tailtriage-tokio`)
- `axum`: enables `tailtriage::axum` (from `tailtriage-axum`)
- `full`: enables `controller`, `tokio`, and `axum`

Advanced users can still depend on focused crates (`tailtriage-core`, `tailtriage-controller`, `tailtriage-tokio`, `tailtriage-axum`) directly for tighter dependency control.

## Examples

Core-only usage:

```no_run
use tailtriage::Tailtriage;

# fn demo() -> Result<(), Box<dyn std::error::Error>> {
let run = Tailtriage::builder("checkout-service")
    .output("tailtriage-run.json")
    .build()?;
run.shutdown()?;
# Ok(())
# }
```

Tokio runtime sampling (requires `tokio` feature):

```no_run
# #[cfg(feature = "tokio")]
# async fn demo(run: std::sync::Arc<tailtriage::Tailtriage>) -> Result<(), Box<dyn std::error::Error>> {
use tailtriage::tokio::RuntimeSampler;

let sampler = RuntimeSampler::builder(run).start()?;
sampler.shutdown().await;
# Ok(())
# }
```

Controller convenience layer (default `controller` feature):

```no_run
# #[cfg(feature = "controller")]
# fn demo() -> Result<(), Box<dyn std::error::Error>> {
use tailtriage::controller::TailtriageController;

let controller = TailtriageController::builder("checkout-service")
    .initially_enabled(true)
    .build()?;
let _status = controller.status();
# Ok(())
# }
```
