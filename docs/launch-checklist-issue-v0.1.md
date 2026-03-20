# Launch checklist issue: crates metadata and dry-run publish status (v0.1)

Date: 2026-03-20

## Scope

- Added public-facing crates.io metadata to publishable crates:
  - `tailtriage-core`
  - `tailtriage-macros`
  - `tailtriage-tokio`
  - `tailtriage-cli`
- Ran `cargo publish --dry-run` for each publishable crate.

## Metadata checklist

- [x] `description` added and aligned with Tokio tail-latency triage positioning.
- [x] `repository` added (`https://github.com/tailtriage/tailtriage`).
- [x] `documentation` added (docs.rs URL per crate).
- [x] `readme` added (`../README.md`).
- [x] `keywords` added for crates.io discovery.
- [x] `categories` added for crates.io discovery.

## Dry-run outcomes

### 1) `tailtriage-core`

Command:

```bash
cargo publish -p tailtriage-core --dry-run --allow-dirty
```

Outcome: ✅ Success (packaging + verification completed; upload aborted due to dry-run).

### 2) `tailtriage-macros`

Command:

```bash
cargo publish -p tailtriage-macros --dry-run --allow-dirty
```

Outcome: ⚠️ Blocked in pre-publish sequence because `tailtriage-core` is not yet available on crates.io in this local dry-run context.

Error excerpt:

```text
no matching package named `tailtriage-core` found
location searched: crates.io index
```

### 3) `tailtriage-tokio`

Command:

```bash
cargo publish -p tailtriage-tokio --dry-run --allow-dirty
```

Outcome: ⚠️ Blocked in pre-publish sequence because `tailtriage-core` is not yet available on crates.io in this local dry-run context.

Error excerpt:

```text
no matching package named `tailtriage-core` found
location searched: crates.io index
```

### 4) `tailtriage-cli`

Command:

```bash
cargo publish -p tailtriage-cli --dry-run --allow-dirty
```

Outcome: ⚠️ Blocked in pre-publish sequence because `tailtriage-core` is not yet available on crates.io in this local dry-run context.

Error excerpt:

```text
no matching package named `tailtriage-core` found
location searched: crates.io index
```

## Next checks

1. Publish `tailtriage-core` first.
2. Re-run dry-runs for `tailtriage-macros`, `tailtriage-tokio`, and `tailtriage-cli` after `tailtriage-core` is available on crates.io.
3. Publish remaining crates in dependency order and confirm docs.rs pages resolve.
