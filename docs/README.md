# tailtriage documentation

This is the canonical map for the `tailtriage` documentation set. Choose the row that matches the task; linked pages own the detail rather than repeating it here.

## Documentation ownership map

| Task | Authoritative document |
| --- | --- |
| First adoption and the end-to-end user journey | [User guide](user-guide.md) |
| Production capture, rollout, retention, limits, and operations | [Production operations guide](operations.md) |
| Turn one report into one next check | [Analyzer guide](analyzer-guide.md) |
| Exact analyzer fields, mechanics, options, and limitations | [Analyzer behavior reference](diagnostics.md) |
| Analyzer rationale, tradeoffs, proof ownership, and revision criteria | [Analyzer rationale](analyzer-rationale.md) |
| CLI package commands, artifact loading, and output | [`tailtriage-cli` README](../tailtriage-cli/README.md) |
| Typed in-process analyzer API and rendering | [`tailtriage-analyzer` README](../tailtriage-analyzer/README.md) and its Rustdoc |
| Normative product, data, and analyzer contracts | [SPEC.md](../SPEC.md) |
| Validation approach, evidence, and non-claims | [VALIDATION.md](../VALIDATION.md) and [diagnostic validation](diagnostic-validation.md) |
| Architecture and durable design ownership | [Architecture](architecture.md) and [DESIGN_NOTES.md](../DESIGN_NOTES.md) |
| Versioned user-visible changes | [CHANGELOG.md](../CHANGELOG.md) |

## Integrations and package boundaries

- Capture façade: [`tailtriage`](../tailtriage/README.md)
- Evidence model: [`tailtriage-core`](../tailtriage-core/README.md)
- Repeated capture: [`tailtriage-controller`](../tailtriage-controller/README.md)
- Runtime sampling: [`tailtriage-tokio`](../tailtriage-tokio/README.md)
- Axum integration: [`tailtriage-axum`](../tailtriage-axum/README.md)
- Tracing intake: [`tailtriage-tracing`](../tailtriage-tracing/README.md)

## Measurement, validation, and limits

- [Runtime-cost measurement](runtime-cost.md)
- [Collector limits](collector-limits.md)
- [Diagnostic validation](diagnostic-validation.md)
- [Validation corpus](../validation/diagnostics/README.md) and [latest scorecard](../validation/diagnostics/latest/scorecard.md)
- [Runtime-cost validation](../validation/runtime-cost/README.md)
- [Collector-limit validation](../validation/collector-limits/README.md)

## Examples, demos, and repository reference

- [Repository overview](../README.md)
- [Getting-started demo](getting-started-demo.md)
- [Demo index](../demos/README.md)
- [Implementation plan](../IMPLEMENTATION_PLAN.md)
- [Contributing](../CONTRIBUTING.md)
- [Code of conduct](../CODE_OF_CONDUCT.md)
- [Security](../SECURITY.md)
