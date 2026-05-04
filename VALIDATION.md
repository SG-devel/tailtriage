# Validation

## Summary
`tailtriage` is a triage tool, not root-cause proof. It produces evidence-ranked suspects and next checks, where suspects are leads and not causal certainty.

## What this PR establishes
This PR introduces an initial deterministic validation corpus for controlled Tokio workload fixtures. The corpus and benchmark validate bounded diagnostic behavior on committed fixtures, not universal production behavior.

## Deterministic checks
The deterministic benchmark validates:
- evidence-ranked suspect correctness against corpus labels
- required top-2 visibility (`required_top2` appears in primary or first secondary)
- warning expectations (`expected_warnings` required; unexpected warnings rejected unless explicitly allowed)
- required evidence substrings
- case-level confidence ceilings (`max_primary_confidence`) for sparse/missing/truncated/mixed evidence humility checks

The corpus now includes deterministic adversarial validation that checks sparse, missing, truncated, or mixed evidence is warned about and does not produce overconfident unsupported classifications.

## What this does **not** validate
- root-cause proof from one run
- universal production overhead claims
- replacement of tracing/metrics/tokio-console
- mitigation-effect validation
- overhead integration into diagnostic accuracy scoring
- collector-limit integration into diagnostic accuracy scoring
- real-service validation coverage

## Execution model
- `scripts/diagnostic_benchmark.py` is currently a local/manual deterministic gate.
- Benchmark helper unit tests run in CI (`python3 -m unittest scripts.tests.test_diagnostic_benchmark`).

## Related artifacts
- corpus contract: `validation/diagnostics/README.md`
- corpus data: `validation/diagnostics/manifest.json`
- current scorecard: `validation/diagnostics/latest/scorecard.md`
- user-facing methodology: `docs/diagnostic-validation.md`

Demos teach scenarios; validation measures bounded diagnostic behavior.

## Repeated-run matrix validation (manual/local)
`scripts/run_diagnostic_matrix.py` provides repeated-run validation for controlled demo scenarios (queue, blocking, executor, downstream; optional mixed).

It writes raw JSONL run records plus summary JSON (and optional Markdown scorecard) for stability metrics including top-1 accuracy, top-2 recall, high-confidence-wrong count, per-scenario primary stability, confidence bucket accuracy, and p95/p99 latency distribution summaries.

This repeated-run validation is currently manual/local (not mandatory CI). Publishable repeated-run outputs are generated locally and are not committed by default. Results are machine/workload scoped. It measures stability under bounded controlled Tokio demo workloads on a specific machine/profile; it does not establish production universality or root-cause proof.

## Mitigation matrix validation (manual/local)
`scripts/run_mitigation_matrix.py` runs paired baseline/mitigated controlled demo scenarios and compares latency plus evidence movement for targeted mitigations.

It writes JSONL pair records, summary JSON, and optional scorecard Markdown under `target/` paths. Generated outputs are local/manual and are not committed by default.

Mitigation validation checks whether expected evidence-ranked suspect movement appears under controlled workloads (for example: queue-share drops, service-share drops, blocking queue-depth drops, and explainable top-2/primary movement), while treating score movement as intra-report ranking signal rather than absolute cross-report severity.

This workflow is machine/workload scoped and supports triage next checks. It does not prove root cause and is not mandatory CI.

## Operational validation (manual/local)

Operational validation now has dedicated domain folders under `validation/runtime-cost/` and `validation/collector-limits/`.

`scripts/run_operational_validation.py` adds manual/local operational validation for runtime-cost and collector-limit behavior. It emits raw JSONL records, stable summary JSON, and optional scorecard markdown under `target/operational-validation/`. Diagnostics scorecards may reference these operational domains, but diagnostics is not the only operational validation location.

Non-claims remain explicit: runtime-cost is machine/workload/profile scoped (not a universal production guarantee), collector-limit checks verify visible bounded drops plus downgrade/warning behavior (not never-drop), and results do not provide root-cause proof.

## Unified validation runner

`scripts/validate_all.py` orchestrates existing validation scripts through explicit profiles; it does not replace domain runners or change validation semantics.

Profiles:
- `smoke`: fast local sanity checks.
- `ci`: deterministic/script-test validation suitable for CI adoption.
- `full`: manual/local comprehensive run with repeated-run + mitigation + operational tracks.
- `publish`: full run plus publish-style artifact packaging.

Default generated-output policy:
- `smoke`, `ci`, and `full` write under `target/validation/<profile>/`.
- `publish` can write under `validation/artifacts/<date>-git-<sha>/` when requested.
- Generated outputs are local by default and should not be committed unless an explicit publish/release process decides to do so.

Non-claims remain explicit in unified scorecards:
- no root-cause proof,
- runtime-cost remains machine/workload scoped,
- collector-limit checks do not claim zero drops.
