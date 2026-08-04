# 23A-1 Unresolved Questions

## Repository evidence gaps

- The repository does not establish how many externally stored Runs use older schemas, omit worker count, or contain malformed/partial evidence.
- The historical reasons for every parallel direct/builder, free/reusable, and checked/panicking surface are not consistently recorded.
- Repository evidence cannot establish which feature combinations unpublished consumers compile.

## Tool or environment limitations

- `origin/main` cannot be resolved and `git remote -v` is empty. Remote comparison, pushing, and remote PR creation therefore depend on infrastructure absent from this checkout.
- The required `cargo tree` outputs are large; the command record summarizes them rather than committing raw output.
- The repository build requirements in `AGENTS.md` request full format, clippy, test, and docs validation. The task explicitly says not to run the full suite solely for Markdown evidence and limits changes to review files; this inventory records lightweight factual checks instead.

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
