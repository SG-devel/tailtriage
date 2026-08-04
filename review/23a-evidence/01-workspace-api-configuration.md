# 23A-1 Workspace, API, and Configuration Evidence

## 1. Inspection baseline

- Repository: `SG-devel/tailtriage`; starting SHA `66a31701ff5f0455595977c683981bb54df3165d`; inspection date 2026-08-04 UTC.
- `rustc 1.95.0`, `cargo 1.95.0`, active `1.95.0-x86_64-unknown-linux-gnu`; Git, Cargo, Python, `find`, `rg`, and `sed` were available.
- The initial working tree was clean.
- No production, test, fixture, manifest, workflow, script, or user-documentation file was changed.

## 2. Workspace summary

Cargo metadata reports all packages at version 0.3.0. `publish = false` below means Cargo metadata returned an empty publish list; “allowed” means no manifest restriction.

| Package | Path | Version | Publish setting | Package role stated by code/docs | Internal dependencies | Notable features |
|---|---|---:|---|---|---|---|
| `tailtriage-core` | `tailtriage-core` | 0.3.0 | allowed | Run schema, capture, lifecycle, integrity | none | none |
| `tailtriage-tokio` | `tailtriage-tokio` | 0.3.0 | allowed | Tokio helpers/runtime sampler | core | none |
| `tailtriage-axum` | `tailtriage-axum` | 0.3.0 | allowed | Axum middleware/extractor | core | none |
| `tailtriage-controller` | `tailtriage-controller` | 0.3.0 | allowed | arm/disarm generation controller | core, Tokio | none |
| `tailtriage-tracing` | `tailtriage-tracing` | 0.3.0 | allowed | stable tracing intake/live bridge | core; Tokio optional | `default=jsonl`, `jsonl`, `live`, `tokio` |
| `tailtriage-analyzer` | `tailtriage-analyzer` | 0.3.0 | allowed | evidence-ranked analysis/rendering | core | none |
| `tailtriage-cli` | `tailtriage-cli` | 0.3.0 | allowed | artifact/import/analyze command boundary | analyzer, core, tracing (`jsonl`) | none |
| `tailtriage` | `tailtriage` | 0.3.0 | allowed | default facade | core; optional controller/Tokio/Axum/tracing | seven named features |
| `demo-support` | `demos/demo_support` | 0.3.0 | false | shared demo capture/tracing support | core, tracing (`tokio`) | none |
| `queue-service-demo` | `demos/queue_service` | 0.3.0 | false | queue-service proof-case binary | core, demo-support | none |
| `blocking-service-demo` | `demos/blocking_service` | 0.3.0 | false | blocking-service proof-case binary | core, demo-support | none |
| `executor-pressure-service-demo` | `demos/executor_pressure_service` | 0.3.0 | false | executor-pressure proof-case binary | core, demo-support | none |
| `downstream_service` | `demos/downstream_service` | 0.3.0 | false | downstream-service proof-case binary | core, demo-support | none |
| `mixed_contention_service` | `demos/mixed_contention_service` | 0.3.0 | false | mixed-contention proof-case binary | core, demo-support | none |
| `cold-start-burst-service-demo` | `demos/cold_start_burst_service` | 0.3.0 | false | cold-start-burst proof-case binary | core, demo-support | none |
| `db-pool-saturation-service-demo` | `demos/db_pool_saturation_service` | 0.3.0 | false | database-pool-saturation proof-case binary | core, demo-support | none |
| `shared-state-lock-service-demo` | `demos/shared_state_lock_service` | 0.3.0 | false | shared-state-lock proof-case binary | core, demo-support | none |
| `retry-storm-service-demo` | `demos/retry_storm_service` | 0.3.0 | false | retry-storm proof-case binary | core, demo-support | none |
| `collector_stress` | `demos/collector_stress` | 0.3.0 | false | collector stress binary | core, Tokio | none |
| `runtime_cost` | `demos/runtime_cost` | 0.3.0 | false | runtime-cost binary | analyzer, core, Tokio, tracing | none |

Product targets are libraries except CLI (library+binary); facade and component crates also declare examples/integration tests. Demo packages are binaries except `demo-support`.

