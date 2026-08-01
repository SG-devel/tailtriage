# Prompt 20 executor-normalization evidence (draft)

This directory is an **evidence-only draft PR** export. It must not be merged and may be closed after review. No production or behavior-affecting file changed.

The tracked Rust micro-crate uses repository path dependencies and public APIs only. Its canonical `public-api.json` output and the derived Python harness make the review reproducible from a clean checkout. Generated scratch output belongs under `target/validation/executor-normalization/`.

```sh
mkdir -p target/validation/executor-normalization
cargo run --quiet --locked --manifest-path validation/executor-normalization-review/Cargo.toml > target/validation/executor-normalization/public-api.generated.json
cmp validation/executor-normalization-review/public-api.json target/validation/executor-normalization/public-api.generated.json
python3 validation/executor-normalization-review/harness.py --public-api validation/executor-normalization-review/public-api.json --output-dir target/validation/executor-normalization/run1 --verification target/validation/executor-normalization/verification.json
python3 validation/executor-normalization-review/harness.py --public-api validation/executor-normalization-review/public-api.json --output-dir target/validation/executor-normalization/run2 --verification target/validation/executor-normalization/verification.json
cmp target/validation/executor-normalization/run1/report.json target/validation/executor-normalization/run2/report.json
cmp target/validation/executor-normalization/run1/report.md target/validation/executor-normalization/run2/report.md
```

The verification JSON is deterministic orchestration input: it records successful generator comparison, two-run comparisons, hashes, and clean-tree exit statuses before final tracked reports are rendered. Review findings are derived from structured comparisons; a failed mandatory check is recorded in `failed_cases`, exits nonzero, and changes the recommendation to `revise` (or `reject` only for a contradictory/unusable formula).

The public docs-contract validator intentionally rejects this unindexed evidence directory. Public documentation and indexes are outside this evidence-only scope and must not be changed merely to satisfy that validator.

Suspects and projections are triage leads, not proof of root cause. Projected normalized behavior is review evidence, not current production analyzer behavior.
