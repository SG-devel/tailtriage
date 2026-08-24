# Validation

## Diagnostic corpus accounting

The schema-version-2 deterministic corpus separates analyzer-executed cases, unique accuracy-eligible observations, and report-contract cases. Raw Runs are analyzed at benchmark execution; stable tracing JSONL is imported and then analyzed. Analyzer cases have typed success/failure contracts and may be execution-only. Multiple artifact encodings of one logical workload execute independently but count once for accuracy and must agree on ordered diagnosis visibility and confidence bucket.

Pre-generated and synthetic Reports validate Report fields, warnings, evidence, next checks, confidence, routes, and temporal output without executing the analyzer. They never contribute to analyzer accuracy. Fixture ground truth is controlled fixture intent, not production truth or root-cause proof.

## Validation responsibilities

| Layer | Purpose | Mechanism | Execution |
| --- | --- | --- | --- |
| Analyzer rule correctness | Explicit evidence selects the intended diagnosis | Typed Rust tests | Normal CI |
| Artifact pipeline regression | Representative committed artifacts pass through real intake and analyzer paths | Diagnostic corpus and integrity lock | Normal CI |
| Live workload behavior | Real demos produce expected signals and preserve integration behavior | Bounded demo smoke/parity checks plus repeated-run matrix and canonical live-demo mitigation reporting | CI smoke/parity; local/manual repeated runs |

The diagnostic manifest and benchmark own corpus classification and accounting. The analyzer fixture lock and integrity checker protect committed analyzer artifacts. Typed analyzer tests own diagnosis-rule coverage.

Real workloads are validated in two ways: bounded demo smoke and parity checks may run in CI, while repeated-run matrix and canonical live-demo mitigation reporting remain local/manual and machine-scoped. Generated Runs, Reports, summaries, and matrix outputs remain under `target/` and are not committed.

## Summary
`tailtriage` is a triage tool, not root-cause proof. It produces evidence-ranked suspects and next checks, where suspects are leads and not causal certainty.

For production rollout and operational guidance that applies these bounded claims, see `docs/operations.md`.

## Current validation status
This repository includes an initial deterministic validation corpus for controlled Tokio workload fixtures. The corpus and benchmark validate bounded diagnostic behavior on committed fixtures, not universal production behavior.

## Validation map
This document is the repository validation map and trust boundary. `docs/diagnostic-validation.md` explains diagnostic validation behavior for users. `validation/diagnostics/README.md` defines the corpus/manifest contract for maintainers. `validation/diagnostics/latest/scorecard.md` is a stable note about committed scorecard status, not a live metrics file. `scripts/validate_all.py` is an orchestration convenience over existing tracks, not the source of truth.

| File/script/workflow | Role | Normal CI? | Publishes durable artifacts? |
|---|---|---:|---:|
| `scripts/diagnostic_benchmark.py` | Deterministic diagnostics corpus gate for committed manifest/fixtures | Yes | No |
| `scripts/validate_docs_contracts.py` | Public-doc and validation-doc truth contract | Yes | No |
| `scripts/generate_diagnostic_scorecard.py` | Local/manual deterministic scorecard generation with provenance | No, local/manual | Local outputs only |
| `scripts/run_diagnostic_matrix.py` | Repeated controlled demo runs | No, local/manual | No |
| `scripts/demo_tool.py mitigation-report` | Baseline vs mitigated evidence-movement checks | No, local/manual | No |
| `scripts/measure_runtime_cost.py` | Runtime-cost operational validation | Manual/local; bounded smoke runs in CI | No |
| `scripts/measure_collector_limits.py` | Collector-limit operational validation | Manual/local; bounded smoke runs in CI | No |
| `scripts/validate_all.py` | Optional orchestration wrapper over existing validation tracks | No single source of truth; local/manual wrapper | Local outputs only |

Normal CI owns the deterministic diagnostic regression gate and does not publish scorecards or GitHub artifacts. `scripts/generate_diagnostic_scorecard.py` owns local/manual deterministic scorecard generation; its outputs are local evidence unless a maintainer separately archives them.

### Normal CI ownership and cadence

