# Documentation index

This is the canonical user-facing docs index for `tailtriage`.

## Start here

- [User guide](user-guide.md) — default adoption path (`tailtriage` + `tailtriage-cli`) and the core capture -> analyze -> next check -> re-run workflow.
- [Default crate README (`tailtriage`)](../tailtriage/README.md) — fastest way to integrate with one dependency.

## Core workflow and interpretation

- [Diagnostics guide](diagnostics.md) — quick reading flow plus concise field reference for analyzer output.
- [CLI README (`tailtriage-cli`)](../tailtriage-cli/README.md) — analyzer/report contract and CLI usage.

## Capture surfaces

- [Controller README (`tailtriage-controller`)](../tailtriage-controller/README.md) — repeated bounded windows in long-lived services, TOML config, and reload behavior.
- [Tokio runtime sampler README (`tailtriage-tokio`)](../tailtriage-tokio/README.md) — optional runtime-pressure enrichment and Tokio-specific constraints.
- [Axum adapter README (`tailtriage-axum`)](../tailtriage-axum/README.md) — middleware/extractor ergonomics and framework-boundary behavior.

## Practical measurement guidance

- [Diagnostic validation](diagnostic-validation.md) — corpus-driven diagnostic quality validation methodology and metrics.


- [Runtime cost measurement](runtime-cost.md) — reproducible overhead attribution path (baked-in, core, sampler, post-limit/drop-path).
- [Collector limits and stress guidance](collector-limits.md) — sustained-load truncation onset, artifact-size growth, and memory trend interpretation.

## Demos and architecture

- [Getting started with demos](getting-started-demo.md) — which demos to run first and how to validate scenario outcomes.
- [Architecture](architecture.md) — how the default crate, focused crates, and CLI fit into one file-based triage pipeline.
- [Demos catalog](../demos/README.md) — scenario details and fixture layout.
