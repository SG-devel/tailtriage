# Diagnostic validation

`tailtriage` demos are educational and smoke-test oriented. The diagnostic validation corpus is where diagnostic quality is measured against labeled ground-truth cases.

Ground truth means the injected or independently known bottleneck family for a case. It does not claim causal proof beyond observed data.

## Metrics

The benchmark reports:

- top-1 accuracy
- top-2 recall
- per-ground-truth counts
- confusion matrix
- confidence-bucket accuracy
- required-evidence pass rate
- unexpected warning count
- failed case list

Top-1 is not expected to be perfect. Mixed and correlated scenarios can still be diagnostically useful when the correct suspect family appears in top-2.

## Evidence and warning checks

Each case can require evidence substrings that must be found across primary and secondary suspects.

Warnings are validated too:

- expected warning text can be explicitly allowed per case
- warnings that do not match allowed substrings are counted as unexpected failures

## Run locally

```bash
python3 scripts/diagnostic_benchmark.py --manifest validation/diagnostics/manifest.json
python3 scripts/diagnostic_benchmark.py --manifest validation/diagnostics/manifest.json --output target/diagnostic-benchmark.json
python3 scripts/diagnostic_benchmark.py --manifest validation/diagnostics/manifest.json --min-top1 0.75 --min-top2 0.90
```

## Add a new case

1. Add or reference a committed `analysis_report` artifact.
2. Add a manifest entry with `ground_truth` and `acceptable_top2`.
3. Add `must_include_evidence` and `allowed_warnings` expectations.
4. Add a short `notes` rationale for the label.
5. Re-run benchmark and tests.

This validation framework measures diagnostic behavior quality; it is not causal proof.
