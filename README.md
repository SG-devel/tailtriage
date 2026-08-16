# tailtriage

`tailtriage` is a focused Rust toolkit for **Tokio tail-latency triage**.

When an async Rust service gets slow, `tailtriage` helps you answer a first practical question quickly:

> Is this slowdown mostly app-level queueing, executor pressure, blocking-pool pressure, or a slow downstream stage?

It produces a triage report with **evidence-ranked suspects** and **next checks**. Suspects are leads, not proof of root cause.

- Built for Tokio services and teams doing iterative triage.
- Useful with partial instrumentation.
- Not an observability backend.
- Not root-cause proof on its own.

## When to use tailtriage

| Symptom | tailtriage helps check |
| --- | --- |
| p95/p99 latency spikes | whether tail latency is dominated by queueing, executor pressure, blocking-pool pressure, or downstream stage latency |
| intermittent request timeouts | whether slow requests share a common bottleneck family in one captured run |
| low CPU but high latency | whether requests are waiting in queues, blocked behind constrained resources, or delayed by downstream work |
| requests appear stuck | whether time is spent before work starts, inside service execution, or in a named downstream stage |
| suspected blocking in async code | whether blocking-pool pressure is visible and should be investigated with a targeted follow-up |
| Tokio runtime seems overloaded | whether captured runtime-pressure signals point toward executor contention rather than app-level queueing |
| queue buildup before work starts | whether application queue wait dominates p95 latency |
| slow database or external API suspected | whether a downstream stage dominates request latency enough to be the next check |
| flaky latency in staging or production | which bottleneck family is the strongest lead from a bounded capture window |
| hard-to-reproduce tail spikes | whether a captured slow window contains enough evidence to choose the next experiment |
| unclear profiler results | whether queueing, runtime pressure, blocking-pool pressure, or downstream waiting explains the tail before pursuing CPU hot paths |
| service has partial instrumentation only | whether available request, queue, stage, runtime, or inflight signals are enough for a useful triage lead |

## Quick start (crates.io)

For direct capture or repeated controller-managed capture windows:

```bash
cargo add tailtriage
```

Optional integrations:

```bash
cargo add tailtriage --features axum
cargo add tailtriage --features tracing
cargo add tailtriage --features tracing-live
cargo add tailtriage --features tracing-tokio
```

`controller` and `tokio` are enabled by default on `tailtriage`; `axum` and tracing intake remain opt-in.

If you want a smaller core-only dependency surface, use `tailtriage-core` directly or depend on `tailtriage` with `default-features = false`.

`tailtriage` captures request/runtime evidence. Install analyzer/report tooling based on how you work.

For command-line analysis of saved Run artifact JSON:

```bash
cargo install tailtriage-cli
```

Saved Run analysis is strict by default: error-level core integrity findings stop report generation, while warning-only findings remain accepted. `--allow-ambiguous-artifact` is the explicit escape hatch; it warns for every original issue and analyzes only canonically normalized evidence. Tracing import `--strict` is a separate input parsing/import policy. In-process analyzer defaults remain permissive.

For in-process Rust analysis/report generation:

```bash
cargo add tailtriage-analyzer
```

Add `tailtriage-analyzer` when you want to analyze a completed Run inside Rust code.
- `tailtriage-cli` consumes Run artifact JSON from disk.
- `tailtriage-analyzer` produces typed `Report` values in process and renders **Report JSON** when you call analyzer renderers.

### Already using tracing?

If your service already emits `tracing` spans, use `tailtriage --features tracing-live` when you want the default crate façade (`tailtriage::tracing`) for live tracing intake, or use `tailtriage-tracing` directly when you want the narrow crate boundary. Native capture remains the default path for new integrations.

Offline import expects completed tailtriage `tt.*` tracing span JSONL (not arbitrary tracing log JSON), requires explicit Unix-ms start/end timestamps, and passes source-valid candidate evidence to core for generic Run integrity normalization. Complete run-relative monotonic offsets improve precision when present; missing offsets remain supported as duration-only legacy evidence.

- Offline JSONL import:
  ```bash
  tailtriage import tracing-spans-jsonl completed-spans.jsonl --service checkout --output tailtriage-run.json
  tailtriage analyze tailtriage-run.json
  ```
