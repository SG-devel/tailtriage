# Diagnostic validation methodology

`tailtriage` validation checks diagnosis quality for triage. It does not provide root-cause proof.

## Methodology
The benchmark evaluates a deterministic corpus of analyzer reports against workload-grounded labels. It checks suspect ranking behavior, evidence/warning expectations, and bounded failure semantics.

## Deterministic vs repeated-run validation
The current gate is deterministic fixture validation. Repeated-run variance validation is available as a manual/local workflow.

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
Overhead integration, collector-limit integration, and expanded real-service validation are separate follow-on work.

## Mitigation matrix validation (manual/local)
A manual mitigation matrix runner is available at `scripts/run_mitigation_matrix.py`. It compares degraded/baseline runs against targeted mitigated runs for controlled demos and summarizes whether expected latency/evidence movement occurs.

Typical expected movement by bottleneck family:
- queue-oriented scenarios: p95 improves and queue-share evidence weakens
- downstream-stage scenarios: p95 improves and service/stage share evidence weakens
- blocking scenarios: p95 improves and blocking queue-depth evidence weakens
- db/pool scenarios: p95 improves and queueing/service evidence moves in an explainable direction

Important interpretation rule: suspect score changes are evidence-ranking changes inside each report, not absolute severity values across reports. Mitigation validation therefore uses concrete movement checks (latency, share/depth metrics, and explainable top suspect movement), not score-drop-only gating.

Like repeated-run validation, mitigation validation is manual/local, machine/workload scoped, and designed for triage guidance and next checks. It does not prove root cause.

## Repeated-run diagnostic matrix validation (manual)
A manual repeated-run matrix runner is available at `scripts/run_diagnostic_matrix.py`. It repeatedly executes controlled demo scenarios, analyzes each run, and summarizes stability metrics.

This complements deterministic fixture validation:
- deterministic fixtures validate stable contract behavior on committed artifacts
- repeated-run matrix validation measures stability across repeated controlled runs on a specific machine/workload profile

Key repeated-run metrics:
- **Top-1 stability**: fraction of runs where the primary suspect matches the scenario ground truth
- **Top-2 visibility**: fraction of runs where required causes appear in the top-2 suspects
- **High-confidence-wrong count**: runs where primary confidence is high/very_high but primary kind is outside acceptable primary kinds
- **Confidence bucket accuracy**: top-1 accuracy grouped by confidence bucket
- **Primary stability**: share of runs captured by the most frequent primary suspect kind
- **p95 IQR**: interquartile range of p95 latency across repeated runs

Repeated-run validation remains manual/local for now (not mandatory CI), and results are machine-scoped and workload-scoped. It supports triage confidence checks and reproducibility inspection for controlled Tokio workloads.

Like all tool output, these results are evidence for triage and next checks; they do not prove root cause.

## Operational trust-boundary validation

Operational validation complements deterministic corpus, adversarial synthetic checks, repeated-run matrix validation, and mitigation validation. Use `scripts/run_operational_validation.py` for runtime-cost and collector-limit trust boundaries with machine/workload-scoped outputs.

Operational validation has dedicated domain folders under `validation/runtime-cost/` and `validation/collector-limits/`. The diagnostics scorecard can reference these operational domains, but it is not the only operational validation location. Generated operational outputs remain under `target/operational-validation/` and are not committed by default.


## Unified runner orchestration
Diagnostic validation can run directly with domain scripts or through `scripts/validate_all.py` profiles. The unified runner orchestrates existing validation tracks and outputs; it does not replace diagnostics-specific scripts.
