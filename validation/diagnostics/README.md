# Diagnostic validation corpus

This directory is the diagnostic-quality validation layer for `tailtriage`.

Demos remain educational and smoke-test oriented. This corpus measures whether analyzer diagnosis output matches labeled ground-truth cases.

## Manifest format

`manifest.json` is an array of case objects with:

- `id` unique case id
- `artifact` path to committed analysis report JSON
- `artifact_type` currently `analysis_report`
- `ground_truth` suspect-kind label
- `acceptable_top2` accepted top-2 suspect kinds (must include `ground_truth`)
- `tags` scenario tags
- `must_include_evidence` required evidence substrings
- `allowed_warnings` warning substrings allowed for this case (`["*"]` allows any)
- `notes` short labeling rationale

## Case labeling rules

- Choose `ground_truth` from the known injected or independently known cause family.
- Use `acceptable_top2` for mixed/ambiguous cases where top-2 is still diagnostically useful.
- `must_include_evidence` should target concrete evidence text expected from primary or secondary suspects.
- Use `allowed_warnings` to allow expected warning text without allowing unrelated warnings.

## Synthetic fixtures

Add small hand-readable files under `corpus/` only when demos do not cover an important validation edge case (for example insufficient evidence, missing runtime snapshots, or correlated blocking-stage cases).

## How this differs from demos

- Demos: teach workflows and provide scenario smoke checks.
- Validation corpus: benchmark diagnostic quality with labeled expectations.
