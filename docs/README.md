# tailtriage documentation

This is the canonical user documentation index. Start with the task you need; each linked page owns its detail.

## Start with a user task

| Task | Owner |
| --- | --- |
| Make a first meaningful capture and complete the default journey | [User guide](user-guide.md) |
| Turn one report into one controlled next check | [Analyzer guide](analyzer-guide.md) |
| Roll out and operate bounded captures in a long-lived service | [Production operations guide](operations.md) |
| Choose an integration or focused package boundary | [Integrations and packages](#integrations-and-package-boundaries) |
| Understand evidence quality, validation, and limits | [Evidence and limits](#evidence-quality-validation-and-limits) |
| Inspect exact analyzer behavior | [Analyzer behavior reference](diagnostics.md) |
| Use CLI commands and saved-artifact policy | [`tailtriage-cli` README](../tailtriage-cli/README.md) |

## First journey

1. [Choose, install, instrument, finalize, analyze, and rerun](user-guide.md).
2. [Read the strongest lead and choose one next check](analyzer-guide.md).
3. [Move the capture into a production operating window](operations.md) when needed.

Suspects are evidence-ranked triage leads, not proof of root cause.

## Integrations and package boundaries

The `tailtriage` façade is the default new-integration entry point. Use a focused crate directly only for an intentionally narrower boundary.

- [Façade capture and feature selection](../tailtriage/README.md)
- [Core evidence and capture lifecycle](../tailtriage-core/README.md)
- [Repeated controller-managed captures](../tailtriage-controller/README.md)
- [Tokio runtime sampling and primitive helpers](../tailtriage-tokio/README.md)
- [Axum request-boundary integration](../tailtriage-axum/README.md)
- [Tracing intake](../tailtriage-tracing/README.md)
- [Typed in-process analysis](../tailtriage-analyzer/README.md)
- [CLI analysis and import](../tailtriage-cli/README.md)

## Evidence quality, validation, and limits

These public trust surfaces state what evidence supports and what it does not support. Generated measurements remain machine-, workload-, and profile-scoped rather than universal guarantees.

- [Diagnostic validation](diagnostic-validation.md)
- [Validation map and non-claims](dev/VALIDATION.md)
- [Runtime-cost measurement](runtime-cost.md)
- [Collector limits](collector-limits.md)
- [Security policy](../SECURITY.md)

## Complete user and reference index

- [Repository overview and quick start](../README.md)
- [User guide](user-guide.md)
- [Analyzer guide](analyzer-guide.md)
- [Production operations guide](operations.md)
- [Analyzer behavior reference](diagnostics.md)
- [Getting-started deterministic demo](getting-started-demo.md)
- [Demo index](../demos/README.md)
- [Architecture](architecture.md)
- [Normative product and data contract](../SPEC.md)
- [Versioned user-visible changes](../CHANGELOG.md)

## Maintainer and contributor navigation

Repository development, contribution guidance, implementation/design ownership, invariant proof ownership, validation workflow, and manual release procedure route through the [developer and maintainer index](dev/README.md). They are not steps in the first-user journey.