## 3. Dependency direction

```text
core <- analyzer <- CLI
core <- tracing(jsonl) <- CLI
core <- Tokio <- controller
core <- Axum
core <- facade -> optional {controller, Tokio, Axum, tracing}
tracing(tokio) -> Tokio
demo-support -> {core, tracing(tokio)}; service demos -> {demo-support, core}
runtime_cost -> {analyzer, core, Tokio, tracing(tokio)}
collector_stress -> {core, Tokio}
```

The facade always re-exports core and exposes optional crates as namespaces (`tailtriage/src/lib.rs`). Analyzer and tracing both apply generic core normalization at their boundary; CLI adds saved-artifact and command-specific policy. Whether this policy overlap is historically intentional could not be determined from repository evidence.

## 4. Feature matrix

| Package | Feature | Default? | Enables dependency or code path | Public surface affected | Evidence |
|---|---|---:|---|---|---|
| facade | `controller` | yes | optional controller crate | `tailtriage::controller` | `tailtriage/Cargo.toml`; `tailtriage/src/lib.rs` |
| facade | `tokio` | yes | optional Tokio crate | `tailtriage::tokio` | same |
| facade | `axum` | no | optional Axum crate | `tailtriage::axum` | same |
| facade | `tracing` | no | tracing + its `jsonl` | `tailtriage::tracing`, import APIs | same |
| facade | `tracing-live` | no | `tracing` + tracing `live` | live layer/session | same |
| facade | `tracing-tokio` | no | `tokio` + `tracing-live` + tracing `tokio` | runtime-coupled session | same |
| facade | `full` | no | controller, Tokio, Axum, tracing-Tokio | all facade namespaces | same |
| tracing | `jsonl` | yes | `serde_json` | JSONL reader/path import | `tailtriage-tracing/Cargo.toml`; `tailtriage-tracing/src/lib.rs` |
| tracing | `live` | no | `jsonl`, tracing/subscriber | recorder, layer, session | same |
| tracing | `tokio` | no | `live`, Tokio crate/runtime | optional Tokio session module | same |

Other product crates declare no named features. Facade compile-surface tests in `tailtriage/src/lib.rs` and `tailtriage/tests/tracing_facade.rs` exercise re-export paths.

## 5. Public API ownership map

### Core capture and Run model

| Surface | File and symbol | Public entry style | Validation/failure behavior | Delegates to | Compatibility notes |
|---|---|---|---|---|---|
| live capture | `tailtriage-core/src/config.rs::TailtriageBuilder`; `tailtriage-core/src/collector.rs::Tailtriage::builder` | builder, checked build | empty name/build errors | `Config::from_builder`, collector | full limits and field overrides coexist |
| request lifecycle | `tailtriage-core/src/collector.rs::Tailtriage::begin_request*`; `RequestHandle`; `RequestCompletion` | borrowed/owned direct APIs, RAII completion | late events inert; Drop cancellation while open | collector state | owned/borrowed pairs |
| artifact assembly | `tailtriage-core/src/run_builder.rs::{RunBuilderOptions, RunBuilder}` | checked builder + `finish` | shape errors on push; finish normalizes | core validation | import/conversion path, not live capture |
| integrity | `tailtriage-core/src/validation.rs::{inspect_run, validate_run_strict, normalize_run_permissive}` | inspect/checked/repair free functions | report, error, or normalized Run+dispositions | shared inspection | strict/permissive pair |
| finalize | `tailtriage-core/src/collector.rs::Tailtriage::{snapshot,shutdown}` | snapshot; checked shutdown | snapshot nonterminal; shutdown sink/lifecycle error | Run sink | terminal error replay |

### Tokio helpers

| Surface | File and symbol | Public entry style | Validation/failure behavior | Delegates to | Compatibility notes |
|---|---|---|---|---|---|
| await helpers | `tailtriage-tokio/src/lib.rs::TokioRequestHandleExt` | sealed extension trait | preserves wrapped result | core queue/stage/inflight | low-level core timers remain available |
| sampler | `tailtriage-tokio/src/lib.rs::RuntimeSampler::{builder,start,shutdown}` | builder/direct start; async shutdown | checked start | core runtime snapshots/hidden registration | mode defaults plus explicit knobs |

