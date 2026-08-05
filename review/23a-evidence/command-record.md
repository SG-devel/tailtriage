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

Command: `git status --short`; `git rev-parse HEAD` (before the correction pass)
Purpose: enforce the correction-pass clean-tree and exact-SHA gates.
Result: success; status produced no output.
Important output: `2e5662d34e67a6c674f82a9aa299d2b3b0494eb5`.
Limitations: none.

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

Command: `cargo fmt --check`
Purpose: verify repository formatting after the Markdown corrections.
Result: success.
Important output: no output.
Limitations: none.

Command: `python3 scripts/validate_docs_contracts.py`
Purpose: run the public documentation contract validator.
Result: expected failure for this temporary evidence branch.
Important output: `ValueError: docs index missing required Markdown links: ['review/23a-evidence/00-baseline.md', 'review/23a-evidence/01-workspace-api-configuration.md', 'review/23a-evidence/command-record.md', 'review/23a-evidence/unresolved-questions.md']`.
Limitations: the validator requires every repository Markdown file to be linked from the public docs index. These temporary evidence artifacts intentionally are not public documentation, so this evidence branch records but does not fix the failure.

Command: `git diff --check`
Purpose: validate evidence patch whitespace.
Result: success after final edits.
Important output: no output.
Limitations: Markdown-only validation.

Command: `git status --short`; `git diff --name-only`
Purpose: verify only the four allowed files changed.
Result: success before the correction commit.
Important output: exactly the four allowed evidence files were modified.
Limitations: none.

# 23A-2 Command Record

## Starting verification

Command: `git status --short`; `git rev-parse HEAD`
Purpose: enforce the clean-tree and exact-SHA gates for 23A-2.
Result: success; status produced no output.
Important output: `d7c8b0f756ac8f8dde5b7ce3b243510e6520e179`.
Limitations: `AGENTS.md` was read via `cat AGENTS.md >/tmp/agents_read`; the user-requested verification commands themselves produced the recorded output.

## Discovery

Command: `cargo metadata --format-version 1 --locked > /tmp/23a2-cargo-metadata.json`
Purpose: enumerate workspace packages, targets, examples, demos, and test targets.
Result: success.
Important output: 20 workspace packages: 8 product crates, `demo-support`, and 11 demo/workload binaries.
Limitations: raw metadata was stored in `/tmp` and summarized rather than committed.

Command: `cargo test --workspace --all-features -- --list > /tmp/23a2-test-list-rerun.txt 2>&1`
Purpose: list Rust test targets and test names without running test bodies.
Result: success with exit status 0 after compiling test targets.
Important output: listed tests across core, controller, Tokio, analyzer, tracing, CLI, facade, Axum, demo-support, collector_stress, and runtime_cost; service demo binaries reported `0 tests, 0 benchmarks`.
Limitations: compilation took about 2m48s according to Cargo output; the command lists tests and compiles targets but does not execute test bodies; rerun output was stored in `/tmp/23a2-test-list-rerun.txt`.

Command: `find . -path './target' -prune -o -path './.git' -prune -o -type f \( -path '*/tests/*' -o -path '*/examples/*' -o -name '*.json' -o -name '*.jsonl' -o -name '*.toml' -o -name '*.snap' -o -name '*golden*' -o -name '*expected*' \) -print > /tmp/23a2-files.txt`
Purpose: locate tests, examples, JSON/JSONL/TOML, snapshots, golden, and expected-output files.
Result: success.
Important output: 158 matching files; no `.snap` files appeared in the targeted output.
Limitations: file existence is discovery evidence only and not proof of consumption by itself.

Command: `rg -n "#\\[(tokio::)?test|mod tests|include_str!|include_bytes!|fixture|golden|snapshot|validation|runtime_cost|collector_stress|demo|workload|parity|bench" -g '!target' -g '!review/23a-evidence/02-tests-fixtures-demos-validation.md' > /tmp/23a2-rg.txt || true`
Purpose: locate inline tests, async tests, included fixtures, validation references, workload references, parity and benchmark terminology.
Result: success with matches.
Important output: 4,987 lines of discovery matches.
Limitations: broad search includes docs/tests and was used as a locator, not as a count of behavior.

Command: `find .github workflows . -path './target' -prune -o -path './.git' -prune -o -type f -path '*/.github/workflows/*' -print > /tmp/23a2-workflows.txt 2>/dev/null || true`
Purpose: locate workflow files for invocation-reference inspection only.
Result: success.
Important output: `.github/workflows/ci.yml` and `.github/workflows/validation-snapshot.yml`.
Limitations: this phase did not analyze CI duration, critical path, cadence, or redesign concerns.

