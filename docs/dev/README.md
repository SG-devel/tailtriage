# Repository ownership and flow map

This page maps repository responsibilities to their current authoritative owners. It is a
navigation aid, not an API catalog or a second user guide.

`docs/dev/` contains contributor, maintainer, and repository-development documentation. The
canonical user and product documentation index remains [`docs/README.md`](../README.md).

`tailtriage` is focused Tokio tail-latency triage. It turns captured evidence into
**evidence-ranked suspects** and **next checks**; suspects are investigation leads, not proof of
root cause. It is not an observability backend, a distributed tracing system, or a general
telemetry platform. See the [product contract](../../SPEC.md) for the normative boundary.

## Package responsibilities

The `tailtriage` façade is the default adoption path. Use a narrower package directly only when
its smaller or more specific boundary is useful.

| Package | Primary responsibility | Typical reason to use it directly | Authoritative owner |
| --- | --- | --- | --- |
| `tailtriage` | Default façade over core capture plus feature-gated controller, Tokio, Axum, and tracing integrations | Start a new integration from the recommended, coherent entry point | [Package README and Rustdoc](../../tailtriage/README.md) |
| `tailtriage-core` | Framework-independent capture lifecycle, bounded evidence collection, standard `Run` model, integrity/normalization, and sinks | Use the smallest capture and evidence-model boundary or assemble a completed `Run` | [Package README and Rustdoc](../../tailtriage-core/README.md) |
| `tailtriage-controller` | Repeated bounded capture generations and their lifecycle/configuration | Arm, disarm, and later re-arm capture in a long-lived service | [Package README and Rustdoc](../../tailtriage-controller/README.md) |
| `tailtriage-tokio` | Tokio runtime-pressure sampling and helpers for Tokio primitives | Add runtime snapshots or instrument common Tokio waits and work boundaries | [Package README and Rustdoc](../../tailtriage-tokio/README.md) |
| `tailtriage-axum` | Axum request-boundary middleware and request-handle extraction | Integrate capture lifecycle at an Axum boundary while keeping inner instrumentation explicit | [Package README and Rustdoc](../../tailtriage-axum/README.md) |
| `tailtriage-tracing` | Narrow import/live-intake bridge from completed Tailtriage `tt.*` spans to standard `Run` evidence | Adopt from an existing supported Rust `tracing` instrumentation path | [Package README and Rustdoc](../../tailtriage-tracing/README.md) |
| `tailtriage-analyzer` | Batch diagnosis from typed `Run` values into typed `Report` values and canonical text/JSON rendering | Analyze and render completed evidence in process | [Package README and Rustdoc](../../tailtriage-analyzer/README.md) |
| `tailtriage-cli` | Saved-artifact loading, command-line import/analysis, validation policy selection, and output selection | Import supported tracing JSONL or analyze a saved `Run` from a shell | [Package README](../../tailtriage-cli/README.md) |

Demos and validation workloads are teaching and validation surfaces, not product packages; their
owners are mapped below.

## Supported adoption and data flows

| Path | Current flow | Detailed owner |
| --- | --- | --- |
| Native capture | `tailtriage::Tailtriage` (façade) or `tailtriage_core::Tailtriage` starts capture → `tailtriage-core` owns lifecycle and evidence → shutdown finalizes a standard `Run` through the configured sink | [Core capture](../../tailtriage-core/README.md) and [user guide](../user-guide.md) |
| Controller-managed capture | `tailtriage::controller` (façade) or `tailtriage-controller` → enable one bounded generation → capture → disable/finalize → enable a later generation | [Controller lifecycle and configuration](../../tailtriage-controller/README.md) |
| Offline tracing import | Stable `tailtriage.tracing-span.v1` completed-span JSONL → `tailtriage import tracing-spans-jsonl` → tracing parsing/import policy → normalized standard `Run` JSON | [CLI import contract](../../tailtriage-cli/README.md) and [tracing input contract](../../tailtriage-tracing/README.md) |
| Live tracing intake | `tailtriage::tracing` façade features or direct `tailtriage-tracing` → `TracingSession` layer records completed `tt.*` spans → core normalization → standard `Run` evidence (and configured outputs) | [Live tracing session](../../tailtriage-tracing/README.md) |
| Saved-artifact analysis | Saved `Run` JSON → `tailtriage analyze <run.json>` loading and strict-by-default validation → `tailtriage-analyzer` → text or canonical pretty `Report` JSON | [CLI artifact contract](../../tailtriage-cli/README.md) |
| In-process analysis | Typed `tailtriage_core::Run` → `tailtriage_analyzer::try_analyze_run` (or `analyze_run`) → typed `Report` → optional canonical renderers | [Analyzer package](../../tailtriage-analyzer/README.md) |

Offline import supports the documented Tailtriage completed-span wrapper, not arbitrary tracing
logs or `tracing_subscriber::fmt().json()` output. Native and tracing paths converge on the core
`Run` evidence model; they do not create separate analyzer contracts.

