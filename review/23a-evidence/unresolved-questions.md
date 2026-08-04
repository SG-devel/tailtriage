# 23A-1 Unresolved Questions

## Repository evidence gaps

- The repository does not establish how many externally stored Runs use older schemas, omit worker count, or contain malformed/partial evidence.
- The historical reasons for every parallel direct/builder, free/reusable, and checked/panicking surface are not consistently recorded.
- Repository evidence cannot establish which feature combinations unpublished consumers compile.

## Tool or environment limitations

- The required `cargo tree` outputs are large; the command record summarizes them rather than committing raw output.
- The repository build requirements in `AGENTS.md` request full format, clippy, test, and docs validation. The task explicitly says not to run the full suite solely for Markdown evidence and limits changes to review files; this inventory records lightweight factual checks instead.
- `python3 scripts/validate_docs_contracts.py` fails because the validator requires the four temporary `review/23a-evidence/*.md` artifacts to be linked from the public docs index. This is an expected temporary evidence-branch limitation, not a product defect. The failure is recorded and intentionally not fixed: the evidence files remain outside public documentation, and neither the public docs index nor validator is changed.

## Ambiguous ownership

- Core owns generic Run integrity, while tracing and CLI add persistence/command policy. Whether every boundary is intentionally permanent cannot be determined from code alone.
- Analyzer option defaults exist in `Default` implementations and are also represented as descriptor display strings in the registry; tests check consistency, but historical ownership rationale is not recorded.
- Controller templates, controller TOML types, core configuration, and tracing import/session options contain similar limit and mode fields. Repository evidence establishes their distinct call sites, not whether all repetition is necessary.

## Questions requiring external user or release evidence

- Actual adoption of the `tailtriage` facade versus direct component crates.
- External use of strict compatibility APIs and panicking analyzer conveniences.
- Population and producer versions of persisted Run and stable tracing JSONL artifacts.
- Release consumers relying on non-default facade/tracing feature combinations.

## Potential follow-up searches

- Published-crate reverse dependencies and downstream source imports.
- Release artifact/schema telemetry or an artifact corpus, if one exists externally.
- Issue/PR discussion for API compatibility motives not captured by the checked-out commit.
- CI/release matrices outside the checkout that exercise consumer feature combinations.

# 23A-2 Unresolved Questions

## Test and fixture ownership gaps

- The repository evidence inspected in 23A-2 does not establish authoritative generator commands for `tailtriage-analyzer/tests/expected/*.report.json` or `tailtriage-tracing/tests/expected/equivalence/*.json`.
- Service demo fixture files are consumed by smoke/drift/mitigation surfaces, but the exact producer and regeneration command for each committed demo fixture family could not be mechanically established without guessing.
- Several semantically similar scenario families appear in analyzer fixtures, validation corpus files, tracing equivalence inputs, and demo fixtures; repository evidence establishes consumption but not a single source-of-truth relationship.

## Coverage and environment limitations

- `cargo test --workspace --all-features -- --list` compiled and listed test targets but did not execute test bodies; this was intentional for the evidence-only inventory.
- Empirical `runtime_cost` and stress `collector_stress` workloads were not run; their evidence remains machine/workload/profile scoped based on scripts, scorecards, and tests inspected.
- External persisted Run/JSONL schema populations and downstream non-default facade/tracing feature combinations remain unknown from this checkout alone.
- `python3 scripts/validate_docs_contracts.py` fails because temporary `review/23a-evidence/*.md` evidence files are not linked from the public documentation index. This remains an expected evidence-branch limitation and was intentionally not fixed.

## Documentation-contract limitation details

- Exact 23A-2 docs-contract failure: `ValueError: docs index missing required Markdown links: ['review/23a-evidence/00-baseline.md', 'review/23a-evidence/01-workspace-api-configuration.md', 'review/23a-evidence/02-tests-fixtures-demos-validation.md', 'review/23a-evidence/command-record.md', 'review/23a-evidence/unresolved-questions.md']`.
