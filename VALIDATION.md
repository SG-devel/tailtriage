# Validation

## Summary
`tailtriage` is a triage tool, not root-cause proof. It produces evidence-ranked suspects and next checks, where suspects are leads and not causal certainty.

## Current validation status
This repository includes an initial deterministic validation corpus for controlled Tokio workload fixtures. The corpus and benchmark validate bounded diagnostic behavior on committed fixtures, not universal production behavior.

## Evidence levels

| Level | Runs in CI? | What it supports | What it does not prove |
|---|---|---|---|
| Unit/helper tests | Yes | script/helper correctness checks for validation tooling | end-to-end diagnostic behavior by itself |
| Deterministic corpus | No (manual/local) | bounded analyzer/report behavior on committed fixtures | production root cause certainty or universal accuracy |
| Repeated-run matrix | No (manual/local) | stability metrics across repeated controlled runs on one machine/workload profile | universal stability across production environments |
| Mitigation matrix | No (manual/local) | baseline vs mitigated movement checks for next-check usefulness | formal causal proof |
| Runtime-cost measurement | Partially (non-blocking measure in CI) | overhead measurement under documented synthetic workloads | universal production overhead guarantees |
| Collector-limit stress | Yes (smoke profile + summary validation) | bounded drop/truncation/warning/downgrade behavior under stress | zero drops under all load |
| Real-service validation | No (planned) | future curated real-service truth checks when artifacts exist | current real-service validation coverage |

## Deterministic corpus validation
The deterministic benchmark validates:
- evidence-ranked suspect correctness against corpus labels
- required top-2 visibility (`required_top2` appears in primary or first secondary)
- warning expectations (`expected_warnings` required; unexpected warnings rejected unless explicitly allowed)
- required evidence substrings
- required next-check substrings when required by a case
- case-level confidence ceilings (`max_primary_confidence`) for sparse/missing/truncated/mixed evidence humility checks

The corpus includes deterministic adversarial validation that checks sparse, missing, truncated, or mixed evidence is warned about and does not produce overconfident unsupported classifications.

## Repeated-run matrix validation (manual/local)
`scripts/run_diagnostic_matrix.py` provides repeated-run validation for controlled demo scenarios (queue, blocking, executor, downstream; optional mixed).

It writes raw JSONL run records plus summary JSON (and optional Markdown scorecard) for stability metrics including top-1 accuracy, top-2 recall, high-confidence-wrong count, per-scenario primary stability, confidence bucket accuracy, and p95/p99 latency distribution summaries.

This repeated-run validation is manual/local (not mandatory CI). Publishable repeated-run outputs are generated locally and are not committed by default. Results are machine/workload scoped.

## Mitigation matrix validation (manual/local)
`scripts/run_mitigation_matrix.py` runs paired baseline/mitigated controlled demo scenarios and compares latency plus evidence movement for targeted mitigations.

It writes JSONL pair records, summary JSON, and optional scorecard Markdown under `target/` paths. Generated outputs are local/manual and are not committed by default.

Mitigation validation checks whether expected evidence-ranked suspect movement appears under controlled workloads (for example: queue-share drops, service-share drops, blocking queue-depth drops, and explainable top-2/primary movement), while treating score movement as intra-report ranking signal rather than absolute cross-report severity.

This workflow is machine/workload scoped and supports triage next checks. Mitigation movement is not formal causal proof.

## Runtime-cost / operational validation
Operational validation has dedicated domain folders under `validation/runtime-cost/` and `validation/collector-limits/`.

`scripts/run_operational_validation.py` adds manual/local operational validation for runtime-cost and collector-limit behavior. It emits raw JSONL records, stable summary JSON, and optional scorecard markdown under `target/operational-validation/`.

Runtime-cost results are machine/workload/profile scoped and are not universal production guarantees.

## Collector-limit validation
Collector-limit validation checks visible bounded drops, truncation warnings, and confidence downgrade behavior.

It does not claim no drops.

## Real-service validation (future)
Real-service validation is planned for curated anonymized real-service artifacts.

## Unified validation runner
Use `scripts/validate_all.py` to orchestrate existing validation tracks through explicit profiles (`smoke`, `ci`, `full`, `publish`).

The unified runner orchestrates existing scripts; it does not replace domain runners or change analyzer behavior.

## Validation non-claims
Validation does not claim:
- root-cause proof from one run
- universal production overhead
- replacement for tracing, metrics, tokio-console, or tokio-metrics
- real-service validation unless curated real-service artifacts exist
- mitigation movement as formal causal proof

Demos teach scenarios; validation measures bounded diagnostic behavior.
