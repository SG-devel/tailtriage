# Diagnostic validation corpus

Purpose: provide machine-readable labeled cases for deterministic analyzer validation.

Demos vs validation:
- Demos teach scenarios.
- Validation measures diagnostic quality against labels.

## Manifest format
`manifest.json` contains `schema_version` and a `cases` array. Each case defines artifact path, label, acceptable top-2 required causes set, evidence requirements, expected warnings, allowed warnings, and rationale notes.

## Labeling guidance
- `ground_truth` should be independently justified by scenario design, not by analyzer output.
- `acceptable_primary` must include `ground_truth`; include close alternates only when scenario intentionally mixes signals.
- `must_include_evidence` should use meaningful substrings (for example `Queue wait`, `Blocking queue depth`, `Stage`).
- `expected_warnings` should list warning substrings that must appear.
- `allowed_warnings` should list optional warning substrings that may appear without failing the case.

## Synthetic fixtures
Synthetic cases use `artifact_type: "synthetic_analysis_report"` to distinguish them from real demo-produced `analysis_report` fixtures. These synthetic artifacts are intentionally small, hand-readable report-shaped fixtures used only to cover validation gaps (e.g., insufficient evidence, truncation, missing instrumentation warnings, weak/mixed ambiguity).

## Run benchmark
```bash
python3 scripts/diagnostic_benchmark.py --manifest validation/diagnostics/manifest.json
python3 scripts/diagnostic_benchmark.py --manifest validation/diagnostics/manifest.json --output target/diagnostic-benchmark.json
```
