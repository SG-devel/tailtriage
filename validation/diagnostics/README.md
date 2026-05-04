# Diagnostic validation corpus contract

This directory defines the deterministic diagnostic-validation corpus used by `scripts/diagnostic_benchmark.py`.

Demos teach; validation measures.

## Case schema fields

- `schema_version`: manifest schema version (currently `1`).
- `artifact`: path to the analyzer-report fixture, relative to `manifest.json`.
- `artifact_type`:
  - `analysis_report`: real demo-emitted analyzer report fixture.
  - `synthetic_analysis_report`: hand-written report-shaped synthetic fixture used for coverage gaps.
- `ground_truth`: expected true diagnosis kind for the fixture intent.
- `required_top2`: diagnosis kinds that must appear in primary or first secondary suspect. Usually `[ground_truth]`. Must include `ground_truth`.
- `acceptable_primary`: diagnosis kinds acceptable as primary for mixed/ambiguous interpretation. Must include `ground_truth`. This does **not** satisfy `required_top2` by itself.
- `top1_required`: when `true`, primary kind must equal `ground_truth`.
- `max_primary_confidence`: optional confidence ceiling for primary suspect (`low|medium|high|very_high`).
- `must_include_evidence`: evidence substrings that must appear in primary or secondary evidence.
- `must_include_next_checks`: next-check substrings that must appear when required by a case. Schema-supported; current initial corpus has no required next-check cases.
- `expected_warnings`: warning substrings that must appear.
- `allowed_warnings`: warning substrings that may appear in addition to expected warnings (tolerated extras only).
- `notes`: workload-intent note explaining why labels are set.
- `tags`: non-empty string tags for grouping/filtering.

## Corpus discipline

- Label by fixture/workload intent, not by analyzer output.
- `required_top2` and `acceptable_primary` are different:
  - `required_top2` = required visibility of true causes.
  - `acceptable_primary` = tolerated primary classification for ambiguity handling/high-confidence-wrong interpretation.
- Do not use wildcard warning allowlists (`"*"` is invalid).
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

A unified orchestrator is available at `scripts/validate_all.py` for smoke/ci/full/publish profile coordination. Diagnostics-specific commands in this README remain supported directly.