Normal mandatory CI runs on relevant pull requests. The Cargo matrix owns cross-platform dev and
release Cargo proof, including release documentation and dependency-policy checks. Independent
Linux jobs run after changed-path detection: `validation / deterministic` owns the deterministic
corpus, fixture, and public-example checks; `validation / live` owns release demo validation and
live tracing parity; and `validation / operational` owns bounded runtime-cost and collector-limit
smoke checks. These proof classes run in parallel when code changes are applicable.

`CI required` aggregates the applicable mandatory job results and performs no additional proof.
Docs-only pull requests run docs contracts without unnecessarily running code jobs. A manual
`workflow_dispatch` runs the full workflow, while a normal merge to `main` does not automatically
repeat the same full CI matrix. There is no dedicated GitHub Actions diagnostic snapshot workflow:
the local scorecard generator supplies the same deterministic summary and provenance without a
redundant remote-execution, workflow-input, and publication surface that adds no independent proof.

### Remote Action pin review

Remote GitHub Actions must use full 40-character commit SHAs, but 40 hexadecimal characters alone
are not sufficient review. When adding or updating a pin, maintainers must verify its upstream
provenance and expected retention, preferring a release commit or a commit retained in the
maintained default-branch history. Do not pin generated commits that belong only to moving
convenience refs when upstream may replace or garbage-collect them. For
`dtolnay/rust-toolchain`, pin a retained `master` commit and pass the desired toolchain, such as
`stable`, explicitly through `with:`. For `taiki-e/install-action`, pin a retained normal release
commit and pass the exact tool and version through `with:` instead of pinning a generated tool
alias such as `@cargo-deny`. Green hosted CI proves only that a commit is reachable and functional
now; it does not establish long-term upstream retention. The credential-free offline workflow
checker enforces mechanically verifiable syntax and configuration, not provenance or retention.

## Evidence levels

| Level | Runs in CI? | What it supports | What it does not prove |
|---|---|---|---|
| Unit/helper tests | Yes | script/helper correctness checks for validation tooling | end-to-end diagnostic behavior by itself |
| Deterministic corpus | Yes in normal CI | bounded analyzer/report behavior on committed fixtures | production root cause certainty or universal accuracy |
| Repeated-run matrix | No (manual/local) | stability metrics across repeated controlled runs on one machine/workload profile | universal stability across production environments |
| Live-demo mitigation | No (manual/local) | baseline vs mitigated movement checks for next-check usefulness | formal causal proof |
| Runtime-cost measurement | Yes (bounded hard-gated smoke in CI) + manual/local deeper runs | overhead measurement under documented synthetic workloads | universal production overhead guarantees |
| Collector-limit stress | Yes (bounded smoke profile) | bounded collector-pressure characterization with retained/truncation/drop evidence and measured onset/resource behavior | zero drops under all load; analyzer warning or diagnosis downgrade behavior |
| Real-service validation | No (planned) | future curated real-service truth checks when artifacts exist | current real-service validation coverage |

Tracing-intake contract coverage is currently validated by unit/package tests and examples that exercise:
- stable wrapper fixture (`tailtriage.tracing-span.v1`)
- stable wrapper-only import behavior
- CLI stable import guardrails
- `TracingSession` request/stage/queue capture and conversion
- example compile/run coverage under package example tests

These checks validate conversion correctness and user-facing guardrails for triage intake. They do not validate production root-cause truth, do not claim import support for ordinary tracing log JSON (`fmt().json` style output), do not claim OTel/OTLP support, and do not claim runtime-pressure diagnosis from tracing-only intake without runtime snapshots/Tokio sampler coupling.

## Deterministic corpus validation
The deterministic benchmark validates:
- evidence-ranked suspect correctness against corpus labels
- analyzer-path behavior on selected committed raw run-artifact fixtures (Run -> analyze_run())
- completed tailtriage tracing JSONL import through `tailtriage import tracing-spans-jsonl`, followed by `tailtriage analyze` on the imported Run JSON
- required visible suspects (`required_visible_suspects`) appear as the primary or first secondary suspect
- warning expectations (`expected_warnings` required; unexpected warnings rejected unless explicitly allowed)
- required evidence substrings
- required next-check substrings when required by a case
- case-level confidence ceilings (`max_primary_confidence`) for sparse/missing/truncated/mixed evidence humility checks

Normal CI enforces this deterministic benchmark directly against `validation/diagnostics/manifest.json` and referenced fixtures. This is a correctness gate for committed corpus/schema drift, not a durable scorecard publication path.

