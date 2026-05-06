# User guide

This guide teaches the default `tailtriage` workflow for end users.

## 1) Default adoption path

For most services, use:

- `tailtriage` for capture instrumentation
- `tailtriage-cli` for artifact analysis/report generation
- `tailtriage-analyzer` for in-process analysis in Rust code

Install:

```bash
cargo add tailtriage
cargo add tailtriage-analyzer
cargo install tailtriage-cli
```

Optional integrations:

```bash
cargo add tailtriage --features tokio
cargo add tailtriage --features "tokio,axum"
```

## 2) Core workflow: capture -> analyze -> next check -> re-run

### Capture

```rust,no_run
use tailtriage::Tailtriage;

# fn main() -> Result<(), Box<dyn std::error::Error>> {
let run = Tailtriage::builder("checkout-service")
    .output("tailtriage-run.json")
    .build()?;

let started = run.begin_request("/checkout");
started.completion.finish_ok();

run.shutdown()?;
# Ok(())
# }
```

### Analyze

```bash
tailtriage analyze tailtriage-run.json --format json
```

### Decide next check

Read output in this order:

1. `primary_suspect.kind`
2. `primary_suspect.evidence[]`
3. `primary_suspect.next_checks[]`

Then run one targeted check, change one thing, and re-run under comparable load.


## 3) In-process analysis (embedded Rust users)

If you already have a completed in-memory `Run` (or stable snapshot in process), use `tailtriage-analyzer`:

```rust
use tailtriage_analyzer::{analyze_run, render_text, AnalyzeOptions};
# use tailtriage_core::Run;
# fn example(run: Run) -> Result<(), serde_json::Error> {
let report = analyze_run(&run, AnalyzeOptions::default());
let text = render_text(&report);
let json = serde_json::to_string_pretty(&report)?;
# let _ = (text, json);
# Ok(())
# }
```

Current analyzer semantics are batch/snapshot based for one completed run. Live streaming analysis is not part of the current contract.

## 4) Request lifecycle contract (required)

`begin_request(...)` / `begin_request_with(...)` returns `StartedRequest`:

- `started.handle` (`RequestHandle`) for instrumentation
- `started.completion` (`RequestCompletion`) for explicit completion

```rust,no_run
use tailtriage::Tailtriage;

# async fn demo(run: &Tailtriage) -> Result<(), Box<dyn std::error::Error>> {
let started = run.begin_request("/checkout");
let req = started.handle.clone();

req.queue("checkout_queue").await_on(async {}).await;
let _: Result<(), ()> = req.stage("downstream_call").await_on(async { Ok(()) }).await;

started.completion.finish_ok();
# Ok(())
# }
```

Important semantics:

- finish exactly once (`finish`, `finish_ok`, `finish_result`)
- drop does not auto-finish
- `shutdown()` does not fabricate completion/outcome
- `strict_lifecycle(true)` can fail shutdown when unfinished requests remain

## 5) Direct capture vs controller

Use **direct capture** (`Tailtriage`) when you want a straightforward run lifecycle in app code.

Use **controller** (`TailtriageController`) when your service is long-lived and you need repeated bounded windows over time:

- enable capture window
- collect
- disable/finalize
- re-enable later

Minimal controller window example:

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

Controller details: [tailtriage-controller/README.md](../tailtriage-controller/README.md)

## 6) Controller TOML config and reload semantics

Controller config is for repeatable operational settings across environments.

Stay with builder defaults when you are exploring locally or need one straightforward capture setup. Move to TOML when you need consistent operational settings (service identity, output path, capture limits, sampler template) across environments without rebuilding.

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

At contract level:

- set config file path with `config_path(...)`
- call `reload_config()` to refresh the template from file
- reload applies to **future generations only**
- active generation keeps activation-time config

See crate README for the full TOML field reference and expanded starter example: [tailtriage-controller/README.md](../tailtriage-controller/README.md)

## 7) Runtime sampler: when and why

Add runtime sampling when request timing alone does not clearly separate:

- queueing saturation
- executor pressure
- blocking-pool pressure

Use `tailtriage --features tokio`, then start `RuntimeSampler` for the run. `CaptureMode` does not auto-start sampling.

Key constraints:

- start inside an active Tokio runtime
- one successful sampler start per run
- runtime snapshot retention is bounded by core limits
- some runtime fields require `tokio_unstable`

Sampler details: [tailtriage-tokio/README.md](../tailtriage-tokio/README.md)

## 8) Axum adapter: what it is and is not

`tailtriage-axum` is a framework-boundary ergonomics layer:

- middleware handles request start/finish at Axum boundary
- extractor passes request handle into handlers

It is not automatic diagnosis. Queue/stage/inflight instrumentation is still explicit in handler/helper code.

Adapter details: [tailtriage-axum/README.md](../tailtriage-axum/README.md)

## 9) What to do when result is `insufficient_evidence`

When `primary_suspect.kind` is `insufficient_evidence`:

1. add at least one queue wrapper around suspected waits
2. add at least one stage wrapper around suspected downstream work
3. optionally add runtime sampler if runtime pressure is unclear
4. rerun with comparable load and compare evidence movement

Use [diagnostics.md](diagnostics.md) for interpretation details.

## Migration note (library analyzer path)

```rust
// Old pre-0.1.x API, no longer the supported library analyzer path:
use tailtriage_cli::analyze::{analyze_run, render_text};

// New:
use tailtriage_analyzer::{analyze_run, render_text, AnalyzeOptions};
```

## 10) Next docs

- [Documentation index](README.md)
- [Diagnostics guide](diagnostics.md)
- [Getting started demos](getting-started-demo.md)
- [Architecture](architecture.md)