Command: Python directory size summary over fixture, validation, demo, example, and expected-output directories, written to `/tmp/23a2-sizes.txt`.
Purpose: estimate file counts and approximate total sizes for fixture families.
Result: success.
Important output: analyzer fixtures 9 files/~20.9 KiB; analyzer expected reports 9 files/~32.9 KiB; tracing equivalence fixtures 5 files/~82.4 KiB; tracing equivalence expected 8 files/~48.8 KiB; validation diagnostics corpus 22 files/~82.8 KiB; service demo fixtures about 30 JSON files/~108 KiB by directory summary.
Limitations: sizes are approximate filesystem byte totals and not semantic fixture counts.

Command: Python workspace-member summary from `/tmp/23a2-cargo-metadata.json`; targeted `rg` over `/tmp/23a2-test-list-rerun.txt`; targeted Rust test annotation search.
Purpose: group workspace packages, targets, test modules, and async/inline tests for the proof map.
Result: success.
Important output: metadata listed all product, demo, example, and integration-test targets; test annotation search found inline tests in core, controller, Tokio, Axum, analyzer, tracing, CLI, demo-support, runtime_cost, collector_stress, and integration tests.
Limitations: grouping intentionally avoids listing every analyzer/tracing test function individually.

Command: `find scripts -maxdepth 2 -type f | sort`; `find validation -maxdepth 3 -type f | sort`; `find demos -maxdepth 2 -type f ... | sort`; `find .github/workflows -type f -maxdepth 1 -print | sort`
Purpose: inventory validation scripts, validation artifacts, demo packages, and workflow references.
Result: success.
Important output: validation scripts include diagnostic matrix, mitigation matrix, runtime-cost, collector-limit, docs contract, fixture integrity, and smoke scripts; validation directories include diagnostics/runtime-cost/collector-limits; workflows are `ci.yml` and `validation-snapshot.yml`.
Limitations: workflow inspection was limited to establishing invocation presence.

## Documentation-contract validation

Command: `python3 scripts/validate_docs_contracts.py`
Purpose: run the requested documentation contract validator after creating the temporary evidence file.
Result: expected failure.
Important output: `ValueError: docs index missing required Markdown links: ['review/23a-evidence/00-baseline.md', 'review/23a-evidence/01-workspace-api-configuration.md', 'review/23a-evidence/02-tests-fixtures-demos-validation.md', 'review/23a-evidence/command-record.md', 'review/23a-evidence/unresolved-questions.md']`.
Limitations: temporary `review/23a-evidence/*.md` files are intentionally not linked from the public documentation index; this was recorded and not fixed.


## 23A-2 correction pass

Command: `git status --short`; `git rev-parse HEAD`
Purpose: enforce the correction-pass clean-tree and exact-SHA gates.
Result: success; status produced no output.
Important output: `bd562915a220e8267edfa44bbf4b37ed9c390915`.
Limitations: none.

Command: `cargo test --workspace --all-features -- --list > /tmp/23a2-test-list-rerun.txt 2>&1`
Purpose: rerun the workspace test listing without masking its exit status.
Result: success with exit status 0.
Important output: completed test-target compilation/listing and ended with doc-test listings; output stored at `/tmp/23a2-test-list-rerun.txt`.
Limitations: listed tests and compiled targets; did not execute test bodies.

Command: `cargo test -p tailtriage-core --all-features --lib -- --list > /tmp/23a2-tailtriage-core-lib-list.txt 2>&1`
Purpose: verify the product-crate inline test surface for `tailtriage-core`.
Result: success with exit status 0.
Important output: 141 tests, 0 benchmarks; included `tests::queue_stage_and_inflight_are_recorded`, request ID, schema/finalization, retention, shutdown, validation, normalization, and sink/failure tests.
Limitations: listed tests only; did not execute test bodies.

Command: `cargo test -p tailtriage-controller --all-features --lib -- --list > /tmp/23a2-tailtriage-controller-lib-list.txt 2>&1`
Purpose: verify the product-crate inline test surface for `tailtriage-controller`.
Result: success with exit status 0.
Important output: 57 tests, 0 benchmarks; included enable/disable/reload lifecycle, finalization schema, sampler lifecycle, generation binding, TOML, and failure tests.
Limitations: listed tests only; did not execute test bodies.

Command: `cargo test -p tailtriage-tokio --all-features --lib -- --list > /tmp/23a2-tailtriage-tokio-lib-list.txt 2>&1`
Purpose: verify the product-crate inline test surface for `tailtriage-tokio`.
Result: success with exit status 0.
Important output: 32 tests, 0 benchmarks; included `tests`, `helper_tests`, and partial-event Tokio helper tests for runtime sampling and helper behavior.
Limitations: listed tests only; did not execute test bodies.

