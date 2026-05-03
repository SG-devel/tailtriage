# Diagnostic validation corpus

This corpus defines machine-readable diagnostic validation for `tailtriage` analyzer reports.

- Demos teach scenarios.
- Validation measures diagnostic behavior against labeled cases.

## Manifest format

`manifest.json` is a JSON array of cases. Each case includes:
- `id` unique case id
- `artifact` report path relative to `validation/diagnostics/`
- `artifact_type` (`analysis_report` for now)
- `ground_truth` dominant bottleneck family label
- `acceptable_top2` allowed top-2 suspects (must include `ground_truth`)
- `tags` scenario tags
- `must_include_evidence` required evidence substrings
- `allowed_warnings` expected warning substrings
- `notes` independent label rationale

## Labeling guidance

- Label `ground_truth` from scenario design intent and fixture context, not analyzer output alone.
- Use `acceptable_top2` to encode known ambiguity boundaries while preserving true label.
- Keep `must_include_evidence` meaningful but substring-based.
- Keep `allowed_warnings` narrow and intentional.

Add synthetic fixtures only for coverage gaps (for example: insufficient evidence, truncation, or warning behavior).

## Run benchmark

```bash
python3 scripts/diagnostic_benchmark.py --manifest validation/diagnostics/manifest.json
python3 scripts/diagnostic_benchmark.py --manifest validation/diagnostics/manifest.json --output target/diagnostic-benchmark.json
```
