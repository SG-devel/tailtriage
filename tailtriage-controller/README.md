# tailtriage-controller

`tailtriage-controller` manages repeated, bounded capture windows for long-lived services.

Use it when you want to turn capture on, collect one generation, turn capture off, and later start a fresh generation without restarting the process.

For in-process analysis/report generation, use `tailtriage-analyzer`.
For command-line analysis of saved artifacts, use `tailtriage-cli`.

## When to use this crate

Use `tailtriage-controller` when you need repeated arm/disarm windows in one process.

Use `tailtriage-core` for a single explicit `build -> capture -> shutdown` run.

Use `tailtriage` when you want the default entry point with controller support enabled by default (or disabled via Cargo features).

## Installation

```bash
cargo add tailtriage-controller
```

## Quick start

`output("tailtriage-run.json")` configures the base artifact path template. It is required unless `controller.activation.output_path` is supplied by configured TOML. No output path is invented implicitly. Each activation writes a per-generation artifact with `-generation-N` in the file name (for example, generation 1 writes `tailtriage-run-generation-1.json`).

```rust,no_run
use tailtriage_controller::TailtriageController;

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

## Mental model

A controller owns a **template** plus at most one **active generation**.

- `enable()` creates a fresh generation from the current template.
- `disable()` stops new admissions for that generation.
- If no captured requests are still in flight, the generation finalizes immediately.
- Otherwise the generation enters **closing** and finalizes after its already-admitted captured requests drain.
- The next `enable()` creates a new generation with a new artifact path.
- `shutdown()` is terminal: it stops admissions, finalizes the current generation, and permanently rejects later `enable()` and reload operations.
- `disable()` and `shutdown()` report active-generation failures through `GenerationFinalizationError`, whose source is the core `ShutdownError`.
- After retryable unfinished-request shutdown, admitted work may finish and trigger automatic finalization; call `shutdown()` again to authoritatively replay the stored success or terminal failure without another sink write.
- `status()` reports the simple `GenerationState::Shutdown` marker after shutdown; detailed persistence results remain in the returned `GenerationFinalizationError`.

Requests started while the controller is disabled or closing are **inert**:

- they preserve request metadata
- they record no capture events
- they never join a later generation

Each activation writes a per-generation artifact whose file name includes `-generation-N`.

## Request wrappers

Instrument through the controller request wrapper without branching on capture state:

```rust,ignore
let started = controller.begin_request("/checkout");

started.handle.queue("db").await_on(async {
    // work
}).await;

let _: Result<(), ()> = started
    .handle
    .stage("query")
    .await_on(async { Ok(()) })
    .await;

let _guard = started.handle.inflight("requests");
```

No capture-state branch is needed for ordinary instrumentation. Admissions made while capture is
disabled, closing, or shut down return inert wrappers that await work unchanged and record nothing.
An already-captured wrapper remains tied to the generation that admitted it.

Inspect admission identity only when needed:

```rust,ignore
if started.handle.is_captured() {
    // This request was captured when it began.
}

if let Some(core_handle) = started.handle.captured_handle() {
    // Explicit interoperability with the core OwnedRequestHandle.
}
```

“Captured” is historical admission identity, not the controller's current enablement state.
`captured_handle()` is the explicit core-interoperability escape hatch; normal instrumentation
should continue through the controller wrapper.

## Minimal TOML example

Use TOML when you want repeatable operational settings, including mode selection.

```toml
[controller]
service_name = "checkout-service"

[controller.activation]
mode = "light"

output_path = "tailtriage-run.json"
```

## Expanded TOML example

```toml
[controller]
service_name = "checkout-service"
initially_enabled = false

[controller.activation]
output_path = "tailtriage-run.json"
mode = "investigation"
strict_lifecycle = true
run_end_policy = "auto_seal_on_limits_hit"

[controller.activation.capture_limits_override]
max_requests = 150000
max_stages = 300000
max_queues = 300000
max_inflight_snapshots = 300000
max_runtime_snapshots = 150000

