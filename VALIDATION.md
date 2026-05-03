# Validation

## Summary
`tailtriage` is a triage tool, not a root-cause proof engine. Validation checks whether evidence-ranked suspects are useful and stable on controlled Tokio workloads.

## Scope of validation
Validation is machine-scoped and workload-scoped. Current foundation is deterministic fixture-driven diagnostic validation.

## Claims validated
Under controlled and documented Tokio service workloads, tailtriage reliably classifies the dominant bottleneck family, avoids high-confidence claims when evidence is weak or mixed, exposes truncation/partial-data limits, and provides useful next checks.

## Claims not validated
No claim of universal production overhead, one-run root-cause proof, replacement for tracing/metrics/tokio-console, correctness with missing critical instrumentation, or perfect blocking-vs-executor separation on stable Tokio in all cases.

## Methodology
Use committed analysis-report fixtures in `validation/diagnostics/manifest.json` and run `scripts/diagnostic_benchmark.py` for deterministic scoring.

## Diagnostic matrix
See `validation/diagnostics/latest/scorecard.md`.

## Scenario results
See benchmark JSON output and scorecard for case-level status.

## Overhead validation
Measured separately via `docs/runtime-cost.md` and related scripts.

## Collector-limit validation
Measured separately via `docs/collector-limits.md` and related scripts.

## Reproducibility
Corpus fixtures are committed and benchmark logic is deterministic.

## CI coverage
Benchmark unit tests are in `scripts/tests/test_diagnostic_benchmark.py`; docs contracts and demo drift checks remain in CI.

## Known limitations
This is a first deterministic corpus and not yet repeated-run perturbation validation.

## Future validation work
Add repeated-run stability, perturbation robustness, real-service case studies, and integrated overhead/limit scorecards.