The corpus includes deterministic adversarial validation that checks sparse, missing, truncated, or mixed evidence is warned about and does not produce overconfident unsupported classifications.

## Repeated-run matrix validation (manual/local)
`scripts/run_diagnostic_matrix.py` provides repeated-run validation for controlled demo scenarios (queue, blocking, executor, downstream; optional mixed).

It writes raw JSONL run records plus summary JSON (and optional Markdown scorecard) for stability metrics including top-1 accuracy, top-2 recall, high-confidence-wrong count, per-scenario primary stability, confidence bucket accuracy, and p95/p99 latency distribution summaries.

This repeated-run validation is manual/local (not mandatory CI). Publishable repeated-run outputs are generated locally and are not committed by default. Results are machine/workload scoped.

## Live-demo mitigation validation (manual/local)
`scripts/demo_tool.py mitigation-report` runs paired baseline/mitigated controlled demo scenarios and compares latency plus evidence movement for targeted mitigations.

It writes JSONL pair records, summary JSON, and optional scorecard Markdown under `target/` paths. Generated outputs are local/manual and are not committed by default.

Mitigation validation checks whether expected evidence-ranked suspect movement appears under controlled workloads (for example: queue-share drops, service-share drops, blocking queue-depth drops, and explainable top-2/primary movement), while treating score movement as intra-report ranking signal rather than absolute cross-report severity.

This workflow is machine/workload scoped and supports triage next checks. Mitigation movement is not formal causal proof.

## Runtime-cost / operational validation
Operational validation has dedicated domain folders under `validation/runtime-cost/` and `validation/collector-limits/`.

`scripts/measure_runtime_cost.py` and `scripts/measure_collector_limits.py` directly own manual/local runtime-cost and collector-limit validation. They emit raw JSONL records and summary JSON in their selected artifact directories.

Runtime-cost results are machine/workload/profile scoped and are not universal production guarantees.

## Collector-limit validation
Collector-limit validation characterizes bounded collector pressure through retained counts, truncation and dropped-category counters, and measured onset/resource behavior. Results are machine/workload/profile scoped. Analyzer warning and diagnosis-downgrade behavior for partial or truncated evidence is owned separately by analyzer tests and deterministic corpus validation.

It does not claim no drops.

## Real-service validation (future)
Real-service validation is planned for curated anonymized real-service artifacts.

## Unified validation runner
Use `scripts/validate_all.py` to orchestrate existing validation tracks through explicit profiles (`smoke`, `ci`, `full`, `publish`).

This page owns the profile meanings and invocation policy:

| Profile | Intended audience and scope |
|---|---|
| `smoke` | Local fast pass over one bounded scenario per live validation track, deterministic diagnostics, docs contracts, and Cargo completion checks. |
| `ci` | Contributor/CI-shaped deterministic and script-test coverage without the full repeated-run matrix and canonical live-demo mitigation reporting. |
| `full` | Manual/local repeated-run, mitigation, runtime-cost, and one complete default collector-limit matrix repeat. Diagnostic and runtime-cost tracks default to five repetitions. Outputs remain machine/workload/profile scoped. |
| `publish` | The same substantive validation depth as `full`, using a release-artifact directory. It is credential-free and check-only: it does **not** publish crates; the manual procedure is owned solely by [RELEASING.md](RELEASING.md). |

Cargo formatting, Clippy, and workspace tests run by default in every profile. Use `--skip-cargo`
only when those checks were run separately and the intended invocation is limited to non-Cargo
tracks. The unified runner orchestrates existing scripts; it does not replace focused domain
runners, change analyzer behavior, publish crates, or create tags or releases.

Examples:

```bash
# Fast local orchestration
python3 scripts/validate_all.py --profile smoke

# Check-only release-readiness orchestration; no publication occurs
python3 scripts/validate_all.py --profile publish --profile-mode release
```

Run focused package tests and domain runners while iterating; use the completion commands in
[`AGENTS.md`](../../AGENTS.md) before committing. Fixture check/refresh ownership and required
ordering live in [FIXTURE_LINEAGE.md](FIXTURE_LINEAGE.md), rather than in a second command matrix
here. CI-only command composition remains owned by [`.github/workflows/ci.yml`](../../.github/workflows/ci.yml).

