# tailtriage-cli

`tailtriage-cli` is the **analysis and report generation** crate for `tailtriage` artifacts.

Use it after capture to produce evidence-ranked suspects and next checks.

## When to use this crate vs others

- `tailtriage-core` / `tailtriage-*` crates capture runtime data.
- `tailtriage-cli` loads captured artifacts and produces triage reports.

## Installation

```bash
cargo install tailtriage-cli
```

## Minimal usage

```bash
tailtriage analyze tailtriage-run.json --format json
```

## Output fields to inspect first

1. `primary_suspect.kind`
2. `primary_suspect.evidence[]`
3. `primary_suspect.next_checks[]`

Suspects are investigation leads, not proof of root cause.

## Artifact contract

- Requires top-level `schema_version`.
- Current supported schema version: `1`.
- Loader warnings include lifecycle warnings and unfinished request notices when present.
