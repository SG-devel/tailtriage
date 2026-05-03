# Diagnostic validation

`tailtriage` validation checks diagnostic behavior, not root-cause proof.

## Methodology
- Deterministic analyzer report corpus with labeled `ground_truth`.
- Benchmark verifies top-1, top-2 required causes, evidence presence, and warning expectations.
- Metrics include high-confidence wrong count and confidence-bucket accuracy.

## Deterministic vs repeated-run validation
Current foundation is deterministic-case validation. Repeated-run validation is planned for variance and perturbation checks.

## Top-1 vs Top-2
- Top-1 accuracy tracks dominant-label correctness.
- Top-2 recall tracks whether acceptable alternate suspects remain visible for mixed scenarios.

## High-confidence-wrong count
Tracks cases where primary confidence is high/very-high and the primary suspect is not in `acceptable_primary`. This protects against overconfident misranking outside accepted alternatives.

## Confidence calibration
Confidence is score-derived ranking strength. It is not causal certainty.

## Insufficient-evidence validation
Corpus includes low-signal cases to ensure the analyzer can emit `insufficient_evidence` and avoid false certainty.

## Warning validation
Cases require `expected_warnings` to appear and allow optional warnings only when they match `allowed_warnings`.

## Synthetic corpus fixture type
Synthetic gap-covering cases use `artifact_type: "synthetic_analysis_report"` to keep typing truthful: they are hand-readable report-shaped fixtures for validation gaps, not real demo-emitted `analysis_report` artifacts.

## Future work
- repeated runs
- perturbation validation
- expanded overhead publication
- expanded collector-limit publication
