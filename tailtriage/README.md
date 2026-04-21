# tailtriage

`tailtriage` is the **default facade crate** for Tokio tail-latency triage.

Use this crate when you want one dependency that can expose:

- `tailtriage-core` (always) as the instrumentation foundation
- `tailtriage::controller` (default feature) for long-lived arm/disarm capture control
- `tailtriage::tokio` (feature) for runtime-pressure evidence
- `tailtriage::axum` (feature) for Axum middleware/extractor ergonomics

If you want tighter dependency control, depend on focused crates directly.

## Installation

```bash
cargo add tailtriage
```

Enable optional integrations:

```bash
cargo add tailtriage --features tokio
cargo add tailtriage --features "tokio,axum"
```

## Feature flags

- `controller` (default): enables `tailtriage::controller` (`tailtriage-controller`)
- `tokio`: enables `tailtriage::tokio` (`tailtriage-tokio`)
- `axum`: enables `tailtriage::axum` (`tailtriage-axum`)
- `full`: enables `controller`, `tokio`, and `axum`

## Minimal example

```no_run
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

## Choosing crates in this workspace

- Use **`tailtriage`** for most integrations and docs.rs onboarding.
- Use **`tailtriage-core`** when you only want core request lifecycle instrumentation.
- Use **`tailtriage-controller`** when capture must be repeatedly armed/disarmed without restarting the service.
- Use **`tailtriage-tokio`** when you need runtime-pressure evidence in the same run artifact.
- Use **`tailtriage-axum`** for Axum-specific ergonomics.
- Use **`tailtriage-cli`** to analyze captured artifacts into evidence-ranked suspects and next checks.
