# Diagnostic validation

`tailtriage` demos are educational scenario walkthroughs and smoke checks. The diagnostic validation corpus is where we measure analyzer quality against labeled cases.

## What ground truth means here

Ground truth is the injected or independently known dominant bottleneck family for a case. It is used to evaluate triage ranking quality, not to claim causal proof.

## Metrics

`scripts/diagnostic_benchmark.py` reports:

- top-1 accuracy
- top-2 recall
- per-ground-truth counts
- confusion matrix
- confidence-bucket accuracy
- required evidence pass rate
- unexpected warning count
- failed case details

Top-1 is not expected to be perfect. Mixed or correlated cases can be valid when the ground truth appears in top-2.

## Evidence and warning validation

Each case can require evidence substrings via `must_include_evidence`.
Warnings are validated with `allowed_warnings`; unmatched warnings count as unexpected failures.

## Confidence calibration checks

The benchmark groups top-1 correctness by primary suspect confidence bucket (`high`, `medium`, `low`) so we can track whether confidence levels remain directionally calibrated.

## Add a new case

1. Add or reuse an artifact (prefer existing demo analysis fixtures first).
2. Add a case entry in `validation/diagnostics/manifest.json`.
3. Set `ground_truth`, `acceptable_top2`, evidence checks, warning expectations, and notes.
4. Run benchmark and tests.

## Run locally

```bash
python3 scripts/diagnostic_benchmark.py --manifest validation/diagnostics/manifest.json
python3 scripts/diagnostic_benchmark.py --manifest validation/diagnostics/manifest.json --output target/diagnostic-benchmark.json
python3 -m unittest scripts.tests.test_diagnostic_benchmark
```

This validation layer measures diagnostic behavior quality over a labeled corpus; it does not prove causal certainty.
