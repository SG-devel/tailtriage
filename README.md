# tailtriage

`tailtriage` is a focused Rust toolkit for **Tokio tail-latency triage**.

It helps you answer a first practical question quickly:

> Is this slowdown mostly app-level queueing, executor pressure, blocking-pool pressure, or a slow downstream stage?

The analyzer reports **evidence-ranked suspects** and **next checks**. Suspects are leads, not proof of root cause.

## Quick start (default path)

For most users, start with the facade crate:

```bash
cargo add tailtriage
```

Optional facade integrations:

```bash
cargo add tailtriage --features tokio
cargo add tailtriage --features "tokio,axum"
```

Install the CLI separately for analysis/report generation:

```bash
cargo install tailtriage-cli
```

> Library crates capture data. `tailtriage-cli` analyzes artifacts.

## Primary entry points

From `tailtriage`:

- `tailtriage::Tailtriage` — direct capture lifecycle
- `tailtriage::controller::TailtriageController` — repeated arm/disarm bounded capture windows for long-lived services
- `tailtriage::tokio` *(optional feature)* — runtime-pressure sampling
- `tailtriage::axum` *(optional feature)* — Axum middleware/extractor ergonomics

## When to choose the controller

Use `tailtriage::controller::TailtriageController` when your service must stay up and you need repeated capture windows over time (arm, collect, disarm, re-arm).

This is a major capability of the facade crate, not a niche add-on.

## Minimal examples

### Facade capture (library)

```rust,no_run
use tailtriage::Tailtriage;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let run = Tailtriage::builder("checkout-service")
        .output("tailtriage-run.json")
        .build()?;

    let started = run.begin_request("/checkout");
    started.completion.finish_ok();

    run.shutdown()?;
    Ok(())
}
```

### Controller capture window (library)

```rust,no_run
use tailtriage::controller::TailtriageController;

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

### Analyze artifact (CLI)

```bash
tailtriage analyze tailtriage-run.json --format json
```

## Development alternative (workspace checkout)

Use the GitHub/workspace path when you want to run packaged examples/demos or contribute:

```bash
cargo run -p tailtriage-tokio --example minimal_checkout
cargo run -p tailtriage-cli -- analyze tailtriage-run.json --format json
```

## Which package should I use?

- **Default:** `tailtriage` + `tailtriage-cli`
- **Controller-heavy operations:** `tailtriage` (controller is included by default)
- **Fine-grained dependency control:** direct `tailtriage-core`, `tailtriage-controller`, `tailtriage-tokio`, or `tailtriage-axum`
- **Analysis only:** `tailtriage-cli`

## Documentation map

- Facade/default crate docs: [`tailtriage/README.md`](tailtriage/README.md)
- Controller docs + config: [`tailtriage-controller/README.md`](tailtriage-controller/README.md)
- Runtime sampler docs: [`tailtriage-tokio/README.md`](tailtriage-tokio/README.md)
- User workflow guide: [`docs/user-guide.md`](docs/user-guide.md)
- Analyzer/diagnostics references:
  - [`tailtriage-cli/README.md`](tailtriage-cli/README.md)
  - [`docs/diagnostics.md`](docs/diagnostics.md)
- Advanced references:
  - [`docs/getting-started-demo.md`](docs/getting-started-demo.md)
  - [`docs/runtime-cost.md`](docs/runtime-cost.md)
  - [`docs/collector-limits.md`](docs/collector-limits.md)