## Validation non-claims
Validation does not claim:
- root-cause proof from one run
- universal production overhead
- replacement for tracing, metrics, tokio-console, or tokio-metrics
- real-service validation unless curated real-service artifacts exist
- mitigation movement as formal causal proof

Demos teach scenarios; validation measures bounded diagnostic behavior.


## Manual diagnostic snapshots
For a provenance-rich deterministic snapshot, run `scripts/generate_diagnostic_scorecard.py` locally. Generated `benchmark-summary.json`, `environment.json`, and `scorecard.md` files are local evidence, not automatically published or durable artifacts, and normal CI does not auto-overwrite `validation/diagnostics/latest/scorecard.md`. Broader credential-free, check-only release-readiness validation is local/manual through `python3 scripts/validate_all.py --profile publish --profile-mode release`; it publishes neither crates nor GitHub artifacts.

Snapshot outputs include deterministic benchmark metrics, thresholds, git/ref metadata, `tailtriage` workspace/package version metadata, GitHub Actions metadata when available, software/hardware metadata, and manifest/referenced-artifact hashes.


Optional manifest fields can validate expanded analyzer report surface on selected cases only: `expected_evidence_quality`, `expected_signal_statuses`, `must_include_confidence_notes`, `expected_route_breakdowns`, `expected_temporal_segments`, `must_include_route_warning`, `must_include_temporal_warning`, and `expected_top_level_warnings`. These checks are fixture-scoped and optional; cases that omit them continue to validate under the existing suspect/evidence/warning contract.

## Tracing contract parity CI gates
CI gates native/tracing contract parity for demo scenarios via `scripts/demo_tool.py validate-tracing-parity all --profile release`. Checks include scenario coverage, expected evidence presence, route coverage, capture-mode behavior across `light` and `investigation`, retention semantics, and runtime/non-runtime boundaries. These checks do not require byte-for-byte artifact equality or exact suspect-ranking equality in every scenario. The queue tiny-limit retention parity gate runs in both `light` and `investigation` and checks retained counts, dropped counters, `truncation.limits_hit`, and `metadata.effective_core_config`. Runtime-sensitive tracing scenarios (Tokio session coupling) use deterministic/manual runtime snapshots and require all of: non-empty runtime snapshots, scenario-specific runtime field evidence, and an explicit disabled-background-sampler lifecycle warning. Runtime-sensitive tracing contract parity does not rely on ambient sampler metadata/noise. Tracing-only scenarios must not fabricate runtime snapshots. These checks support repeatable triage evidence and do not prove universal production performance or root-cause certainty.


TracingSession validation paths may run deterministic/manual runtime snapshot mode (`manual_runtime_snapshots()` + `record_runtime_snapshot(...)`) to remove sampler cadence noise. This supports repeatable triage evidence and does not claim causal proof.


## Partial queue/stage evidence

Completed queue and stage distributions exclude partial observations. Partial durations are observed lower bounds: tailtriage observed the helper from first poll until Drop, not proof that the underlying operation completed, failed, or stopped. Partial evidence remains visible in event totals, evidence-quality limitations, top-level warnings, and suspect evidence.

Queue/service public p95 fields remain completed-only. A queue or downstream-stage suspect materially relying on an observed-lower-bound path cannot exceed medium confidence; partial evidence that does not affect selected eligibility or score does not automatically cap a completed candidate. Partial stage `success = false` is not interpreted as a completed operation failure.

Global, route, and temporal projections share this policy. Tracing imports remain completed-only. Completed-only Report JSON and text remain unchanged; mixed or partial Runs may change scores or ranking only when explicitly labeled lower-bound evidence is selected and qualified. Suspects remain triage leads, not root-cause proof.

### Native/tracing equivalence tracks

Deterministic exact package-level equivalence uses committed native Run and stable completed-span JSONL fixtures in normal Cargo tests. It compares exact representable completed evidence, retention order/counters, and exact comparable analyzer output against independent expected artifacts. This proves the intake equivalence contract; it is not production diagnostic-accuracy evidence.

Live demo parity exercises real capture paths across multiple scenarios and modes with broader, machine-sensitive semantic checks. It is intentionally not byte-exact and does not require exact ranking equality in every mitigated run. It supports capture-to-re-run triage validation, not causal proof. Run JSON remains the complete artifact because completed-span JSONL excludes Run-only runtime, in-flight, lifecycle, and complete truncation state.
