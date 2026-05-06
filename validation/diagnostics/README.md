# Diagnostic validation corpus contract

This directory defines the deterministic diagnostic-validation corpus used by `scripts/diagnostic_benchmark.py`.

Demos teach; validation measures.

Normal CI runs the deterministic corpus benchmark against `validation/diagnostics/manifest.json` as a required gate for schema/corpus drift. Durable/versioned scorecards remain manual/tag snapshot artifacts from `.github/workflows/validation-snapshot.yml`.

## Case schema fields

- `schema_version`: manifest schema version (currently `1`).
- `artifact`: path to the analyzer-report fixture, relative to `manifest.json`.
- `artifact_type`:
  - `analysis_report`: real demo-emitted analyzer report fixture.
  - `synthetic_analysis_report`: hand-written report-shaped synthetic fixture used for coverage gaps.
- `ground_truth`: expected diagnostic family for the controlled fixture intent. It does not mean production root-cause proof.
- `required_top2`: diagnosis kinds that must appear in primary or first secondary suspect. Usually `[ground_truth]`. Must include `ground_truth`.
- `acceptable_primary`: diagnosis kinds acceptable as primary for mixed/ambiguous interpretation. Must include `ground_truth`. This does **not** satisfy `required_top2` by itself.
- `top1_required`: when `true`, primary kind must equal `ground_truth`.
- `max_primary_confidence`: optional confidence ceiling for primary suspect (`low|medium|high`).
- `must_include_evidence`: evidence substrings that must appear in primary or secondary evidence.
- `must_include_next_checks`: next-check substrings that must appear when required by a case. Selected adversarial cases use this to validate relevant follow-up guidance.
- `expected_warnings`: warning substrings that must appear.
- `allowed_warnings`: warning substrings that may appear in addition to expected warnings (tolerated extras only).
- `expected_top_level_warnings` (optional): additional required top-level warning substrings; can be used as a stricter supplement to `expected_warnings` for selected cases.
- `expected_evidence_quality` (optional): expected evidence quality bucket (`strong|partial|weak`) from `report.evidence_quality.quality`.
- `expected_signal_statuses` (optional): expected signal-family status map (`requests|queues|stages|runtime_snapshots|inflight_snapshots` -> `present|missing|partial|truncated`).
- `must_include_confidence_notes` (optional): confidence-note substrings that must appear across primary + secondary suspects.
- `expected_route_breakdowns` (optional): route-breakdown presence check (`empty|non_empty`).
- `expected_temporal_segments` (optional): temporal-segment presence check (`empty|non_empty`).
- `must_include_route_warning` / `must_include_temporal_warning` (optional): substring checks across route-breakdown warnings and temporal-segment warnings.
- `notes`: workload-intent note explaining why labels are set.
- `tags`: non-empty string tags for grouping/filtering.

## Corpus discipline

- Label by fixture/workload intent, not by analyzer output.
- `required_top2` and `acceptable_primary` are different:
  - `required_top2` = required visibility of true causes.
  - `acceptable_primary` = tolerated primary classification for ambiguity handling/high-confidence-wrong interpretation.
- Do not use wildcard warning allowlists (`"*"` is invalid).
- Optional report-surface checks are case-scoped. Do not require every corpus case to assert every optional field.
- Keep synthetic fixtures small, hand-readable, and explicitly scoped to gaps.
- Use `max_primary_confidence` for humility checks in sparse-sample, missing-instrumentation, truncation, noise-only, or close mixed-signal cases.
- Confidence ceilings validate conservative triage behavior, not truth probabilities.
- Synthetic fixtures are report-shaped adversarial coverage artifacts, not substitutes for analyzer-generated captures.

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


## Versioned/manual scorecard generation
Use `.github/workflows/validation-snapshot.yml` to generate durable diagnostic snapshots on manual dispatch or `v*` tag pushes. Normal CI does not upload durable diagnostic scorecards.

Snapshot output directory: `target/validation/diagnostics/`
- `benchmark-summary.json`
- `environment.json`
- `scorecard.md`

`environment.json` includes `tailtriage` workspace version and per-crate versions, git metadata, GitHub Actions metadata when available, software/hardware metadata, manifest hash, referenced-artifact hash, and benchmark thresholds.

Deterministic fixture metrics validate committed fixtures only; they are not root-cause proof, universal production accuracy, universal production overhead, or real-service validation.
