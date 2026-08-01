# Prompt 20 executor normalization characterization

Verified commit: `1a083075a477d658cb6a581cb6c07c4e87935e42`

Recommendation: **approve unchanged**.

The exact candidate formula resisted the specified falsification checks. Projected normalized results below are projections, not current `analyze_run` behavior.

## Source truth
- `tailtriage-analyzer/src/scoring.rs:238-281` — legacy executor trigger, score, +4 growth, clean-extreme and 94 cap: global p95 >= 1; formula verified; clean extreme global>=140 and samples>=30.
- `tailtriage-analyzer/src/scoring.rs:5-8,601-611` — sample quality: <8:0, 8..19:1, 20..39:3, 40..99:5, >=100:8.
- `tailtriage-analyzer/src/lib.rs:1168-1188` — percentile: sorted index ceil((n-1)*numerator/denominator), bounded.
- `tailtriage-analyzer/src/options/mod.rs:133-155` — confidence thresholds and ambiguity defaults: 65 medium, 85 high, ambiguity min 60 gap 4.
- `tailtriage-analyzer/src/confidence.rs:31-106,179-210` — caps and ambiguity cluster: conservative min; raw-score cluster.
- `tailtriage-analyzer/src/lib.rs:810-854` — confidence-first ordering: final confidence, score, stable kind.
- `tailtriage-core/src/validation.rs:20-68,142-184, tailtriage-core/src/tests.rs:2110-2160` — canonical invalid zero normalization: typed invalid_worker_count; strict error; retained snapshot with worker cleared.

## Criteria

| # | Result | Reason |
|---:|:---:|---|
| 1 | pass | source constants match |
| 2 | pass | all direct boundaries match |
| 3 | pass | exact scale cases agree |
| 4 | pass | quantization explicitly reported |
| 5 | pass | depth monotonic |
| 6 | pass | worker scaling nonincreasing |
| 7 | pass | combined snapshot p95 rule verified |
| 8 | pass | 75-bit maximum intermediate fits u128 |
| 9 | pass | typed public legacy cases match expected path |
| 10 | pass | ambiguous modes capped medium |
| 11 | pass | relevant missing local capped medium |
| 12 | pass | caps compose conservatively |
| 13 | pass | first five controls have no false High executor primary |
| 14 | pass | complete extreme score is High |
| 15 | pass | two-run byte equality is checked externally |
| 16 | pass | Phase 1 clean tree is checked externally |

## Boundary summary

| milli | candidate | contribution |
|---:|:---:|---:|
| 0 | false | — |
| 499 | false | — |
| 500 | true | 5 |
| 999 | true | 5 |
| 1000 | true | 15 |
| 1999 | true | 15 |
| 2000 | true | 25 |
| 3999 | true | 25 |
| 4000 | true | 40 |
| 7999 | true | 40 |
| 8000 | true | 55 |

## Findings

- Integer targets are never fabricated. The 0.5-task target is not representable for one worker; nearest depths are 0 (0 milli) and 1 (1000 milli).
- All exactly representable equal-per-worker cases have identical milli values, contributions, and held-constant scores.
- Fixed backlog milli values and contributions are nonincreasing with worker count; contributions are monotonic for depths 0 through 128.
- Combined-per-snapshot p95 avoids adding independent non-coincident global and local peaks; exact sorted series and indices are in `report.json`.
- Fully absent worker evidence uses the exact public legacy analyzer path. Typed cases include below-trigger, ordinary, soft-cap, clean-extreme, absent optional values, all sample bands, growth, and truncation.
- Typed zero checks prove strict rejection, retained permissive snapshots, worker-only clearing, exact original index/field, and `InvalidWorkerCount` provenance.
- Worker ambiguity and relevant missing-local lower bounds cap executor confidence at Medium and compose by conservative bucket minimum without erasing existing notes.
- All six competing controls retain current public analyzer outputs and separately labeled projections; the first five do not create a false High executor primary, while the complete extreme reaches High.
- Maximum exact intermediate is 75 bits, so `u128` addition and multiplication cover the full source domain without wrapping, floating point, saturation, or another policy.

## Commands
- `cargo run --quiet --offline --manifest-path target/validation/executor-normalization/Cargo.toml > target/validation/executor-normalization/public-api.json`
- `cargo run --quiet --locked --manifest-path target/validation/executor-normalization/Cargo.toml > target/validation/executor-normalization/public-api-locked.json`
- `cmp target/validation/executor-normalization/public-api.json target/validation/executor-normalization/public-api-locked.json`
- `python3 target/validation/executor-normalization/harness.py`
- `cargo test --locked -p tailtriage-core invalid_worker_count_is_rejected_strictly_and_cleared_permissively`
- `cargo test --locked -p tailtriage-analyzer`

## Questionable cases
- Future implementation must carry typed invalid-worker provenance across permissive normalization; normalized worker_count=None alone is insufficient.

No criterion failed. Detailed inputs, intermediates, output candidates, caps, tables, and projections are retained in `report.json`.