### Axum integration

| Surface | File and symbol | Public entry style | Validation/failure behavior | Delegates to | Compatibility notes |
|---|---|---|---|---|---|
| middleware/extractor | `tailtriage-axum/src/lib.rs::{middleware,middleware_with_status_classifier,TailtriageRequest}` | async free/default and classifier factory | missing extractor returns HTTP 500 | owned core request | raw-path fallback without `MatchedPath` |

### Controller

| Surface | File and symbol | Public entry style | Validation/failure behavior | Delegates to | Compatibility notes |
|---|---|---|---|---|---|
| construction/config | `tailtriage-controller/src/lib.rs::{TailtriageControllerBuilder,TailtriageControllerTemplate}`; `TailtriageController::load_config_from_path` | builder and associated TOML loader | typed build/load errors | resolved template/core builder | direct and file templates |
| lifecycle | `tailtriage-controller/src/lib.rs::TailtriageController::{enable,disable,shutdown,reload_*}` | checked direct methods | strict retry vs terminal sink error modeled | active generation/core shutdown | terminal failure replay |
| request | `tailtriage-controller/src/lib.rs::TailtriageController::{begin_request*,try_begin_request*}`; `ControllerRequestCompletion` | admitting/inert and optional checked forms | disabled direct path returns inert; try path returns `None` | core owned request | completion Drop finalizes admission count |

### Tracing

| Surface | File and symbol | Public entry style | Validation/failure behavior | Delegates to | Compatibility notes |
|---|---|---|---|---|---|
| typed/stable import | `tailtriage-tracing/src/lib.rs::run_from_span_records`; `tailtriage-tracing/src/jsonl.rs::import_jsonl_*` | checked free functions, feature gated JSONL | strict fails first violation; non-strict warns/skips | core strict/normalize, `RunBuilder` concepts | stable wrapper required by JSONL parser |
| live | `tailtriage-tracing/src/recorder.rs::{TracingSessionBuilder,TracingSession,TailtriageLayer}` | builder, snapshot, async shutdown | checked; bounds/drop warnings retained | importer/core | gated `live`; Tokio additions gated `tokio` |
| manual runtime evidence | `tailtriage-tracing/src/recorder.rs::TracingSession::record_runtime_snapshot` | session method | returns an import error if manual snapshots were not enabled | runtime collector/core snapshots | gated by `tokio` |
| persistability policy | `tailtriage-tracing/src/lib.rs::ensure_persistable_run_has_requests` | checked free function | rejects a Run with no completed requests | Run request inventory | used before optional Run persistence and by the CLI import path |

### Analyzer

| Surface | File and symbol | Public entry style | Validation/failure behavior | Delegates to | Compatibility notes |
|---|---|---|---|---|---|
| analysis | `tailtriage-analyzer/src/lib.rs::{analyze_run,try_analyze_run,Analyzer}` | free/reusable; panicking/checked | checked config error or convenience panic | common `analyze_run_impl` after permissive normalize | strict artifact compatibility entry also exists |
| configuration | `tailtriage-analyzer/src/lib.rs::{AnalyzeOptions,AnalyzeConfigError,AnalyzeOptionDescriptor,analyze_option_descriptors}` | exported types and descriptor function | typed parse/validation errors; descriptors expose registered option metadata | `tailtriage-analyzer/src/options/` | analyzer public API owns runtime options, configuration errors, and descriptor-backed discovery |
| rendering | `tailtriage-analyzer/src/render.rs::render_text`; `tailtriage-analyzer/src/lib.rs::{render_json,render_json_pretty}` | free functions | JSON serialization error | Report/serde | compact/pretty pairs |

### CLI

