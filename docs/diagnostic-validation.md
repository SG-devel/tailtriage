# Diagnostic validation

`tailtriage` validation checks diagnostic behavior, not root-cause proof.

## Methodology
- Deterministic analyzer report corpus with labeled `ground_truth`.
- Benchmark verifies top-1, top-2 required causes, evidence presence, and warning expectations.
- Metrics include high-confidence wrong count and confidence-bucket accuracy.

## Top-1 vs Top-2
- Top-1 accuracy tracks dominant-label correctness.
- `required_top2` is the list of diagnosis kinds that must appear in primary or first secondary suspect.
- Top-2 recall is based on required causes appearing, not on acceptable alternate primaries.

## Acceptable primary
- `acceptable_primary` is the list of primary suspects accepted for ambiguity/high-confidence-wrong interpretation.
- It does not replace `required_top2` requirements.

## High-confidence-wrong count
Tracks cases where primary confidence is high/very-high and primary suspect is not in `acceptable_primary`.

## Next-check validation status
Next-check substring validation is schema-supported, but the current initial corpus has no required next-check cases.
