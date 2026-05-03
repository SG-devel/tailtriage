# VALIDATION

## Summary

`tailtriage` validation establishes whether evidence-ranked suspects are useful and reliable under controlled Tokio workloads.

`tailtriage` is a triage tool, not a root-cause proof engine.

## Scope of validation

Validation currently focuses on deterministic analyzer-report fixtures and machine-readable diagnostic checks.
Validation is machine-scoped and workload-scoped.

## Claims validated

Under controlled and documented Tokio service workloads, tailtriage reliably classifies the dominant bottleneck family, avoids high-confidence claims when evidence is weak or mixed, exposes truncation/partial-data limits, and provides useful next checks.

## Claims not validated

- universal production overhead
- root-cause proof from one run
- replacement for tracing, metrics, tokio-console, or production observability
- correct diagnosis when critical instrumentation is missing
- precise blocking-vs-executor separation on stable Tokio in all cases

## Methodology

Validation corpus: `validation/diagnostics/manifest.json`.
Benchmark: `scripts/diagnostic_benchmark.py`.

The benchmark enforces manifest integrity, scores top-1/top-2 outcomes, checks required evidence substrings, and checks warning expectations.

## Diagnostic matrix

The matrix covers queue, blocking, executor, downstream, mixed, and insufficient-evidence cases using committed demo fixtures plus small synthetic reports for warning/edge coverage.

## Scenario results

Initial results are published as deterministic benchmark summary output and reflected in `validation/diagnostics/latest/scorecard.md`.

## Overhead validation

Runtime overhead validation is tracked separately in `docs/runtime-cost.md` and is not folded into this diagnostic benchmark yet.

## Collector-limit validation

Collector-limit behavior is tracked separately in `docs/collector-limits.md` and is not folded into this diagnostic benchmark yet.

## Reproducibility

Use committed fixtures and run:

```bash
python3 scripts/diagnostic_benchmark.py --manifest validation/diagnostics/manifest.json
```

## CI coverage

The benchmark script has unit tests in `scripts/tests/test_diagnostic_benchmark.py` and documentation contracts remain enforced by existing docs validators.

## Known limitations

- deterministic fixtures are not full production variability
- ground-truth labels are scenario-scoped and not universal
- warning/cue checks use substring matching

## Future validation work

- repeated-run framework
- perturbation validation
- integrated overhead and collector-limit publication
- real-service validation corpus expansion
