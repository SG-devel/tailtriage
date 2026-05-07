# tailtriage documentation

This is the canonical documentation index for `tailtriage`.

`tailtriage` is a Rust toolkit for Tokio tail-latency triage. It helps turn one captured run into evidence-ranked suspects and targeted next checks. Suspects are triage leads, not proof of root cause.

## Start here

- [User guide](user-guide.md) — recommended first read. Covers the default workflow, request lifecycle, direct capture versus controller capture, TOML-backed controller setup, runtime-sampler context, and result interpretation.
- [Default crate README (`tailtriage`)](../tailtriage/README.md) — quickest path for most users who want one dependency plus optional integrations.
- [Core crate README (`tailtriage-core`)](../tailtriage-core/README.md) — framework-agnostic capture primitives and artifact writing.

## Choose the right crate

Most users should start with `tailtriage`.

- [`tailtriage`](../tailtriage/README.md) — recommended default entry point; re-exports core APIs and exposes optional controller, Tokio, and Axum integrations.
- [`tailtriage-core`](../tailtriage-core/README.md) — smallest framework-agnostic capture surface.
- [`tailtriage-controller`](../tailtriage-controller/README.md) — repeated bounded capture windows for long-lived services, including TOML config and reload behavior.
- [`tailtriage-tokio`](../tailtriage-tokio/README.md) — optional Tokio runtime-pressure sampling.
- [`tailtriage-axum`](../tailtriage-axum/README.md) — Axum middleware/extractor integration.
- [`tailtriage-analyzer`](../tailtriage-analyzer/README.md) — in-process analysis of completed runs.
- [`tailtriage-cli`](../tailtriage-cli/README.md) — command-line analysis of saved run artifacts.

## Capture and analysis workflow

Use these docs when wiring `tailtriage` into a service or interpreting output.

- [User guide](user-guide.md) — end-to-end adoption path and operational workflow.
- [Diagnostics guide](diagnostics.md) — how to read analyzer output, evidence quality, suspects, confidence, warnings, and field meanings.
- [Analyzer README (`tailtriage-analyzer`)](../tailtriage-analyzer/README.md) — typed in-process report contract and rendering API.
- [CLI README (`tailtriage-cli`)](../tailtriage-cli/README.md) — saved-artifact loading, schema validation, and text/JSON output.

## Controller and runtime integrations

Use these docs when running repeated capture windows or adding runtime/framework signal.

- [Controller README (`tailtriage-controller`)](../tailtriage-controller/README.md) — arm/disarm lifecycle, capture generations, TOML field reference, run-end policy, and reload semantics.
- [Tokio runtime sampler README (`tailtriage-tokio`)](../tailtriage-tokio/README.md) — runtime-pressure enrichment, feature requirements, and sampling constraints.
- [Axum adapter README (`tailtriage-axum`)](../tailtriage-axum/README.md) — request-boundary wiring for Axum services.

## Validation, cost, and limits

Use these docs to understand what the project claims, how those claims are tested, and where measurement boundaries are.

- [Validation overview](../VALIDATION.md) — validation scope, claims, and current diagnostic scorecard entry point.
- [Diagnostic validation](diagnostic-validation.md) — corpus-driven diagnostic-quality methodology and metrics.
- [Runtime cost measurement](runtime-cost.md) — reproducible overhead attribution path for baked-in, core, sampler, and drop-path costs.
- [Collector limits and stress guidance](collector-limits.md) — sustained-load behavior, truncation onset, artifact-size growth, and memory trend interpretation.

## Demos and examples

Use these docs when you want runnable scenarios or fixture-backed examples.

- [Getting started with demos](getting-started-demo.md) — recommended first demos and validation commands.
- [Demos catalog](../demos/README.md) — scenario layout, fixtures, and demo-specific notes.

## Design and repository structure

Use these docs when contributing or trying to understand how the workspace fits together.

- [Architecture](architecture.md) — crate responsibilities and the file-based triage pipeline.
- [Project spec](../SPEC.md) — product and implementation specification.
