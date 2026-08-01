# Prompt 20 evidence export

Verified starting SHA: `1a083075a477d658cb6a581cb6c07c4e87935e42`.

Original Prompt 20 recommendation: **approve unchanged**.

| Criterion | Result |
| ---: | :---: |
| 1 | Pass |
| 2 | Pass |
| 3 | Pass |
| 4 | Pass |
| 5 | Pass |
| 6 | Pass |
| 7 | Pass |
| 8 | Pass |
| 9 | Pass |
| 10 | Pass |
| 11 | Pass |
| 12 | Pass |
| 13 | Pass |
| 14 | Pass |
| 15 | Pass |
| 16 | Pass |

Phase 1 commands:

```text
cargo run --quiet --offline --manifest-path target/validation/executor-normalization/Cargo.toml > target/validation/executor-normalization/public-api.json
cargo run --quiet --locked --manifest-path target/validation/executor-normalization/Cargo.toml > target/validation/executor-normalization/public-api-locked.json
cmp target/validation/executor-normalization/public-api.json target/validation/executor-normalization/public-api-locked.json
python3 target/validation/executor-normalization/harness.py
cargo test --locked -p tailtriage-core invalid_worker_count_is_rejected_strictly_and_cleared_permissively
cargo test --locked -p tailtriage-analyzer
```

Two-run determinism: `report.json` and `report.md` were byte-identical under `cmp` after two harness executions. Hashes: harness `dfde36b44b6e01ea3d26a38cb631153b06d57ea406a4550be61691da5e567f25`; report.json `56b6b69d2e9149b18804532392aabc44f202838a79da16af0a320f9387abdc1a`; report.md `ce22b4e29a2adb7959a5590644881e711490c6a025eb3b825494e5c10dab921a`.

Source/copy comparisons: `harness.py`: `cmp` passed; `report.json`: `cmp` passed; `report.md`: `cmp` passed.

This export preserves the durable evidence used to falsify the candidate executor worker-normalization formula.

The analysis completed before this branch was created.

The tracked artifacts are byte-identical copies of the generated ignored artifacts.

No production or behavior-affecting repository file changed.

This draft PR is evidence-only, must not be merged, and may be closed after review.