Command: `cargo test -p tailtriage-axum --all-features --lib -- --list > /tmp/23a2-tailtriage-axum-lib-list.txt 2>&1`
Purpose: verify the product-crate inline test surface for `tailtriage-axum`.
Result: success with exit status 0.
Important output: 2 tests, 0 benchmarks; included crate identity and default HTTP status mapping tests.
Limitations: listed tests only; did not execute test bodies.

Command: `find scripts/tests -maxdepth 1 -name 'test_*.py' -printf '%f\n' | sort`; `sed -n '90,310p' .github/workflows/ci.yml`; targeted `rg` for `_scenario_specs`, `tailtriage-run.json`, and CLI analysis prints.
Purpose: verify repository-owned Python unittest modules, CI dotted-module invocations, demo fixture drift ownership, and Tokio example artifact behavior.
Result: success.
Important output: found 13 `scripts/tests/test_*.py` unittest modules; CI invokes dotted modules with `python3 -m unittest`; `scripts/check_demo_fixture_drift.py::_scenario_specs()` enumerates committed demo fixture paths; both Tokio examples write `tailtriage-run.json` and print `cargo run -p tailtriage-cli -- analyze tailtriage-run.json --format json`.
Limitations: discovery only; Python tests and examples were not run.

Command: targeted `rg` sweep over the three allowed evidence files for unsupported speculative phrases, stale inline-test claims, non-unittest ownership wording, and invalid collector/runtime-cost library commands.
Purpose: verify speculative wording and invalid-command sweep.
Result: success after edits; no output.
Important output: no remaining matches.
Limitations: literal-pattern sweep only.

## Final correction-pass validation

Command: `cargo fmt --check`
Purpose: verify Rust formatting before committing.
Result: success with exit status 0.
Important output: no output before recorded `FMT_STATUS:0`.
Limitations: Rust formatting check only; Markdown formatting is not checked by rustfmt.

Command: `git diff --check`
Purpose: verify patch whitespace before committing.
Result: success with exit status 0.
Important output: no output before recorded `DIFF_CHECK_STATUS:0`.
Limitations: whitespace check only.

Command: `git diff --name-only`
Purpose: enforce the allowed-file scope before committing.
Result: success.
Important output:
```text
review/23a-evidence/02-tests-fixtures-demos-validation.md
review/23a-evidence/command-record.md
review/23a-evidence/unresolved-questions.md
```
Limitations: reports tracked diff paths only.

Command: `git status --short`
Purpose: record working-tree state before committing.
Result: success.
Important output:
```text
 M review/23a-evidence/02-tests-fixtures-demos-validation.md
 M review/23a-evidence/command-record.md
 M review/23a-evidence/unresolved-questions.md
```
Limitations: none.

## Final correction-pass validation rerun after command-record update

Command: `cargo fmt --check`
Purpose: verify Rust formatting immediately before committing.
Result: success with exit status 0.
Important output: no output before recorded `FMT_STATUS:0`.
Limitations: Rust formatting check only; Markdown formatting is not checked by rustfmt.

Command: `git diff --check`
Purpose: verify patch whitespace immediately before committing.
Result: success with exit status 0.
Important output: no output before recorded `DIFF_CHECK_STATUS:0`.
Limitations: whitespace check only.

Command: `git diff --name-only`
Purpose: enforce the allowed-file scope immediately before committing.
Result: success.
Important output:
```text
review/23a-evidence/02-tests-fixtures-demos-validation.md
review/23a-evidence/command-record.md
review/23a-evidence/unresolved-questions.md
```
Limitations: reports tracked diff paths only.

Command: `git status --short`
Purpose: record working-tree state immediately before committing.
Result: success.
Important output:
```text
 M review/23a-evidence/02-tests-fixtures-demos-validation.md
 M review/23a-evidence/command-record.md
 M review/23a-evidence/unresolved-questions.md
```
Limitations: none.


## Final 23A-2 evidence correction

Command: `git status --short`; `git rev-parse HEAD`
Purpose: enforce the final correction clean-tree and exact-SHA gates.
Result: success; status produced no output.
Important output: `ddc4b1fb6a9e0554680394c115d288237e5e2067`.
Limitations: none.