## Policy and configuration ownership

There is no repository-wide configuration precedence system. Each surface owns its defaults,
types, parsing, overrides, and validation where applicable.

| Concern | Current owner and boundary |
| --- | --- |
| Core `Run` integrity and normalization | `tailtriage-core` owns inspection, strict validation, and canonical permissive normalization; see its [artifact and lifecycle contract](../../tailtriage-core/README.md) and [`validation` module](../../tailtriage-core/src/validation.rs). |
| Strict saved-artifact acceptance | `tailtriage-cli` owns JSON envelope/file requirements and uses core strict validation by default; see the [artifact compatibility contract](../../tailtriage-cli/README.md). |
| Explicit permissive/ambiguous saved artifacts | The CLI owns `--allow-ambiguous-artifact`; core owns the normalization it invokes. Analyzer library entry points remain permissive by default and expose strict alternatives. See the [CLI policy](../../tailtriage-cli/README.md) and [analyzer report contract](../../tailtriage-analyzer/README.md). |
| Tracing input parsing/import policy | `tailtriage-tracing` owns `SpanRecord`, `ImportOptions`, wrapper parsing, semantic conversion, and strict/non-strict import behavior; the CLI owns import arguments and file handling. See both [tracing](../../tailtriage-tracing/README.md) and [CLI](../../tailtriage-cli/README.md) contracts. |
| Capture persistence/output policy | `tailtriage-core` owns `LocalJsonSink`, `MemorySink`, `DiscardSink`, capture limits, validation, and finalization. Controller and tracing sessions configure their own generation/session outputs. See their package READMEs. |
| Controller configuration | `tailtriage-controller` owns builder/TOML defaults, configuration types, parsing, reload validation, generation path derivation, and future-generation semantics. See its [configuration contract](../../tailtriage-controller/README.md). |
| Runtime sampling configuration | `tailtriage-tokio` owns sampler defaults, builder/config types, startup validation, cadence, and runtime snapshot behavior; controller config owns whether and how a sampler starts for an armed generation. See [Tokio sampling](../../tailtriage-tokio/README.md) and [controller configuration](../../tailtriage-controller/README.md). |
| Tracing configuration | `tailtriage-tracing` owns import/session/recorder defaults, builders, limits, output validation, and optional Tokio coupling; façade features only expose that crate. See its [package contract](../../tailtriage-tracing/README.md). |
| Analyzer options and interpretation | `tailtriage-analyzer` owns `AnalyzeOptions`, defaults, TOML schema/parsing, validation, diagnosis, typed `Report`, and renderers. The CLI layers config-file and `--analyzer-set` overrides over those options. See [analyzer configuration](../../tailtriage-analyzer/README.md), [behavior](../diagnostics.md), and [rationale](../analyzer-rationale.md). |
| CLI arguments, files, and output selection | `tailtriage-cli` owns command parsing, saved-file loading, CLI-only acceptance rules, analyzer overrides, and text/JSON selection; analyzer rendering remains delegated to `tailtriage-analyzer`. See the [CLI README](../../tailtriage-cli/README.md). |

## Documentation ownership

[`docs/README.md`](../README.md) is the canonical documentation index. Use these owners for detail:

