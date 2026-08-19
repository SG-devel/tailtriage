# Diagnostic validation methodology

`tailtriage` validation checks diagnosis quality for triage. It does not provide root-cause proof.

## Methodology
The schema-version-2 benchmark separates three concepts. An analyzer-executed case either analyzes a raw Run or imports stable tracing JSONL and then analyzes it; its success or failure execution contract is always checked. An accuracy-eligible observation is a unique logical workload and is the only diagnosis-accuracy denominator, even when multiple artifact encodings execute independently. A report-contract case inspects a pre-generated or synthetic Report and never executes the analyzer or affects accuracy.

Top-1 means the observation primary equals ground truth. Top-2 means ground truth is primary or first secondary. High-confidence-wrong means a high-confidence primary falls outside `expected_primary_kinds`. Ground truth is controlled fixture intent, not production truth or root-cause proof. Equivalent encodings must agree on ordered diagnosis visibility and confidence bucket.

## Deterministic vs repeated-run validation
Deterministic fixture validation is mandatory in normal CI; deterministic scorecards are local/manual generated evidence. Report-contract checks cover Report fields, warnings, evidence, next checks, confidence, routes, and temporal output but are not analyzer accuracy. Repeated-run validation is a separate manual/local, machine/workload-scoped track.

## Validation responsibilities

| Layer | Purpose | Mechanism | Execution |
| --- | --- | --- | --- |
| Analyzer rule correctness | Explicit evidence selects the intended diagnosis | Typed Rust tests | Normal CI |
| Artifact pipeline regression | Representative committed artifacts pass through real intake and analyzer paths | Diagnostic corpus and integrity lock | Normal CI |
| Live workload behavior | Real demos produce expected signals and preserve integration behavior | Bounded demo smoke/parity checks plus repeated-run and mitigation matrices | CI smoke/parity; local/manual repeated runs |

The diagnostic manifest and benchmark own corpus classification and accounting. The analyzer fixture lock and integrity checker protect committed analyzer artifacts. Typed analyzer tests own diagnosis-rule coverage.

The committed diagnostic corpus proves that representative Run and tracing artifacts pass through the real decoding, import, CLI, and analyzer paths. Its `analyzer_accuracy` field measures agreement against controlled committed observation labels. It does not estimate universal or production accuracy, and the corpus does not need one analyzer fixture for every diagnosis family.

Real workloads are validated in two ways: bounded demo smoke and parity checks may run in CI, while repeated-run and mitigation matrices remain local/manual and machine-scoped. Generated Runs, Reports, summaries, and matrix outputs remain under `target/` and are not committed.

## Confidence calibration
The scorecard includes confidence-bucket accuracy summaries (low/medium/high buckets) as calibration hints, not probability guarantees.

## Evidence validation
`must_include_evidence` substrings must appear in primary or secondary evidence.

## Warning validation
- `expected_warnings` substrings are required.
- observed warnings are allowed only if they match `expected_warnings` or `allowed_warnings`.

## Negative and adversarial validation
The corpus includes deterministic synthetic adversarial cases for sparse samples, missing instrumentation, truncated artifacts, and mixed-signal workloads, plus selected deterministic raw run-artifact adversarial cases that exercise analyzer-path behavior on committed fixtures. These cases validate triage humility and evidence-ranked suspect visibility under partial data.

## Confidence ceilings (`max_primary_confidence`)
Case-level confidence ceilings enforce conservative confidence behavior for conditions where data is sparse, missing, truncated, noisy, or intentionally ambiguous. A case fails if primary confidence exceeds its configured ceiling.

This check validates humility in diagnosis ranking behavior. It does not claim calibrated truth probability.

## Insufficient-evidence validation
The corpus includes insufficient-evidence scenarios to validate conservative fallback behavior and warning handling when signal is limited.

## Synthetic corpus fixture type
`synthetic_analysis_report` entries are small, hand-readable, report-shaped fixtures used only to cover gaps that real demo fixtures do not cover.

## Next-check validation status
The corpus supports `must_include_next_checks`, and selected adversarial cases use it to validate that reports suggest relevant follow-up actions.

Next-check validation is substring-based rather than exact-output based. This keeps the diagnostic contract stable while allowing wording to improve.

## Future work
Future work is limited to:

- deeper operational coverage;
- broader workload coverage;
- curated real-service validation.

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
- **High-confidence-wrong count**: runs where primary confidence is high but primary kind is outside acceptable primary kinds
- **Confidence bucket accuracy**: top-1 accuracy grouped by confidence bucket
- **Primary stability**: share of runs captured by the most frequent primary suspect kind
- **p95 IQR**: interquartile range of p95 latency across repeated runs

Repeated-run validation remains manual/local for now (not mandatory CI), and results are machine-scoped and workload-scoped. It supports triage confidence checks and reproducibility inspection for controlled Tokio workloads.

