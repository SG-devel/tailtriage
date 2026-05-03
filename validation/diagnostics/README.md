# Diagnostic validation corpus

This corpus validates diagnostic behavior from analysis reports.

Demos teach scenarios; validation measures diagnostic quality.

## Manifest format
`manifest.json` contains `cases[]` with report path, ground truth, acceptable top-2 set, evidence requirements, warning allowances, and rationale notes.

## Labeling guidance
- `ground_truth`: dominant bottleneck family label.
- `acceptable_top2`: include `ground_truth`; add neighboring class only when ambiguity is expected.
- `must_include_evidence`: meaningful substrings (e.g., `Queue wait`, `Blocking queue`, `Stage`).
- `allowed_warnings`: warning substrings expected for that case only.

## Synthetic fixtures
Add synthetic analysis reports only for gaps not represented by committed demo fixtures (for example insufficient evidence, explicit truncation warning, or missing runtime warning).

## Run benchmark
```bash
python3 scripts/diagnostic_benchmark.py --manifest validation/diagnostics/manifest.json
python3 scripts/diagnostic_benchmark.py --manifest validation/diagnostics/manifest.json --output target/diagnostic-benchmark.json
```
