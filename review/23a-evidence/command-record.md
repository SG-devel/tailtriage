# 23A-1 Command Record

## Repository baseline

Command: `git status --short` (before inspection)
Purpose: enforce the clean-tree gate.
Result: success; no output.
Important output: clean working tree.
Limitations: none.

Command: `git rev-parse HEAD` (before inspection)
Purpose: enforce the base-SHA gate.
Result: success.
Important output: `66a31701ff5f0455595977c683981bb54df3165d`.
Limitations: none.

Command: `git branch --show-current`; `git log -1 --oneline --decorate`; `git remote -v`
Purpose: record branch, commit decoration, and remotes.
Result: success.
Important output: branch `work`; `66a3170 (HEAD -> work) Simplify scoped analyzer ratio comparisons (#1128)`; no remotes.
Limitations: no remote can be used for push/PR operations.

Command: `git rev-parse origin/main`
Purpose: compare the remote main ref.
Result: failed because `origin/main` is unknown.
Important output: `fatal: ambiguous argument 'origin/main'`.
Limitations: remote-main comparison unavailable.

Command: `rustc --version`; `cargo --version`; `rustup show active-toolchain`
Purpose: record toolchain.
Result: success.
Important output: rustc `1.95.0`; cargo `1.95.0`; toolchain `1.95.0-x86_64-unknown-linux-gnu`.
Limitations: none.

## Workspace and manifests

Command: `cargo metadata --format-version 1 --locked` plus a Python JSON summary
Purpose: enumerate workspace members, versions, publish settings, targets, dependencies, and features without committing raw metadata.
Result: success; 20 workspace packages (8 product crates, `demo-support`, and 11 demo binaries).
Important output: all packages version `0.3.0`; demos have `publish = false`; product crates have no explicit publish restriction; only facade and tracing declare features.
Limitations: raw metadata was temporary at `/tmp/tt-metadata.json`, not committed.

Command: `cargo tree --workspace --edges normal,build,dev`
Purpose: inspect normal/build/dev dependency direction.
Result: success.
Important output: core is the common product dependency; analyzer, Axum, Tokio depend on core; controller depends on core+Tokio; CLI depends on analyzer+core+tracing; facade has optional component dependencies.
Limitations: output was 1,154 lines together with the feature tree and was summarized.

Command: `cargo tree --workspace --edges features`
Purpose: inspect activated Cargo features.
Result: success.
Important output: facade defaults activate controller+Tokio; tracing progresses `jsonl` -> `live` -> `tokio`; facade maps its feature tiers to these.
Limitations: large transitive external feature output was not copied.

Command: `find . -name Cargo.toml -not -path './target/*' -print | sort`
Purpose: enumerate manifests.
Result: success; 21 manifests including workspace root.
Important output: root, eight product members, demo support, and eleven demo manifests.
Limitations: none.

## Surface and ownership discovery

Command: the four requested targeted `rg --glob '*.rs'` public/API searches
Purpose: discover public declarations, functions, re-exports/features, builders/lifecycle/validation.
Result: success.
Important output: respectively 574, 338, 127, and 1,750 matching lines before surrounding-module verification.
Limitations: counts include tests and are discovery aids, not API counts.

Command: `rg -n 'AnalyzeOptions|OptionDescriptor|...|deny_unknown_fields'`
Purpose: discover configuration ownership.
Result: success; 2,204 matches, then verified in key modules.
Important output: analyzer registry/TOML/overrides, core builder/config, controller TOML/template, tracing options/session, and CLI flags.
Limitations: broad count includes docs/tests.

Command: targeted `sed` and `rg` reads of every product manifest and key source modules
Purpose: verify important symbols and call paths in context.
Result: success.
Important output: ownership and convergence summarized in the main evidence inventory.
Limitations: no generated rustdoc was needed.

Command: Python regex count over `tailtriage-analyzer/src/options/registry.rs`
Purpose: count registered analyzer option paths.
Result: success.
Important output: 30 distinct paths across eight option domains.
Limitations: mechanically checked against registry literals at this commit.

## Validation

Command: `git diff --check`
Purpose: validate evidence patch whitespace.
Result: success after final edits.
Important output: no output.
Limitations: Markdown-only validation.

Command: `git status --short`; `git diff --name-only`
Purpose: verify only the four allowed files changed.
Result: success before commit.
Important output: exactly the four allowed evidence files were staged; status showed only those four additions.
Limitations: none.