| Surface | File and symbol | Public entry style | Validation/failure behavior | Delegates to | Compatibility notes |
|---|---|---|---|---|---|
| commands | `tailtriage-cli/src/main.rs::{Cli,Command,run}` (crate-private binary boundary) | Clap binary | IO/config/import/analyze errors become command failure | artifact, tracing, analyzer | permissive saved-artifact flag |
| artifact load | `tailtriage-cli/src/artifact.rs::{load_run_artifact,decode_run_artifact}` | checked public library functions | schema/decode errors; canonical normalization | core | preserves original and normalized candidate |
| analyzer configuration | `tailtriage-cli/src/lib.rs::{build_analyze_options,analyzer_options_help_text}` | checked builder and help renderer | build returns typed CLI/analyzer configuration errors | analyzer defaults, optional TOML, ordered CLI overrides, analyzer descriptors | library surface shared by CLI command handling and consumers |

### Facade

| Surface | File and symbol | Public entry style | Validation/failure behavior | Delegates to | Compatibility notes |
|---|---|---|---|---|---|
| default surface | `tailtriage/src/lib.rs` | wildcard core re-export; feature-gated crate aliases | delegated unchanged | component crates | direct crate imports remain possible |

## 6. End-to-end path map

### Native capture

`Tailtriage::builder` -> `TailtriageBuilder::build` (`tailtriage-core/src/config.rs`) -> `begin_request*` and queue/stage/inflight/runtime events (`tailtriage-core/src/collector.rs`) -> `snapshot` or `shutdown` -> collector Run assembly/persist -> `normalize_run_permissive` (`tailtriage-core/src/validation.rs`) at the analyzer boundary or strict validation -> `try_analyze_run`/`Analyzer::try_analyze_run` (`tailtriage-analyzer/src/lib.rs`) -> `render_text`/JSON renderers (`tailtriage-analyzer/src/render.rs`, `tailtriage-analyzer/src/lib.rs`).

### Tracing import

Stable JSONL `tailtriage-tracing/src/jsonl.rs::{import_jsonl_reader, import_jsonl_path}` -> wrapper parsing into `SpanRecord` -> `run_from_span_records` / provenance conversion -> optional `validate_run_strict`, always `normalize_run_permissive` -> `ImportedRun` retaining warnings and source provenance -> CLI/library analyzer -> Report renderer.

### Live tracing

`TracingSession::builder` / `TracingSessionBuilder` -> `build` -> `TailtriageLayer` + session -> optional `sampler_interval` or `manual_runtime_snapshots` under `tokio` -> `snapshot_run` or async `shutdown` -> normalized `ImportedRun`; shutdown optionally writes completed-span JSONL and/or Run JSON (`tailtriage-tracing/src/recorder.rs`).

### Saved artifact CLI analysis

`tailtriage-cli/src/artifact.rs::decode_run_artifact` reads/decodes original Run -> schema check -> canonical permissive normalization retained beside original -> `tailtriage-cli/src/main.rs` strict default validates the original candidate unless `--allow-ambiguous-artifact` -> analyzer consumes normalized evidence -> JSON (`render_json*`) or text (`render_text`). Command policy additionally requires analyzable/persistable content.

### Controller lifecycle

`TailtriageControllerBuilder` and optional `ControllerConfigFile` -> `resolve_controller_template` -> `enable` constructs generation/core collector and optional runtime sampler -> `begin_request*` admission and `ControllerRequestCompletion` -> `disable`/`shutdown`/limit auto-seal -> generation finalizer -> core shutdown/local JSON artifact.

## 7. Configuration ownership map

