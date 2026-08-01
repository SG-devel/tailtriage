# Executor-normalization review evidence

This directory is an evidence-only draft review package. It changes no production or behavior-affecting file. The evidence PR must not be merged; it may be closed after review.

## One-command reproduction

From a clean checkout at the evidence commit, run:

```bash
python3 validation/executor-normalization-review/harness.py --orchestrate
```

The command builds the locked Rust public-API generator, checks its canonical JSON, derives both reports twice, verifies byte identity and the allowed Git state, writes target-only verification evidence, and refreshes the tracked reports. Scratch output is under `target/validation/executor-normalization/`.

Confirm the tracked/generated artifacts explicitly:

```bash
cmp validation/executor-normalization-review/public-api.json target/validation/executor-normalization/public-api.generated.json
cmp validation/executor-normalization-review/report.json target/validation/executor-normalization/report-run1.json
cmp validation/executor-normalization-review/report.json target/validation/executor-normalization/report-run2.json
cmp validation/executor-normalization-review/report.md target/validation/executor-normalization/report-run1.md
cmp validation/executor-normalization-review/report.md target/validation/executor-normalization/report-run2.md
```

Canonical JSON files use sorted, deterministic single-line encoding with one trailing newline.

The repository docs-contract validator is expected to reject this intentionally unindexed evidence directory. Do not add public documentation links to bypass that limitation.

GitHub draft state and PR metadata must be managed manually by the user. No `gh` command or GitHub API operation is part of this workflow. This package does not create, update, or merge a PR.

The projected normalized executor candidate is review evidence only. Evidence-ranked suspects and next checks remain triage leads, not proof of root cause.
