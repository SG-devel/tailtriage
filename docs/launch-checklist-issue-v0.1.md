# Launch checklist issue: crates metadata and dry-run publish status (v0.1)

Date: 2026-03-21

## Scope

This checklist resolves v0.1 crates.io publication readiness for the MVP crates.

It includes:

- explicit crate publication scope (publish now vs workspace-only)
- finalized first-release package versions (`0.1.0`)
- metadata validation for all publishable crates
- local `cargo publish --dry-run` checks in dependency order
- docs.rs surface and owner/access setup notes

## Publication scope decision

### Publish now (v0.1.0)

These crates are part of the first public release surface:

1. `tailtriage-core`
3. `tailtriage-tokio`
4. `tailtriage-cli` (binary crate shipping `tailtriage`)

### Workspace-only (publish later / do not publish)

These are intentionally not part of the crates.io MVP surface right now:

- all demo crates under `demos/*` (`publish = false`)
- `demos/demo_support` (`publish = false`)
- `demos/runtime_cost` (`publish = false`)

Rationale: demos are proof workflows, not stable crate APIs.

## Metadata checklist (publish-now crates)

- [x] `description` present and aligned with Tokio tail-latency triage positioning.
- [x] `license` present (`MIT`).
- [x] `repository` present (`https://github.com/tailtriage/tailtriage`).
- [x] `documentation` present (docs.rs URL per crate).
- [x] `readme` configured (`../README.md`).
- [x] `keywords` present for crates.io discovery.
- [x] `categories` present for crates.io discovery.

## Installation-path alignment

README installation examples use publish-now crate names:

- `tailtriage-core = "0.1"`
- `tailtriage-tokio = "0.1"`

CLI analysis command uses the `tailtriage` binary from `tailtriage-cli`.

## docs.rs landing-page check

Expected docs.rs pages for first publish:

- <https://docs.rs/tailtriage-core>
- <https://docs.rs/tailtriage-tokio>
- <https://docs.rs/tailtriage-cli>

All publish-now crates set `readme = "../README.md"` so docs.rs presents a consistent entry surface.

## Dry-run outcomes (dependency order)

### 1) `tailtriage-core`

Command:

```bash
cargo publish -p tailtriage-core --dry-run --allow-dirty
```

Outcome: ✅ Success (packaging + verification completed; upload aborted due to dry-run).


Command:

```bash
```

Outcome: ✅ Success (packaging + verification completed; upload aborted due to dry-run).

### 3) `tailtriage-tokio`

Command:

```bash
cargo publish -p tailtriage-tokio --dry-run --allow-dirty
```

Outcome: ⚠️ Expected dependency-order block before first real publish because `tailtriage-core` is not yet available on crates.io.

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

Outcome: ⚠️ Expected dependency-order block before first real publish because `tailtriage-core` is not yet available on crates.io.

Error excerpt:

```text
no matching package named `tailtriage-core` found
location searched: crates.io index
```

## Owner/access setup

Before first publish, configure crate owners for each publish-now crate:

```bash
cargo owner --add github:tailtriage:owners tailtriage-core
cargo owner --add github:tailtriage:owners tailtriage-tokio
cargo owner --add github:tailtriage:owners tailtriage-cli
```

If a GitHub team owner is unavailable, add at least two individual maintainers and record the intended team owner migration.

## Publish sequence and immediate next checks

1. Publish `tailtriage-core`.
2. Re-run dry-runs for `tailtriage-tokio` and `tailtriage-cli`.

4. Verify docs.rs builds for all four crates and confirm README rendering on each page.
5. Re-check README install commands from a clean environment.
