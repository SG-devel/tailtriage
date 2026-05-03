# Diagnostic validation

This page describes the diagnostic validation methodology for `tailtriage`.

Deterministic corpus validation uses committed fixtures and a manifest-driven benchmark. Top-1 checks primary suspect correctness; top-2 checks whether an acceptable bottleneck family appears in the top two suspects. We also track high-confidence-wrong count, confidence-bucket accuracy, required evidence pass rate, and warning validation.

Confidence is score-derived ranking strength, not causal certainty. The score is an evidence-ranking score, not probability.

Insufficient-evidence paths are explicitly validated with labeled cases. Warning validation ensures expected warning substrings are allowed while unexpected warnings fail the benchmark.

Future work: repeated-run validation, perturbation validation, integrated overhead validation, and integrated collector-limit validation.