Like all tool output, these results are evidence for triage and next checks; they do not prove root cause.

## Operational trust-boundary validation

Operational validation complements deterministic corpus, adversarial synthetic checks, repeated-run matrix validation, and mitigation validation. Use `scripts/run_operational_validation.py` for runtime-cost and collector-limit trust boundaries with machine/workload-scoped outputs.

Operational validation has dedicated domain folders under `validation/runtime-cost/` and `validation/collector-limits/`. The diagnostics scorecard can reference these operational domains, but it is not the only operational validation location. Generated operational outputs remain under `target/operational-validation/` and are not committed by default.

## Unified orchestration option

You can run diagnostic validation directly with domain scripts or orchestrate tracks with `scripts/validate_all.py` profiles.

The unified runner coordinates existing validation scripts and outputs; it does not replace or redefine diagnostics-specific validation semantics.


## Local/manual deterministic snapshots
For a provenance-rich deterministic snapshot, run `scripts/generate_diagnostic_scorecard.py` locally. It generates `benchmark-summary.json`, `environment.json`, and `scorecard.md` under the selected output directory. These files remain local evidence unless a maintainer separately archives them; normal CI does not publish GitHub artifacts.

The snapshot captures deterministic benchmark metrics, thresholds, `tailtriage` workspace and per-crate versions, git metadata, GitHub Actions runner metadata (when available), software metadata, hardware metadata, and manifest/referenced-artifact hashes.

Deterministic fixture metrics validate committed fixture behavior only. They do not prove production root cause, universal production accuracy, universal production overhead, or real-service behavior. Repeated-run, runtime-cost, and collector-limit results are more hardware-sensitive than deterministic fixture validation.


Optional manifest fields can validate expanded analyzer report surface on selected cases only: `expected_evidence_quality`, `expected_signal_statuses`, `must_include_confidence_notes`, `expected_route_breakdowns`, `expected_temporal_segments`, `must_include_route_warning`, `must_include_temporal_warning`, and `expected_top_level_warnings`. These checks are fixture-scoped and optional; cases that omit them continue to validate under the existing suspect/evidence/warning contract.

- Native/tracing scenario evidence parity checks (release CI): `validate-tracing-parity all` gates tracing contract parity for scenario coverage, expected evidence presence, route coverage, capture-mode behavior across light and investigation, and runtime/non-runtime boundaries. The queue tiny-limit retention parity gate (`validate-tracing-retention-parity`) runs in both `light` and `investigation` modes and checks retained counts, dropped counters, `truncation.limits_hit`, and `metadata.effective_core_config`; these checks do not require byte-for-byte artifact equality or exact suspect-ranking equality in every scenario.


Runtime-sensitive tracing contract parity uses deterministic manually injected runtime snapshots and must not rely on ambient background sampler noise or sampler metadata. When tracing contract parity scenarios need deterministic/manual runtime-pressure evidence, they call `manual_runtime_snapshots()` in `TracingSession` and inject snapshots with `record_runtime_snapshot(...)`. Runtime-sensitive tracing contract parity requires non-empty runtime snapshots, the scenario-specific runtime field evidence, and the explicit manual-runtime lifecycle warning. Output remains a Run artifact for later `tailtriage analyze`, and findings are evidence-ranked suspects, not proof.


## Partial queue/stage evidence

Completed queue and stage distributions exclude partial observations. Partial durations are observed lower bounds: tailtriage observed the helper from first poll until Drop, not proof that the underlying operation completed, failed, or stopped. Partial evidence remains visible in event totals, evidence-quality limitations, top-level warnings, and suspect evidence.

Queue/service public p95 fields remain completed-only. A queue or downstream-stage suspect materially relying on an observed-lower-bound path cannot exceed medium confidence; partial evidence that does not affect selected eligibility or score does not automatically cap a completed candidate. Partial stage `success = false` is not interpreted as a completed operation failure.

Global, route, and temporal projections share this policy. Tracing imports remain completed-only. Completed-only Report JSON and text remain unchanged; mixed or partial Runs may change scores or ranking only when explicitly labeled lower-bound evidence is selected and qualified. Suspects remain triage leads, not root-cause proof.

## Native/tracing equivalence validation

Two complementary tracks protect intake parity. Normal Cargo tests use committed native Run, stable completed-span JSONL, and independent expected fixtures to compare exact shared completed evidence and comparable analyzer results. This deterministic contract is not a claim of production diagnostic accuracy.

The live demo track exercises real native and tracing capture across scenarios and modes with machine-sensitive semantic checks; it is neither byte-exact nor formal causal proof. Completed-span JSONL cannot carry partial stage/queue, runtime, in-flight, sampler, or complete lifecycle/truncation state. Run JSON remains the complete artifact.