Command: targeted edits to restore the original 23A-2 baseline SHA and narrow diagnostic integrity-lock ownership.
Purpose: correct evidence wording without changing code, fixtures, scripts, workflows, manifests, demos, examples, generated artifacts, or public docs.
Result: success.
Important output: the original 23A-2 baseline is `d7c8b0f756ac8f8dde5b7ce3b243510e6520e179`; `bd562915a220e8267edfa44bbf4b37ed9c390915` remains only as the separately labeled correction-pass verification SHA. Diagnostic fixture integrity ownership is limited to manifest cases with `validation_class = "analyzer_execution"`, their raw `run_artifact` and `tracing_span_jsonl` files, and `validation/diagnostics/analyzer-fixtures.lock.json`; `--refresh` rewrites only that lock and does not protect report-contract cases or `tailtriage-analyzer/tests/expected/*.report.json`.
Limitations: evidence correction only; no new conclusions were added.

## Final 23A-2 evidence correction validation

Command: `cargo fmt --check`
Purpose: verify Rust formatting before committing the final evidence correction.
Result: success with exit status 0.
Important output: no output before recorded `FMT_STATUS:0`.
Limitations: Rust formatting check only; Markdown formatting is not checked by rustfmt.

Command: `git diff --check`
Purpose: verify patch whitespace before committing the final evidence correction.
Result: success with exit status 0.
Important output: no output before recorded `DIFF_CHECK_STATUS:0`.
Limitations: whitespace check only.

Command: `git diff --name-only`
Purpose: enforce the three-file scope before committing the final evidence correction.
Result: success with exit status 0.
Important output:
```text
review/23a-evidence/02-tests-fixtures-demos-validation.md
review/23a-evidence/command-record.md
review/23a-evidence/unresolved-questions.md
```
Limitations: reports tracked diff paths only.

Command: `git status --short`
Purpose: record working-tree state before committing the final evidence correction.
Result: success with exit status 0.
Important output:
```text
 M review/23a-evidence/02-tests-fixtures-demos-validation.md
 M review/23a-evidence/command-record.md
 M review/23a-evidence/unresolved-questions.md
```
Limitations: none.

# 23A-3 Command Record

## Starting verification
Command: `git status --short`
Purpose: verify the working tree was clean before evidence work.
Result: success.
Exit status: 0.
Important output: no output.
Limitations: command was run before edits only, per task instruction.

Command: `git rev-parse HEAD`
Purpose: verify required starting commit.
Result: success.
Exit status: 0.
Important output: `2947676608961904c3230e69be5583643e6bdd4f`.
Limitations: none.

## Discovery
Command: `find .. -name AGENTS.md -print`
Purpose: locate repository instruction files.
Result: success.
Exit status: 0.
Important output: `../tailtriage/AGENTS.md` only.
Limitations: none.

Command: `cargo metadata --format-version 1 --locked > /tmp/23a3-cargo-metadata.json`
Purpose: capture workspace/package metadata without mutating repository files.
Result: success.
Exit status: 0.
Important output: metadata written to `/tmp/23a3-cargo-metadata.json`.
Limitations: raw metadata was not committed.

Command: `find . -path './target' -prune -o -path './.git' -prune -o -type f \( -name '*.md' -o -name '*.py' -o -name '*.sh' -o -name 'Makefile' -o -name 'Justfile' -o -name 'Taskfile.yml' -o -path '*/.cargo/config.toml' \) -print | sort`
Purpose: inventory Markdown, Python, shell, and task-runner surfaces while excluding target and git internals.
Result: success.
Exit status: 0.
Important output: 78 files were listed and stored in `/tmp/23a3-files.txt` for inspection.
Limitations: workflow YAML files were inspected separately because they were not part of the requested find expression.

Command: `python3` one-off Markdown summary extractor under `/tmp/23a3-md-summary.txt`
Purpose: extract headings and embedded command-looking lines from repository Markdown inventory.
Result: success.
Exit status: 0.
Important output: summarized titles/headings/commands for Markdown docs, package READMEs, validation docs, and review files.
Limitations: extraction was heuristic and supported, not replaced, manual inspection.

Command: `python3` one-off script summary extractor under `/tmp/23a3-py-summary.txt`
Purpose: extract argparse, subprocess, function, and command-construction cues from Python scripts.
Result: success.
Exit status: 0.
Important output: summarized script entry points, options, helper functions, subprocess use, and mutation flags.
Limitations: extraction was heuristic and supported manual inspection.

Command: `find .github/workflows -type f -maxdepth 1 -print -exec sed -n '1,220p' {} \; > /tmp/23a3-workflows.txt`
Purpose: inspect workflow triggers/jobs/steps for cadence and command ownership.
Result: success.
Exit status: 0.
Important output: workflow command blocks captured for `ci.yml` and `validation-snapshot.yml`.
Limitations: hosted CI was not run.