- Live session path: install either `tailtriage --features tracing-live` for `tailtriage::tracing::TracingSession` or `tailtriage-tracing --features live` for `tailtriage_tracing::TracingSession`, then add `session.layer()` beside your existing subscriber setup.

Both paths convert tracing-shaped evidence into standard `tailtriage_core::Run` data and feed the same analyzer/report workflow (evidence-ranked suspects and next checks). Runtime-pressure evidence still requires runtime snapshots (for example via the Tokio sampler).

### Request ID contract

Within one Run, `request_id` is the tailtriage identity for one completed logical request or work item. It must be unique among completed requests in that Run. Queue and stage events must reuse that ID only for evidence from the same logical request.

External trace or correlation IDs can be broader than a tailtriage request. If an ID can repeat across retries, fanout branches, batch items, or attempts, convert it into a unique tailtriage `request_id` first, for example by adding attempt, span, branch, or item information. `tailtriage` can warn about ambiguous duplicate IDs, but users remain responsible for meaningful instrumentation and request-boundary semantics.


## Why not just tokio-console or tokio-metrics?

Those tools are complementary building blocks. `tailtriage` fills a different gap: it turns request lifecycle timing plus optional runtime signals into a focused triage loop:

`capture -> analyze -> next check -> re-run`

In short:

- `tokio-console` helps you inspect live runtime/task behavior.
- `tokio-metrics` gives you runtime/task metrics signals.
- `tailtriage` helps you rank likely bottleneck families and choose the next targeted check from one captured run.

## Tool comparison

| Tool | Best for | Use with tailtriage when |
| --- | --- | --- |
| `tracing` | structured logs and spans | you need operational context around the captured slow window |
| `tokio-console` | live Tokio task/runtime inspection | tailtriage points toward executor/runtime pressure and you need live inspection |
| `tokio-metrics` | runtime and task metrics | you want runtime signals to strengthen or explain tailtriage evidence |
| `pprof` / flamegraph | CPU hot paths | tailtriage does not show queueing, runtime, blocking-pool, or downstream waiting as the likely lead |
| `tailtriage` | first-pass ranking of likely latency bottleneck families from one run | you need a focused next-check loop rather than continuous observability |

## What you get from the output

A report ranks evidence for four suspect families: application queueing, blocking-pool pressure, executor pressure, and downstream-stage latency. It identifies a primary lead, possible secondary leads, supporting evidence, confidence and evidence-quality limits, warnings, and concrete next checks. These are triage leads, not proof of root cause.

Start with the [analyzer guide](docs/analyzer-guide.md) to turn a report into one controlled next check. Use the [analyzer behavior reference](docs/diagnostics.md) only when you need exact fields, scoring, ordering, configuration, or evidence-limit mechanics.

## Primary entry points

- `tailtriage::Tailtriage` — one direct capture lifecycle
- `tailtriage::controller::TailtriageController` — repeated bounded capture windows for long-lived services
- `tailtriage::tokio` _(default-enabled)_ — runtime-pressure sampling
- `tailtriage::axum` _(optional)_ — Axum integration
- `tailtriage::tracing` _(optional)_ — tracing intake
- `tailtriage-analyzer` — typed in-process analysis and rendering
- `tailtriage-cli` — strict loading and command-line analysis of saved Run artifacts

## Minimal capture and analysis

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

Analyze the saved **Run artifact** at the command line:

```bash
tailtriage analyze tailtriage-run.json
```

Or analyze a completed `Run` in process and obtain a typed **Report**:

```rust
use tailtriage_analyzer::{analyze_run, AnalyzeOptions};
# use tailtriage::Run;
# fn example(run: Run) -> Result<(), Box<dyn std::error::Error>> {
let report = analyze_run(&run, AnalyzeOptions::default())?;
# let _ = report;
# Ok(())
# }
```

The Run artifact is captured evidence and CLI input; Report JSON is analyzer output. For package-local usage details, see [`tailtriage-cli/README.md`](tailtriage-cli/README.md) or [`tailtriage-analyzer/README.md`](tailtriage-analyzer/README.md).

## Controller capture windows

Choose `TailtriageController` when a long-lived service needs repeated arm, collect, disarm, and re-arm windows. Start with builder defaults; use TOML when operational settings must be repeatable. The [controller README](tailtriage-controller/README.md) owns its configuration and reload contract, while the [operations guide](docs/operations.md) owns production capture choices.

