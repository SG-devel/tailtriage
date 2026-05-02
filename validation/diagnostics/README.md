# Diagnostic validation corpus

This directory is the diagnostic validation surface for analyzer quality measurement.

## What this is

- `manifest.json` defines labeled validation cases.
- Cases reference committed analysis artifacts and check whether evidence-ranked suspects match expected diagnostic labels.
- This is different from demos: demos teach scenarios; validation measures diagnostic behavior.

## Manifest format

Each case includes:

- `id` unique case id
- `artifact` relative path to an analysis report
- `artifact_type` (`analysis_report` for now)
- `ground_truth` expected suspect kind
- `acceptable_top2` acceptable kinds in top-2 (must include `ground_truth`)
- `tags` searchable labels
- `must_include_evidence` evidence substrings that must appear across primary/secondary suspects
- `allowed_warnings` warning substrings explicitly tolerated for that case
- `notes` short label rationale

## Labeling rules

- Choose `ground_truth` from injected or independently known bottleneck family.
- Use top-1 for clean scenarios and top-2 tolerance for mixed/ambiguous scenarios.
- Do not use labels to claim causal proof.

## Evidence and warnings checks

- `must_include_evidence` is matched as substring against primary and secondary suspect evidence.
- `allowed_warnings` controls warning expectations:
  - empty list means no warnings are expected
  - warnings not matching allowed substrings are counted as unexpected

## Synthetic fixtures

Add synthetic fixtures under `validation/diagnostics/corpus/` only when existing demo artifacts do not cover an important diagnostic case (for example insufficient evidence or correlated mixed signals). Keep them small and hand-readable.
