# tailtriage-cli

Command-line triage analyzer for one `tailtriage` run artifact.

## Use from the repo

```bash
cargo run -p tailtriage-cli -- analyze tailtriage-run.json --format json
```

## Install from crates.io

```bash
cargo install tailtriage-cli
tailtriage analyze tailtriage-run.json --format json
```

## Output shape to inspect first

1. `primary_suspect.kind`
2. `primary_suspect.evidence[]`
3. `primary_suspect.next_checks[]`

Suspects are evidence-ranked leads, not proof of root cause.

## Artifact schema contract

`tailtriage-cli` requires a top-level `schema_version` field. Current supported value: `1`.

When artifacts contain unfinished lifecycle metadata, the loader surfaces warnings (unfinished count/sample) but does not fabricate missing completion events.

## Related docs

- Repo docs index: <https://github.com/SG-devel/tailtriage/tree/main/docs>
- Core crate: <https://github.com/SG-devel/tailtriage/tree/main/tailtriage-core>
- Tokio integration crate: <https://github.com/SG-devel/tailtriage/tree/main/tailtriage-tokio>