Command: `rg -n "This structural change|u128|u64|saturat|ratio|parity|option paths|extreme|false-positive" docs README.md VALIDATION.md CHANGELOG.md tailtriage-*/*.md review -g '*.md'`
Purpose: search prose for CF-DOC-001 and ratio/saturation/parity terminology.
Result: success.
Exit status: 0.
Important output: found the `docs/analyzer-rationale.md` parity sentence and many unrelated parity/saturation references; no other prose was found that states the exact saturated-`u64` to exact-`u128` scoped-ratio distinction.
Limitations: search scope was repository prose, not code comments outside selected source inspection.

Command: `sed -n '1,220p' tailtriage-analyzer/src/ratio.rs`
Purpose: inspect the scoped ratio helper implementation and tests for CF-DOC-001.
Result: success.
Exit status: 0.
Important output: `meets_ratio` uses exact `u128::from(...) * u128::from(...)` cross-products and tests large values near `u64::MAX`.
Limitations: no code was changed.

Command: `rg -n "validate_docs_contracts|validate_all|diagnostic_benchmark|run_diagnostic_matrix|run_mitigation_matrix|measure_runtime_cost|measure_collector|demo_tool|smoke_|cargo fmt|cargo clippy|cargo test|cargo doc|cargo package|cargo publish|CHANGELOG|tag|release" .github scripts docs README.md VALIDATION.md CONTRIBUTING.md AGENTS.md CHANGELOG.md -g '!target'`
Purpose: map contributor commands, workflow ownership, release-oriented surfaces, and script call graph evidence.
Result: success.
Exit status: 0.
Important output: found CI command blocks, validation script references, release snapshot trigger references, contributor command differences, and no `cargo package`/`cargo publish` implementation in the searched surfaces.
Limitations: output was large; evidence report records material findings rather than committing raw output.

## Focused validation
Command: `python3 -m unittest scripts.tests.test_validate_docs_contracts`
Purpose: run docs-contract unit tests requested for 23A-3.
Result: failure due to the same expected evidence-branch docs-index limitation.
Exit status: 1.
Important output: `Ran 96 tests`; `FAILED (errors=1)`; `test_docs_index_contract` raised `ValueError: docs index missing required Markdown links: ['review/23a-evidence/00-baseline.md', 'review/23a-evidence/01-workspace-api-configuration.md', 'review/23a-evidence/02-tests-fixtures-demos-validation.md', 'review/23a-evidence/03-docs-scripts-commands.md', 'review/23a-evidence/command-record.md', 'review/23a-evidence/unresolved-questions.md']`.
Limitations: run after temporary evidence Markdown existed; this tests validator behavior but is blocked by the intentional review-file index limitation.

Command: `cargo doc --workspace --all-features --no-deps`
Purpose: build workspace Rustdoc as requested for 23A-3.
Result: success.
Exit status: 0.
Important output: `Finished dev profile ...`; documentation generated under `target/doc`.
Limitations: task-requested command omitted `--locked`; CI uses `--locked`.

## Documentation-contract expected limitation
Command: `python3 scripts/validate_docs_contracts.py`
Purpose: run docs contract after creating temporary evidence file.
Result: expected failure.
Exit status: 1.
Important output: `docs/README.md must link to every tracked markdown file; missing links: review/23a-evidence/00-baseline.md, review/23a-evidence/01-workspace-api-configuration.md, review/23a-evidence/02-tests-fixtures-demos-validation.md, review/23a-evidence/03-docs-scripts-commands.md, review/23a-evidence/command-record.md, review/23a-evidence/unresolved-questions.md`.
Limitations: expected evidence-branch limitation; intentionally not fixed, not classified as a product defect.

## Final validation
Command: `cargo fmt --check`
Purpose: verify Rust formatting after evidence-only changes.
Result: success.
Exit status: 0.
Important output: no output.
Limitations: no Rust files were intended to change.

Command: `git diff --check`
Purpose: verify diff whitespace cleanliness.
Result: success.
Exit status: 0.
Important output: no output.
Limitations: none.

Command: `git diff --name-only`
Purpose: verify changed-file allowlist.
Result: success.
Exit status: 0.
Important output: after intent-to-add for the new evidence file, only `review/23a-evidence/03-docs-scripts-commands.md`, `review/23a-evidence/command-record.md`, and `review/23a-evidence/unresolved-questions.md`.
Limitations: run before commit.

Command: `git status --short`
Purpose: verify final pre-commit worktree path set.
Result: success.
Exit status: 0.
Important output: `M review/23a-evidence/command-record.md`, `M review/23a-evidence/unresolved-questions.md`, `?? review/23a-evidence/03-docs-scripts-commands.md`.
Limitations: run before commit.
