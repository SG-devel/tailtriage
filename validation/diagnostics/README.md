# Diagnostic validation corpus contract

This directory defines the deterministic diagnostic-validation corpus used by `scripts/diagnostic_benchmark.py`.

Demos teach; validation measures.

The analyzer artifacts are manually authored and committed. `analyzer-fixtures.lock.json` uses the self-identifying `tailtriage.analyzer-fixture-lock.v1` format to record their manifest-owned inventory, exact bytes, text formatting, and compact structural shape. It is an integrity lock, not a fixture generator. The checker also rejects byte-identical analyzer inputs assigned to distinct accuracy observations; multiple encodings for the same observation may share bytes. Normal CI checks the lock before running the deterministic corpus benchmark against `validation/diagnostics/manifest.json`; durable/versioned scorecards remain manually generated snapshot artifacts from `.github/workflows/validation-snapshot.yml`.

## Validation classes and schema

Manifest schema version 2 requires every case to declare an artifact type, `validation_class`, and `accuracy_eligible`. Raw `run_artifact` cases and stable `tracing_span_jsonl` cases are `analyzer_execution`; the latter import first and both analyze at benchmark execution. Their execution expectation defaults to `success`; typed failure contracts declare `failure_stage`, required and forbidden diagnostics, and stdout expectations. Run artifacts default to strict policy and may explicitly select the existing `allow_ambiguous` CLI policy.

An accuracy-eligible analyzer case declares `observation_id` and `ground_truth`. Encodings of the same logical workload share an observation ID, must have consistent labels and diagnosis/confidence output, and count once. These unique accuracy-eligible observations are the only diagnosis-accuracy denominator. Ground truth is controlled fixture intent, not production truth or root-cause proof.

Pre-generated `analysis_report` and report-shaped `synthetic_analysis_report` cases are `report_contract`, are always accuracy ineligible, and contain neither ground truth nor observation IDs. They inspect Report suspects, evidence, next checks, confidence, warnings, route breakdowns, and temporal segments without executing an importer or analyzer. Report-contract cases do not contribute to top-1, top-2, confusion, confidence-bucket, per-ground-truth, or high-confidence-wrong metrics. Exact warning allowlisting remains mandatory.

Case diagnosis contracts use `expected_primary_kinds`, `required_visible_suspects` (primary or first secondary), and optional `exact_primary_kind`. Analyzer success cases may be execution-only; expected execution failures must be accuracy ineligible.

The integrity checker deliberately stops at inventory, bytes, formatting, and compact shape. The benchmark remains the typed and semantic validation boundary: raw Run fixtures are decoded and strictly validated through the CLI, while stable tracing JSONL fixtures pass through the public tracing importer before analysis. Matching lock hashes does not prove diagnosis correctness, and fixture labels remain controlled intent rather than production truth.

## Fixture ownership rule

Add a committed analyzer fixture only when it protects an artifact-format, importer, CLI, warning, or end-to-end regression boundary. Do not add fixtures solely to achieve one fixture per diagnosis family.

The diagnostic manifest and benchmark own corpus classification and accounting. The analyzer fixture lock and integrity checker protect committed analyzer artifacts. Typed analyzer tests own diagnosis-rule coverage.

Bounded demo smoke and parity checks may run in CI. Repeated-run and mitigation matrices remain local/manual and machine-scoped. Generated Runs, Reports, summaries, and matrix outputs remain under `target/` and are not committed.

Run `python3 scripts/check_diagnostic_fixture_integrity.py` to check the committed lock. For an intentional analyzer-fixture change, run `python3 scripts/check_diagnostic_fixture_integrity.py --refresh`, review the byte and shape changes, and commit the updated lock with the manually edited fixture. Refresh modifies only the lock.

## Running the benchmark

```bash
python3 scripts/diagnostic_benchmark.py \
  --manifest validation/diagnostics/manifest.json \
  --min-top1 0.75 \
  --min-top2 0.90 \
  --max-high-confidence-wrong 0
```

Optional JSON output:

```bash
python3 scripts/diagnostic_benchmark.py \
  --manifest validation/diagnostics/manifest.json \
  --output target/diagnostic-benchmark.json \
  --min-top1 0.75 \
  --min-top2 0.90 \
  --max-high-confidence-wrong 0
```

## Validation tracks
- deterministic corpus benchmark: `scripts/diagnostic_benchmark.py`
- repeated-run controlled matrix runner: `scripts/run_diagnostic_matrix.py`
- mitigation matrix runner: `scripts/run_mitigation_matrix.py`

The deterministic corpus checks fixture-labeled contract behavior. The repeated-run runner checks repeated-run stability for selected controlled demo workloads.

Validation tracks currently include deterministic corpus benchmark, adversarial synthetic coverage (inside the corpus), repeated-run diagnostic matrix, mitigation matrix workflows, and operational validation for runtime cost and collector limits. Operational validation now has dedicated domain folders under `validation/runtime-cost/` and `validation/collector-limits/`; diagnostics references them but is not the only operational validation location. Generated operational outputs remain under `target/operational-validation/` and are not committed by default.

## Unified runner orchestration

For profile-based orchestration across validation tracks, use `scripts/validate_all.py` (`smoke`, `ci`, `full`, `publish`). Keep using this diagnostics runner directly for diagnostics-specific validation workflows.


## Manual scorecard generation
Use `.github/workflows/validation-snapshot.yml` to generate durable diagnostic snapshots through manual workflow dispatch. Normal CI does not upload durable diagnostic scorecards.

Snapshot output directory: `target/validation/diagnostics/`
- `benchmark-summary.json`
- `environment.json`
- `scorecard.md`

`environment.json` includes `tailtriage` workspace version and per-crate versions, git metadata, GitHub Actions metadata when available, software/hardware metadata, manifest hash, referenced-artifact hash, and benchmark thresholds.

Deterministic fixture metrics validate committed fixtures only; they are not root-cause proof, universal production accuracy, universal production overhead, or real-service validation.


Optional manifest fields can validate expanded analyzer report surface on selected cases only: `expected_evidence_quality`, `expected_signal_statuses`, `must_include_confidence_notes`, `expected_route_breakdowns`, `expected_temporal_segments`, `must_include_route_warning`, `must_include_temporal_warning`, and `expected_top_level_warnings`. These checks are fixture-scoped and optional; cases that omit them continue to validate under the existing suspect/evidence/warning contract.


## Partial queue/stage evidence

Completed queue and stage distributions exclude partial observations. Partial durations are observed lower bounds: tailtriage observed the helper from first poll until Drop, not proof that the underlying operation completed, failed, or stopped. Partial evidence remains visible in event totals, evidence-quality limitations, top-level warnings, and suspect evidence.

Queue/service public p95 fields remain completed-only. A queue or downstream-stage suspect materially relying on an observed-lower-bound path cannot exceed medium confidence; partial evidence that does not affect selected eligibility or score does not automatically cap a completed candidate. Partial stage `success = false` is not interpreted as a completed operation failure.

Global, route, and temporal projections share this policy. Tracing imports remain completed-only. Completed-only Report JSON and text remain unchanged; mixed or partial Runs may change scores or ranking only when explicitly labeled lower-bound evidence is selected and qualified. Suspects remain triage leads, not root-cause proof.