| Configuration domain | Runtime type | Defaults owner | File/TOML owner | CLI owner | Merge/precedence owner | Validation owner | Reporting owner |
|---|---|---|---|---|---|---|---|
| core capture/lifecycle | `CaptureMode`, `CaptureLimits`, `TailtriageBuilder`, `EffectiveCoreConfig` | `CaptureMode::core_defaults`, builder | none | import CLI flags | `Config::from_builder`: full override else field override over mode | builder/core lifecycle | Run metadata |
| controller | `TailtriageControllerTemplate`, `ControllerActivationTemplate` | builder/runtime defaults | private `Controller*Toml` | none | `resolve_controller_template`; loaded activation replaces activation fields | build/load/reload/enable | controller status + Run |
| tracing import/session | `ImportOptions`, `RecorderLimits`, `TracingSessionBuilder` | constructors/constants/mode defaults | stable JSONL is evidence input, not config | import flags | full limits else field override; setters | import/session build + core validation | warnings, Run metadata/artifacts |
| analyzer | `AnalyzeOptions`, `AnalyzeConfigError`, `AnalyzeOptionDescriptor`, `analyze_option_descriptors`, and eight nested option types (`tailtriage-analyzer/src/lib.rs`, `tailtriage-analyzer/src/options/`) | `Default` impls | `AnalyzerTomlConfig` patch | `tailtriage-cli::build_analyze_options` owns defaults -> optional TOML -> ordered `--analyzer-set` overrides; `tailtriage-cli::analyzer_options_help_text` renders descriptor-backed help (`tailtriage-cli/src/lib.rs`) | analyzer TOML/override methods and CLI library builder | `AnalyzeOptions::validate` | `AnalyzerConfigSummary`, non-default overrides, descriptor-backed CLI help |
| CLI output/artifact | Clap `Cli`/`Command` fields | Clap defaults | analyzer config path | CLI | CLI command dispatch | CLI path/format and delegated validation | stdout/stderr/files |

No environment-variable configuration handler was found by the targeted searches. Analyzer TOML uses `deny_unknown_fields`; controller TOML does not declare it at the inspected private structs.

## 8. Analyzer option path

Mechanical path: `AnalyzeOptions::default` -> 30 `OPTION_ENTRIES` in `tailtriage-analyzer/src/options/registry.rs` -> `analyze_option_descriptors` generated in `tailtriage-analyzer/src/options/descriptors.rs` -> `from_toml_str`/`merge_toml_str` private patch types -> `tailtriage_cli::build_analyze_options` in `tailtriage-cli/src/lib.rs` applies optional TOML and ordered CLI overrides -> registry typed setter -> `AnalyzeOptions::validate` -> checked/free or reusable Analyzer -> `non_default_overrides` in report configuration summary. `tailtriage_cli::analyzer_options_help_text` renders help from those descriptors.

- Eight domains and 30 registered paths are the path source of truth; nested `Default` implementations are runtime-default truth. Registry descriptor strings are checked against those defaults by tests.
- TOML schema version is 1 and private config/section structs use `deny_unknown_fields`. TOML uses serde typing; CLI accepts `path=value` and registry parses `u64`, `usize`, `u8`, exact booleans, or comma-separated string lists.
- `merge_toml_str` and `apply_overrides` operate on a clone and commit only after parsing/application/semantic validation, providing transactional behavior; override order is caller iteration order.
- CLI constructs defaults, merges TOML, then applies CLI overrides. Validation occurs after each public configuration operation and again in checked analysis.
- `try_analyze_run` and `Analyzer::try_analyze_run` return config errors. `analyze_run` and `Analyzer::analyze_run` panic on invalid options and otherwise converge on shared analysis. Strict artifact analysis is a separate checked compatibility path.

## 9. Run integrity ownership

- Generic inspection is `tailtriage-core/src/validation.rs::inspect_run`; strict rejection is `tailtriage-core/src/validation.rs::validate_run_strict`; deterministic repair/filtering is `tailtriage-core/src/validation.rs::normalize_run_permissive`, returning the normalized Run, complete findings, and event dispositions.
- Native collector snapshots/finalization in `tailtriage-core/src/collector.rs` build Runs; `tailtriage-core/src/run_builder.rs::RunBuilder::finish` normalizes and persists lifecycle summaries for completed assembly.
- Tracing first constructs an original candidate, optionally strictly validates it, always normalizes it, maps dispositions back to retained source spans, retains warnings, and refreshes bounds/provenance (`tailtriage-tracing/src/lib.rs`).
- Analyzer permissively normalizes input and analyzes normalized evidence; `validate_artifact_strict`/`try_analyze_run_strict_artifact` preserve strict compatibility behavior (`tailtriage-analyzer/src/lib.rs`).
- CLI `LoadedArtifact` retains both original decoded candidate and normalized Run/report. Strict default policy validates the original; the permissive flag emits normalization warnings and continues with normalized evidence (`tailtriage-cli/src/artifact.rs`, `tailtriage-cli/src/main.rs`).
- Zero-request persistence suitability is owned by `tailtriage-tracing/src/lib.rs::ensure_persistable_run_has_requests`; analyze suitability and command flags are integration/CLI policy beyond generic Run integrity. Core accepts a generically valid empty Run.