## Operations and validation

The [operations guide](docs/operations.md) covers rollout, capture modes, runtime sampling, retention, truncation, artifact sizing, and controlled reruns. [VALIDATION.md](docs/dev/VALIDATION.md) describes what repository validation does and does not support; measurements remain machine-, workload-, and profile-scoped rather than universal production guarantees.

## What this is not

`tailtriage` is not:

- an observability backend
- a distributed tracing system
- a general telemetry platform
- a root-cause proof engine

## Development alternative (workspace checkout)

Use the GitHub/workspace path when you want to run packaged examples, inspect internals, or contribute.

## Examples

Start with the smallest capture-to-analysis example:

```bash
cargo run -p tailtriage-tokio --example minimal_checkout
```

The relevant [package README](docs/README.md#integrations-and-package-boundaries) owns each
package's adoption examples. Contributors can run the complete public-example smoke through the
command owner, `python3 scripts/smoke_public_examples.py`.

## Demos

The demos are intentionally small services for Tokio tail-latency triage. They are designed to exercise diagnosis behavior with deterministic, reviewable artifacts, not universal causality proof.

For the shortest deterministic demo path, start with:

```bash
python3 scripts/demo_tool.py validate queue
```

Use before/after comparisons as a reproducible mitigation-confirmation loop, not causal proof.

The [demo walkthrough](docs/getting-started-demo.md) owns the first-user sequence; the
[demo index](demos/README.md) owns the complete scenario and contributor command surface.

## Documentation

The canonical user documentation index lives in [`docs/README.md`](docs/README.md).

Start there for the user workflow, crate selection, controller configuration, analyzer and CLI contracts, diagnostics interpretation, demos, validation, runtime-cost measurement, collector limits, and architecture.



### Partial queue and stage events

Completed queue and stage JSON remains wire-compatible: schema version stays `2`, older schema-v2 JSON without `completed` reads as completed evidence, and completed events omit `completed` when serialized. The Rust structs now include `completed: bool`, which is an intentional pre-1.0 source break for external exhaustive `StageEvent` and `QueueEvent` struct literals. Prefer `StageEvent::new(...)` and `QueueEvent::new(...)`; constructors default to completed evidence and `into_partial()` should be used only when intentionally constructing partial evidence.

Timing starts on first poll. Dropping a never-polled helper records no event. Dropping a polled pending helper while capture is open records one bounded partial event whose duration ends at observed helper Drop; late Drop after collector finalization is inert. Partial evidence is a lower-bound observation and does not prove that the underlying operation stopped. For partial stages, `success` is forced to `false`; it is not a completed operation result, so completion-aware consumers must inspect `completed`. Tracing spans remain completed-only. Analyzer reports keep completed queue/stage distributions completed-only, surface partial helper durations as observed lower-bound evidence, and apply evidence-aware confidence before final ranking.

Migration example:

```rust
# use tailtriage_core::StageEvent;
// Old exhaustive struct literal (now must include `completed`).
let _old = StageEvent {
    request_id: "req".into(),
    stage: "db".into(),
    started_at_unix_ms: 1,
    started_at_run_us: None,
    finished_at_unix_ms: 2,
    finished_at_run_us: None,
    latency_us: 10,
    success: true,
    completed: true,
};

// Recommended: constructors default to completed evidence.
let completed = StageEvent::new("req", "db", 1, 2, 10, true);
let partial = completed.clone().into_partial();
```


## Partial queue/stage evidence

Completed queue and stage distributions exclude partial observations. Partial durations are observed lower bounds: tailtriage observed the helper from first poll until Drop, not proof that the underlying operation completed, failed, or stopped. Partial evidence remains visible in event totals, evidence-quality limitations, top-level warnings, and suspect evidence.

Queue/service public p95 fields remain completed-only. A queue or downstream-stage suspect materially relying on an observed-lower-bound path cannot exceed medium confidence; partial evidence that does not affect selected eligibility or score does not automatically cap a completed candidate. Partial stage `success = false` is not interpreted as a completed operation failure.

Global, route, and temporal projections share this policy. Tracing imports remain completed-only. Completed-only Report JSON and text remain unchanged; mixed or partial Runs may change scores or ranking only when explicitly labeled lower-bound evidence is selected and qualified. Suspects remain triage leads, not root-cause proof.
