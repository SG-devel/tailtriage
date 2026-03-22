# tailtriage-cli

Command-line diagnosis tool for one `tailtriage` run artifact.

`tailtriage-cli` reads a JSON artifact and produces a report with evidence-ranked suspects and next checks.

## Install

This repository is pre-publish.

- **After first crates.io publish:** install with `cargo install tailtriage-cli`.
- **Before publish (current state):** run from source in this repository.

## Analyze one artifact

From this repository today (pre-publish):

```bash
cargo run -p tailtriage-cli -- analyze tailtriage-run.json
```

Post-publish path:

```bash
tailtriage analyze tailtriage-run.json
```

## Output shape to inspect first

Start with:

1. Top-ranked suspects and their evidence,
2. `next_checks` to decide what to instrument or capture next,
3. confidence/coverage caveats (suspects are leads, not proof).

For machine processing, use JSON output:

```bash
tailtriage analyze tailtriage-run.json --format json
```

## Related docs

- Data capture API (`tailtriage-core`): <https://docs.rs/tailtriage-core>
- Tokio runtime sampling (`tailtriage-tokio`): <https://docs.rs/tailtriage-tokio>
- Repository docs and demos: <https://github.com/SG-devel/tailtriage>


## Artifact schema contract

`tailtriage-cli` requires a top-level `schema_version` field in every run artifact. Current supported value: `1`. Missing, non-integer, or unsupported values fail fast with a clear error so triage runs against a known schema contract.