## 10. Lifecycle and finalization ownership

- Core collector phases and `TerminalShutdown` live in `tailtriage-core/src/collector.rs`. `snapshot` is nonterminal. `shutdown` seals, summarizes unfinished/lifecycle state, writes through `RunSink`, and replays terminal sink outcome on later calls.
- Borrowed/owned completion tokens finish explicitly; Drop records `cancelled` only for an admitted unfinished request while capture remains open. Late Drop after finalization is inert.
- Controller ownership in `tailtriage-controller/src/lib.rs` covers admission gates, generation state, sampler stop, and invoking core finalization. `ControllerRequestCompletion::drop` decrements captured admission bookkeeping and may trigger deferred finalization. Disabled `TailtriageController::begin_request` creates inert handles; `TailtriageController::try_begin_request` returns `None`.
- Controller strict lifecycle failure before sink attempt remains retryable; a sink-attempted failure is terminal and stored/replayed in controller status/errors (`ActiveGenerationState::last_finalize_error`).
- Tracing lifecycle methods in `tailtriage-tracing/src/recorder.rs` convert current completed candidates without closing on `TracingSession::snapshot_run`. Async `TracingSession::shutdown` stops the optional sampler, freezes the recorder, converts, and conditionally writes completed-span JSONL and Run JSON. Its errors are returned as `ImportError`.

## 11. Compatibility and fallback inventory

| Compatibility path | Current behavior | Code owner | Tests/docs indicating obligation | Supported population known? |
|---|---|---|---|---|
| Run schema version | CLI decoder rejects unsupported version | `tailtriage-cli/src/artifact.rs` | CLI boundary/schema tests | unknown |
| strict vs permissive Run | strict returns findings as error; permissive repairs/filters with warnings | `tailtriage-core/src/validation.rs` | extensive core/CLI/tracing tests | unknown |
| analyzer strict compatibility | checked strict artifact functions coexist with permissive default analysis | `tailtriage-analyzer/src/lib.rs` | public API contract/tests/rustdoc | unknown |
| missing worker count | executor evidence falls back to non-worker-normalized signals | `tailtriage-core/src/events.rs::RuntimeSnapshot::worker_count`; `tailtriage-analyzer/src/scoring.rs::{classify_worker_evidence, executor_pressure_suspect}` | analyzer tests | unknown |
| invalid/partial worker evidence | generic validation reports/repairs invalid count; analyzer bounds evidence | `tailtriage-core/src/validation.rs::{normalize_inner, RunValidationIssueCode::InvalidWorkerCount}`; `tailtriage-analyzer/src/scoring.rs::{classify_worker_evidence, executor_pressure_suspect}` | core/analyzer tests | unknown |
| missing optional precision | partial/run-relative intervals produce warnings/exclusions according to validation | `tailtriage-core/src/validation.rs::{normalize_inner, RunValidationIssueCode::PartialRunRelativeInterval, RunValidationIssueCode::PreciseIntervalValidationUnavailable, RunValidationIssueCode::DurationMismatch}` | core/CLI/tracing parity tests | unknown |
| stable tracing JSONL wrapper | importer requires stable wrapper/record shape and rejects malformed records | `tailtriage-tracing/src/jsonl.rs` | tracing/CLI import tests and README contract | unknown |
| strict/non-strict tracing source | strict fails malformed tagged source; non-strict skips with retained warnings | `tailtriage-tracing/src/lib.rs::{run_from_span_records, convert_span_records_with_provenance, strict_or_warn}` | tracing equivalence tests | unknown |
| facade/direct crates | facade aliases optional crates; direct dependencies remain public packages | `tailtriage/Cargo.toml`; `tailtriage/src/lib.rs`; component crate manifests | facade compile tests/readmes | unknown |
| panicking/checked analyzer | convenience APIs panic only on invalid options; `try_*` returns error | `tailtriage-analyzer/src/lib.rs::{analyze_run, try_analyze_run}`; `tailtriage-analyzer/src/lib.rs::Analyzer::{analyze_run, try_analyze_run}` | public API tests | unknown |
| Axum unmatched route | raw URI path used when `MatchedPath` absent | `tailtriage-axum/src/lib.rs::{middleware, request_route_label}` | adapter/unit tests | unknown |
| deprecated APIs | targeted `deprecated` search found no significant public deprecation attribute | No code owner found; workspace-wide `deprecated` search recorded in `review/23a-evidence/command-record.md` | command record | unknown |

