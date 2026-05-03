# Diagnostic validation

This page describes how `tailtriage` validates diagnosis quality.

## Methodology
Validation uses deterministic analysis-report fixtures and a machine-readable corpus manifest.

## Deterministic cases vs repeated-run validation
Current baseline is deterministic fixture validation. Repeated-run statistical validation is planned and intentionally not part of this first foundation.

## Top-1 vs Top-2
- **Top-1 accuracy**: primary suspect matches ground truth.
- **Top-2 recall**: either of first two suspects matches acceptable set.

## High-confidence-wrong count
Count cases where confidence is `high` but top-1 is wrong.

## Confidence calibration
Confidence is score-derived, not causal certainty. Score is an evidence-ranking score, not probability.

## Insufficient-evidence validation
Corpus includes insufficient-evidence cases to ensure weak-signal runs do not overclaim.

## Warning validation
Cases can whitelist expected warning substrings. Unexpected warnings fail validation.

## Future work
- repeated-run validation
- perturbation/sensitivity validation
- broader overhead publication
- broader collector-limit publication
