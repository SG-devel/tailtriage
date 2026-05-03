# Diagnostic validation corpus

Purpose: machine-readable labeled cases for deterministic analyzer validation.

## Manifest format
Each case defines:
- `required_top2`: diagnosis kinds that must appear in primary or first secondary suspect.
- `acceptable_primary`: primary suspects acceptable for ambiguity/high-confidence-wrong interpretation.
- `expected_warnings`: warning substrings that must appear.
- `allowed_warnings`: optional warning substrings that may appear without failure.

`required_top2` usually contains only `ground_truth`.
`acceptable_primary` may include alternates for mixed/ambiguous cases, but this does not satisfy top-2 by itself.

Next-check substring validation is schema-supported, but the current corpus has no required next-check cases unless new real requirements are added.