## 12. Repeated and parallel surfaces

| Exact files/symbols | Shared | Difference | Tested/documented? | Necessity established? |
|---|---|---|---|---|
| `tailtriage-core/src/config.rs::TailtriageBuilder`; `tailtriage-core/src/run_builder.rs::RunBuilder`; `tailtriage-controller/src/lib.rs::TailtriageControllerBuilder`; `tailtriage-tracing/src/recorder.rs::TracingSessionBuilder`; `tailtriage-tokio/src/lib.rs::RuntimeSamplerBuilder` | fluent construction | live capture, completed assembly, lifecycle control, tracing, sampling responsibilities | rustdoc/examples/tests | no; distinct mechanics are established |
| `tailtriage-core/src/collector.rs::Tailtriage::builder`; `tailtriage-core/src/config.rs::TailtriageBuilder::new`; `tailtriage-tokio/src/lib.rs::RuntimeSampler::{builder, start}` | construct same owned type | direct convenience versus configurable builder | tests/docs | no |
| `tailtriage-analyzer/src/lib.rs::{analyze_run,try_analyze_run,Analyzer}` | same options and analysis implementation | reusable stored options versus per-call options | public API tests | no |
| `tailtriage-analyzer/src/lib.rs::{analyze_run, try_analyze_run}`; `tailtriage-analyzer/src/lib.rs::Analyzer::{analyze_run, try_analyze_run}` | same successful report | returned config error versus panic | tests/rustdoc | no |
| `tailtriage-analyzer/src/render.rs::render_text`; `tailtriage-analyzer/src/lib.rs::{render_json, render_json_pretty}` | Report input | serialization formatting/schema | render/schema tests | no |
| `tailtriage/Cargo.toml`; `tailtriage/src/lib.rs`; root `Cargo.toml` workspace component dependencies | same component implementation | import path and feature activation | compile tests/docs | no |
| `tailtriage-core/src/config.rs::{TailtriageBuilder, CaptureLimitsOverride}`; `tailtriage-controller/src/lib.rs::{TailtriageControllerBuilder, TailtriageControllerTemplate}`; `tailtriage-tracing/src/types.rs::ImportOptions`; `tailtriage-tracing/src/recorder.rs::{TracingSessionBuilder, RecorderLimits}` | mode/limits/strict fields | owner and lifecycle/application time | unit/integration docs/tests | no |
| `tailtriage-core/src/validation.rs`; adapters in `tailtriage-tracing/src/lib.rs`, `tailtriage-analyzer/src/lib.rs`, and `tailtriage-cli/src/artifact.rs` | same core integrity functions | boundary-specific persistence/command policy | parity/boundary tests | no |
| `tailtriage-core/src/collector.rs::Tailtriage::{snapshot,shutdown}`; `tailtriage-tracing/src/recorder.rs::TracingSession::{snapshot_run,shutdown}`; `tailtriage-controller/src/lib.rs::TailtriageController::shutdown` | artifact/lifecycle vocabulary | terminal ownership and return types | tests/rustdoc | no |
| `tailtriage-analyzer/src/options/{registry,descriptors}.rs`; `tailtriage-analyzer/src/lib.rs::AnalyzerConfigSummary` | same 30 option paths | runtime values versus display/help/report representation | descriptor/config tests | no |
| `tailtriage-controller/src/lib.rs::TailtriageController::{begin_request*,try_begin_request*}`; `tailtriage-core/src/collector.rs::Tailtriage::{begin_request*,begin_request_owned*}` | request lifecycle | inert versus optional admission; ownership/lifetime | controller/core tests | no |

