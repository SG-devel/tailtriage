# Diagnostic validation corpus

Purpose: provide machine-readable labeled cases for deterministic analyzer validation.

Demos vs validation:
- Demos teach scenarios.
- Validation measures diagnostic quality against labels.

## Manifest format
`manifest.json` contains a `cases` array. Each case defines artifact path, label, acceptable top-2 set, evidence requirements, warning allowances, and rationale notes.

## Labeling guidance
- `ground_truth` should be independently justified by scenario design, not by analyzer output.
- `acceptable_top2` must include `ground_truth`; include close alternates only when scenario intentionally mixes signals.
- `must_include_evidence` should use meaningful substrings (for example `Queue wait`, `Blocking queue depth`, `Stage`).
- `allowed_warnings` should list only expected warning substrings.

## Synthetic fixtures
Add synthetic analysis reports only for gaps that committed demo fixtures do not cover (e.g., insufficient evidence, truncation, missing instrumentation warnings, weak/mixed ambiguity).

## Run benchmark
```bash
python3 scripts/diagnostic_benchmark.py --manifest validation/diagnostics/manifest.json
python3 scripts/diagnostic_benchmark.py --manifest validation/diagnostics/manifest.json --output target/diagnostic-benchmark.json
```
