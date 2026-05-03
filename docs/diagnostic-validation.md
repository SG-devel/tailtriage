# Diagnostic validation methodology

`tailtriage` validation checks diagnosis quality for triage. It does not provide root-cause proof.

## Methodology
The benchmark evaluates a deterministic corpus of analyzer reports against workload-grounded labels. It checks suspect ranking behavior, evidence/warning expectations, and bounded failure semantics.

## Deterministic vs repeated-run validation
The current gate is deterministic fixture validation. Repeated-run variance validation is future work.

## Top-1 vs required top-2
- **Top-1**: primary suspect matches `ground_truth`.
- **Required top-2**: every kind in `required_top2` appears in primary or first secondary suspect.

## `acceptable_primary`
`acceptable_primary` defines which primary kinds are acceptable for ambiguous/mixed interpretation and high-confidence-wrong classification. It does not replace `required_top2`.

## High-confidence-wrong count
`high_confidence_wrong_count` increments when primary confidence is `high`/`very_high` and primary kind is outside `acceptable_primary`.

## Confidence calibration
The scorecard includes confidence-bucket accuracy summaries (low/medium/high buckets) as calibration hints, not probability guarantees.

## Evidence validation
`must_include_evidence` substrings must appear in primary or secondary evidence.

## Warning validation
- `expected_warnings` substrings are required.
- observed warnings are allowed only if they match `expected_warnings` or `allowed_warnings`.

## Negative and adversarial validation
The corpus includes deterministic synthetic adversarial cases for sparse samples, missing instrumentation, truncated artifacts, and mixed-signal workloads. These cases validate triage humility and evidence-ranked suspect visibility under partial data.

## Confidence ceilings (`max_primary_confidence`)
Case-level confidence ceilings enforce conservative confidence behavior for conditions where data is sparse, missing, truncated, noisy, or intentionally ambiguous. A case fails if primary confidence exceeds its configured ceiling.

This check validates humility in diagnosis ranking behavior. It does not claim calibrated truth probability.

## Insufficient-evidence validation
The corpus includes insufficient-evidence scenarios to validate conservative fallback behavior and warning handling when signal is limited.

## Synthetic corpus fixture type
`synthetic_analysis_report` entries are small, hand-readable, report-shaped fixtures used only to cover gaps that real demo fixtures do not cover.

## Next-check validation status
Schema supports `must_include_next_checks`, but the current initial corpus has no non-empty next-check requirements, so next-check substrings are not currently part of the deterministic gate.

## Future work
Mitigation validation, overhead integration, collector-limit integration, and expanded real-service validation are separate follow-on work.

## Repeated-run diagnostic matrix (manual/local)
Use `scripts/run_diagnostic_matrix.py` to run controlled demo scenarios repeatedly, analyze each run, and summarize stability metrics.

This repeated-run matrix complements deterministic fixture validation:
- deterministic fixture validation checks bounded behavior on committed fixtures;
- repeated-run matrix validation checks repeated-run stability on controlled demo workloads.

Key repeated-run metrics:
- **Top-1**: primary suspect matches scenario ground truth.
- **Top-2**: expected causes remain visible in primary + first secondary suspects.
- **High-confidence-wrong**: high/very-high confidence primary suspect is outside acceptable primary kinds.
- **Primary stability**: most-common primary suspect frequency within a scenario.
- **Confidence bucket accuracy**: top-1 accuracy within each confidence bucket.
- **p95 IQR**: interquartile range of repeated p95 latency values (variance summary only, not a gate yet).

Repeated-run validation is manual/local in the current phase (not mandatory CI). Results are workload- and machine-scoped and support triage quality inspection under bounded controlled Tokio workloads; they are not root-cause proof.