Further review is required to decide whether any observed repetition is necessary or accidental; repository evidence alone does not decide that question.

## 13. Evidence gaps

- External artifact populations and producer versions.
- Actual user adoption of facade versus direct component crates.
- External use of strict compatibility and panicking convenience APIs.
- Unpublished consumer feature combinations.
- Historical reasons not present in code, tests, docs, or the single checked-out commit context.

## 14. Mechanical observations for reviewer follow-up

**OBS-23A1-001**
Observed state: Five responsibility-specific builder families expose overlapping mode/limit/lifecycle vocabulary.
Evidence: `tailtriage-core/src/config.rs::TailtriageBuilder`, `tailtriage-core/src/run_builder.rs::RunBuilder`, `tailtriage-controller/src/lib.rs::TailtriageControllerBuilder`, `tailtriage-tracing/src/recorder.rs::TracingSessionBuilder`, `tailtriage-tokio/src/lib.rs::RuntimeSamplerBuilder`.
Difference or repetition: construction style repeats; produced object and lifecycle differ.
What repository evidence establishes: each has tests/docs and distinct call sites.
What repository evidence does not establish: external usage or whether every overlap is necessary.

**OBS-23A1-002**
Observed state: Analyzer exposes free/reusable and checked/panicking pairs.
Evidence: `tailtriage-analyzer/src/lib.rs::{analyze_run,try_analyze_run,Analyzer}`.
Difference or repetition: successful paths converge; invalid configuration failure mode differs.
What repository evidence establishes: parity and failure semantics are tested.
What repository evidence does not establish: downstream dependence on every form.

**OBS-23A1-003**
Observed state: Generic Run normalization is invoked at core assembly, conversion of tracing data, analyzer, and CLI artifact boundaries.
Evidence: `tailtriage-core/src/run_builder.rs`, `tailtriage-tracing/src/lib.rs`, `tailtriage-analyzer/src/lib.rs`, `tailtriage-cli/src/artifact.rs`.
Difference or repetition: core algorithm is shared; warnings/provenance/command policy differ.
What repository evidence establishes: normalized-evidence parity is tested.
What repository evidence does not establish: historical placement rationale.

**OBS-23A1-004**
Observed state: Analyzer option representation spans nested defaults, a 30-entry registry, TOML patches, CLI overrides, and report summaries.
Evidence: `tailtriage-analyzer/src/options/{mod,registry,toml,overrides,descriptors}.rs`; `tailtriage-cli/src/lib.rs::{build_analyze_options, analyzer_options_help_text}`; `tailtriage-cli/src/main.rs` for command-line flag wiring.
Difference or repetition: paths are registry-owned while runtime defaults are `Default`-owned.
What repository evidence establishes: consistency and transactional application are tested.
What repository evidence does not establish: whether external tooling consumes descriptors.

**OBS-23A1-005**
Observed state: Facade and direct-crate entry paths coexist with a feature ladder.
Evidence: `tailtriage/Cargo.toml`, `tailtriage/src/lib.rs`, component manifests.
Difference or repetition: identical implementation is reached through alternate imports/activation.
What repository evidence establishes: compile-surface tests cover facade paths.
What repository evidence does not establish: adoption ratios or unpublished combinations.

**OBS-23A1-006**
Observed state: Core, controller, and tracing expose snapshot/finalization vocabulary with different terminal models.
Evidence: `tailtriage-core/src/collector.rs`, `tailtriage-controller/src/lib.rs`, `tailtriage-tracing/src/recorder.rs`.
Difference or repetition: snapshot is nonterminal; shutdown ownership, async behavior, retry, and persistence differ.
What repository evidence establishes: terminal replay and strict retry behavior are encoded in code/tests.
What repository evidence does not establish: user comprehension or external lifecycle patterns.

## 15. Command summary

See [command-record.md](command-record.md). Baseline, metadata, both Cargo trees, manifest discovery, requested public/configuration searches, targeted source reads, and analyzer path counting succeeded. Raw metadata/tree output was not committed. Lightweight Markdown/diff validation is recorded there; no full test suite was run solely for evidence files.