[controller.activation.runtime_sampler]
enabled = true
mode_override = "investigation"
interval_ms = 250
max_runtime_snapshots = 20000
```

## Config precedence and reload rules

When TOML is loaded with `config_path(...)`:

- `service_name` from TOML overrides the builder value when present.
- builder `service_name` is a fallback only when TOML omits `service_name`.
- `initially_enabled` falls back to the builder value when omitted.
- TOML `output_path` and `mode` override builder values when present.
- omitted TOML `output_path` falls back to the original builder output; if neither source supplies one, construction or reload fails.
- omitted TOML `mode` falls back to the original builder mode, which defaults to `light`.
- omitted optional activation subfields use TOML contract defaults.

`reload_config()` and the result-returning `reload_template(template)` update the
template for **future** generations only. Template validation and replacement do
not create a capture generation or start a runtime sampler; sampler startup remains
part of `enable()`.

They do not mutate a generation that is already active. Each active generation
keeps one immutable activation snapshot, and admitted requests remain bound to it
until completion even across disarm and re-enable.

`reload_template(template)` returns a `Result`; callers handle or propagate validation errors.
Replacement is transactional, so an invalid template leaves the current template unchanged.

## Run-end policies

Supported policies:

- `continue_after_limits_hit` _(default)_
- `auto_seal_on_limits_hit`

Behavior:

- `continue_after_limits_hit`: generation stays active after the first truncation
- `auto_seal_on_limits_hit`: on the first `limits_hit`, new admissions stop and the generation moves to closing; finalization happens immediately if no captured requests are still in flight, otherwise after they drain

TOML contract:

- `controller.activation.run_end_policy` is optional
- when present, it is a string value

## Runtime sampler template

The controller can start a Tokio runtime sampler automatically for armed generations.

Important constraints:

- sampler startup still requires an active Tokio runtime
- sampler settings are fixed at activation time
- runtime snapshot retention is still bounded by the resolved core capture limits

Programmatic Rust configuration uses `enabled` and a native `Option<Duration>` interval:

```rust
use std::time::Duration;
use tailtriage_controller::{RuntimeSamplerTemplate, TailtriageController};

let sampler = RuntimeSamplerTemplate {
    enabled: true,
    mode_override: None,
    interval: Some(Duration::from_millis(250)),
    max_runtime_snapshots: Some(20_000),
};
let controller = TailtriageController::builder("checkout-service")
    .output("tailtriage-run.json")
    .runtime_sampler(sampler)
    .build()?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

The operator-facing TOML boundary keeps the `interval_ms` integer shown above; the public
`RuntimeSamplerTemplate` itself is not a standalone serialization type.

## TOML field reference

### `[controller]`

- `service_name` _(optional string)_: overrides the builder service name when present; must not be empty
- `initially_enabled` _(optional bool)_: when `true`, `build()` starts generation `1`

### `[controller.activation]`

- `mode` _(optional string)_: `light` or `investigation`; falls back to builder mode and ultimately `light`
- `output_path` _(required unless supplied with builder `.output(...)`)_: base path template for per-generation files
- `strict_lifecycle` _(optional bool, default `false`)_

### `[controller.activation.capture_limits_override]`

All fields are optional:

- `max_requests`
- `max_stages`
- `max_queues`
- `max_inflight_snapshots`
- `max_runtime_snapshots`

### `[controller.activation.runtime_sampler]`

Optional table. Default is disabled.

- `enabled`
- `mode_override`
- `interval_ms`
- `max_runtime_snapshots`

### `controller.activation.run_end_policy`

Optional string. The default is `continue_after_limits_hit`.

- `run_end_policy = "continue_after_limits_hit"`
- `run_end_policy = "auto_seal_on_limits_hit"`

## Important constraints

- at most one generation is active at a time
- active generation settings do not change after activation
- requests remain bound to the generation that admitted them
- controller capture and analysis are separate
- for in-process analysis/report generation, use `tailtriage-analyzer`
- for command-line analysis of saved artifacts, use `tailtriage-cli`

## Related crates

- `tailtriage`: default entry point
- `tailtriage-core`: direct instrumentation lifecycle
- `tailtriage-tokio`: runtime-pressure sampling
- `tailtriage-axum`: Axum request-boundary integration
- `tailtriage-tracing`: optional tracing intake bridge that converts tracing-shaped evidence into standard `tailtriage_core::Run` values
- `tailtriage-analyzer`: in-process analysis/report generation for completed runs
- `tailtriage-cli`: command-line analysis of saved run artifacts
