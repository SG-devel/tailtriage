# Diagnostic validation

`tailtriage` is a triage tool, not a root-cause proof engine.

This page describes the initial deterministic diagnostic validation harness.

- Validation is machine-scoped and workload-scoped.
- Suspects are evidence-ranked leads, not proof.
- Score is an evidence-ranking score, not probability.
- Confidence is score-derived, not causal certainty.

## Methodology

The benchmark reads a labeled manifest of analyzer report artifacts and checks:
- top-1 classification correctness
- top-2 recall against allowed ambiguity sets
- required evidence substrings across primary and secondary suspects
- warning expectations and unexpected warnings

## Deterministic cases vs repeated runs

Current validation is deterministic fixture validation.
Repeated-run and perturbation validation are planned future layers.

## Metrics

- top-1 accuracy
- top-2 recall
- high-confidence-wrong count
- confusion matrix
- confidence-bucket accuracy
- required-evidence pass rate
- unexpected-warning count
- failed cases

## Insufficient evidence and warning validation

The corpus includes explicit insufficient-evidence and truncation/warning cases so that weak or partial-data runs do not overclaim certainty.

## Future work

- repeated-run variance envelopes
- perturbation sensitivity checks
- runtime-overhead validation integration
- collector-limit validation integration
