# Diagnostic validation corpus

This corpus validates diagnostic behavior. Demos teach scenarios; validation measures diagnostic quality.

- Manifest: `validation/diagnostics/manifest.json`
- Ground truth labels are bottleneck families, not root-cause proof.
- `acceptable_top2` must include `ground_truth` and may include a plausible co-dominant family.
- `must_include_evidence` uses meaningful substrings, not exact full lines.
- `allowed_warnings` permits expected warning substrings only.
- Add synthetic fixtures only to cover explicit gaps (insufficient evidence, truncation, missing-signal warnings, weak/mixed ambiguity).

Run:

```bash
python3 scripts/diagnostic_benchmark.py --manifest validation/diagnostics/manifest.json
```