| Need | Authoritative owner |
| --- | --- |
| First adoption and end-to-end user journey | [User guide](../user-guide.md) |
| Production operation and rollout | [Production operations guide](../operations.md) |
| Reading a report and choosing a next check | [Analyzer guide](../analyzer-guide.md) |
| Exact analyzer behavior | [Analyzer behavior reference](../diagnostics.md) |
| Analyzer rationale and proof ownership | [Analyzer rationale](../analyzer-rationale.md) |
| Normative product and data contracts | [SPEC.md](../../SPEC.md) |
| Validation evidence and non-claims | [VALIDATION.md](VALIDATION.md) and [diagnostic validation](../diagnostic-validation.md) |
| Architecture and design | [Architecture](../architecture.md) and [DESIGN_NOTES.md](DESIGN_NOTES.md) |
| Package-specific usage | The package READMEs in the [documentation index](../README.md#integrations-and-package-boundaries) |
| Versioned changes | [CHANGELOG.md](../../CHANGELOG.md) |
| Contribution guidance | [CONTRIBUTING.md](../../CONTRIBUTING.md) and [AGENTS.md](../../AGENTS.md) |
| Manual release procedure | [RELEASING.md](RELEASING.md) |

## Command and validation ownership

| Category | Entry point and ownership |
| --- | --- |
| Local iteration | Package examples and focused Cargo tests are owned by their package READMEs; demos and `scripts/demo_tool.py` are indexed by the [demo guide](../../demos/README.md). Use focused commands for the area being investigated rather than treating this page as a command catalog. |
| Required completion gate | [`AGENTS.md`](../../AGENTS.md) owns `cargo fmt --check`, workspace all-target/all-feature locked Clippy with warnings denied, the corresponding workspace tests, and `python3 scripts/validate_docs_contracts.py`. |
| Hosted CI | [`.github/workflows/ci.yml`](../../.github/workflows/ci.yml) owns hosted docs contracts, cross-platform Cargo checks, and its documented extended validation/smoke split. The workflow itself is the authority for what CI currently enforces. |
| Unified validation | [`scripts/validate_all.py`](../../scripts/validate_all.py) orchestrates named profiles; [VALIDATION.md](VALIDATION.md) classifies the tracks and their CI/manual/release status. Focused scripts remain the owners of their domains. |
| Focused validation | Diagnostic corpus, repeated-run, mitigation, runtime-cost, and collector-limit entry points are mapped in [VALIDATION.md](VALIDATION.md) and the [`validation/` domain documentation](../../validation/diagnostics/README.md). |
| Diagnostic snapshot | [`.github/workflows/validation-snapshot.yml`](../../.github/workflows/validation-snapshot.yml) is a manually dispatched diagnostic snapshot workflow. It is not scheduled and is not a release workflow; it has no tag or release behavior. |
| Package/release preflight | [`scripts/check_release.py`](../../scripts/check_release.py) is a local, check-only preflight. It may validate/package and print inert manual publication commands; it does not publish. |
| Manual release | [`RELEASING.md`](RELEASING.md) alone owns the step-by-step manual procedure. |

No current workflow establishes scheduled validation. Repository scripts and workflows do not
execute `cargo publish`; registry credentials are not repository or CI concerns. A maintainer
publishes manually, creates/pushes tags only after the intended package set is published, and
creates and publishes the GitHub Release manually.

## Fixture and generated-artifact ownership

This is intentionally a high-level owner map, not a provenance audit.

| Family | Broad contract and current owner | Detailed documentation |
| --- | --- | --- |
| Analyzer/golden and report-contract fixtures | Typed analyzer behavior is exercised by [`tailtriage-analyzer/tests/fixtures/`](../../tailtriage-analyzer/tests/fixtures); corpus report-contract artifacts are owned by the diagnostic manifest and benchmark. | [Analyzer README](../../tailtriage-analyzer/README.md) and [diagnostic corpus contract](../../validation/diagnostics/README.md) |
| Tracing-equivalence artifacts | Native `Run`, stable completed-span JSONL, and expected equivalence evidence are owned by `tailtriage-tracing` tests and [`tests/fixtures/equivalence/`](../../tailtriage-tracing/tests/fixtures/equivalence). | [Tracing README](../../tailtriage-tracing/README.md) and [validation equivalence tracks](../diagnostic-validation.md#native-tracing-equivalence-tracks) |
| Diagnostic validation corpus | Manifest-classified analyzer execution and report-contract cases live under [`validation/diagnostics/`](../../validation/diagnostics), with integrity and benchmark scripts under `scripts/`. | [Corpus contract](../../validation/diagnostics/README.md) and [VALIDATION.md](VALIDATION.md) |
| Demo fixtures/artifacts | Scenario-owned checked-in analysis fixtures live below each demo; `scripts/demo_tool.py` and drift checks own execution/verification. | [Demo guide](../../demos/README.md) and [getting-started demo](../getting-started-demo.md) |
| Runtime-cost outputs | Committed latest summaries live under [`validation/runtime-cost/latest/`](../../validation/runtime-cost/latest); machine/workload/profile-scoped generated operational output is normally under `target/`. | [Runtime-cost domain](../../validation/runtime-cost/README.md) and [user guidance](../runtime-cost.md) |
| Collector-limit outputs | Committed latest summaries live under [`validation/collector-limits/latest/`](../../validation/collector-limits/latest); generated validation checks visible bounded drops, warnings, and downgrade behavior rather than claiming no drops. | [Collector-limit domain](../../validation/collector-limits/README.md) and [user guidance](../collector-limits.md) |
| Generated diagnostic scorecards | The generator and manual snapshot workflow own generated scorecards; the current committed reference is [`validation/diagnostics/latest/scorecard.md`](../../validation/diagnostics/latest/scorecard.md). | [Diagnostic corpus snapshots](../../validation/diagnostics/README.md#versionedmanual-scorecard-generation) |

Detailed source, consumer, generator/refresh command, mutation policy, lineage, and
retirement/consolidation decisions remain Phase 25A work. This map makes no fixture ownership
change.

## Intentionally unresolved work

This page describes **current ownership**, not permanent 0.4 stability promises. It does not
resolve reconsiderable pre-1.0 compatibility paths.

- **Phase 25:** scenario and fixture lineage, followed by demo/example/workload simplification.
- **Phase 26:** package, feature, public API, command-surface, and documentation simplification.
- **Phase 27:** invariant-proof ownership, test consolidation, validation simplification, and CI
  simplification.
