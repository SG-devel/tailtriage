# tailtriage-cli

`tailtriage-cli` is the analyzer/report tool for tailtriage artifacts.

Use it after capture to generate triage output with evidence-ranked suspects and next checks.

## What this crate is for

This crate owns the **analysis contract**:

- load a captured artifact
- validate schema compatibility
- emit human-readable or JSON triage output

## When to use this crate vs others

- **Use `tailtriage-cli`:** analyze existing artifacts.
- **Use `tailtriage` / `tailtriage-core` / related crates:** capture instrumentation data.

## Installation

```bash
cargo install tailtriage-cli
```

## Minimal usage

```bash
tailtriage analyze tailtriage-run.json --format json
```

## What to inspect first in output

1. `primary_suspect.kind`
2. `primary_suspect.evidence[]`
3. `primary_suspect.next_checks[]`

Suspects are investigation leads, not proof of root cause.

## Artifact compatibility contract

- Requires top-level `schema_version`.
- Current supported schema version: `1`.
- Loader warnings include lifecycle warnings and unfinished request notices when present.

## Deeper docs

- User guide workflow: [`../docs/user-guide.md`](../docs/user-guide.md)
- Diagnostics references: [`../docs/diagnostics.md`](../docs/diagnostics.md)
- Capture-side default crate docs: [`../tailtriage/README.md`](../tailtriage/README.md)
