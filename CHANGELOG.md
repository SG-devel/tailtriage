# Changelog

## [0.4.0] - Unreleased

### Changed

- `InflightTrend::growth_delta` is now `Option<i64>`: migrate typed consumers to handle `None`
  as unavailable direction. Report JSON renders that case as `null`; `Some(0)`/JSON `0` now
  unambiguously means an observed flat multi-sample episode.

### 0.4 tracing API migration

- `SpanRecord` fluent construction now uses `with_id`, `with_parent_id`, `with_field`, `with_started_at_run_us`, `with_finished_at_run_us`, and `with_duration_us`. Its getters now use the exact model names `id`, `parent_id`, `started_at_run_us`, `finished_at_run_us`, and `duration_us` instead of the former `_ref` spellings.
- `ImportOptions` inspection now uses `configured_service_version`, `configured_run_id`, `is_strict`, and `capture_mode` instead of `service_version_ref`, `run_id_ref`, `strict_mode`, and `mode_value`. Setters are unchanged.
- `ImportedRun::new` and `ImportWarning::new` are now crate-owned constructors. Custom sources should construct `SpanRecord` inputs and obtain result values from `run_from_span_records(...)`.

### 0.4 core API migration

- Tokio semaphore helpers are now awaited directly: replace
  `.semaphore(...).acquire().await` with `.semaphore(...).await` and
  `.owned_semaphore(...).acquire_owned().await` with `.owned_semaphore(...).await`.
  The intermediate `InstrumentedSemaphore` and `InstrumentedOwnedSemaphore` types were removed.
  Replace the redundant Tokio `inflight_guard(...)` alias with the canonical core
  `inflight(...)` method.

- Diagnosis taxonomy and Report vocabulary changed as follows:

  | Previous Rust variant / machine label | 0.4 Rust variant / machine label |
  | --- | --- |
  | `ApplicationQueueSaturation` / `application_queue_saturation` | `ApplicationQueuePressure` / `application_queue_pressure` |
  | `ExecutorPressureSuspected` / `executor_pressure_suspected` | `ExecutorPressure` / `executor_pressure` |
  | `DownstreamStageDominates` / `downstream_stage_dominates` | `DownstreamStageDominance` / `downstream_stage_dominance` |

  `BlockingPoolPressure` / `blocking_pool_pressure` and `InsufficientEvidence` /
  `insufficient_evidence` are unchanged. Machine-readable Report JSON uses the 0.4 labels;
  human-readable reports use spaced diagnosis phrases.

- Analyzer configuration now uses direct nested `AnalyzeOptions` mutation; the eight `with_*` closure wrappers and `valid_override_paths()` were removed. Derive paths from `analyze_option_descriptors()` and its read-only descriptor getters. Detailed CLI discovery moved from `analyze --help-analyzer-options` to `tailtriage analyzer-options`, and `analyze` now requires `RUN_JSON` at Clap parsing.

- Direct `Tailtriage::builder(...)` capture now requires `.output(...)` or `.sink(...)`; omission returns `BuildError::MissingSink` instead of implicitly writing `tailtriage-run.json`.
- Replace `.light()` / `.investigation()` with `.mode(CaptureMode::Light)` / `.mode(CaptureMode::Investigation)`, and `selected_mode()` with `capture_mode()`.
- Replace `begin_request_owned` / `begin_request_with_owned` with `begin_owned_request` / `begin_owned_request_with`.
- `RunBuilder::new` now returns `RunBuilderError`, and completed assembly uses `RunBuilder::build()` instead of `finish()`. Invalid zero worker counts are rejected by `push_runtime_snapshot` before retention.
- Core runtime-sampler registration plumbing and the audience-selecting validation-summary helper are no longer supported root APIs; use the Tokio sampler startup errors and the two purpose-specific validation summary functions.
- Controller template validation now uses `ControllerTemplateError`. `ReloadConfigError::Validate` and `ReloadTemplateError::Validate` carry that type instead of core `BuildError`; callers matching those variants must update their patterns and type references accordingly.

- Split direct shutdown failures into `ShutdownError::UnfinishedRequests` and
  `ShutdownError::Sink`, narrowing `SinkError` to sink-produced failures. Consolidated controller
  `DisableError`/`ShutdownError` into `GenerationFinalizationError`, renamed status
  `last_finalize_error` to `last_finalization_error`, and made controller `shutdown()` terminal.
  Use reversible `disable()` for capture-window disarm; post-shutdown enable/reload is rejected and
  status is `GenerationState::Shutdown`.

- Consolidated the unreleased 0.4 live tracing API onto `TracingSession` as the single live entry
  point, replacing the interim recorder, intake-session, and Tokio-session surfaces.

- Removed the deprecated no-op `scripts/validate_all.py --include-cargo` compatibility flag. Cargo completion checks already run by default; omit the flag, or use `--skip-cargo` when intentionally running only the non-Cargo validation tracks.

- Removed residual compatibility-only public APIs before 0.4.0. Migrate `controller.try_begin_request(route)` and `controller.try_begin_request_with(route, options)` to `controller.begin_request(route)` and `controller.begin_request_with(route, options)`; callers that truly need an admission branch can match `ControllerRequestHandle::{Active, Inert}`. Migrate `RuntimeSampler::start(run, interval)?` to `RuntimeSampler::builder(run).interval(interval).start()?`. The `crate_name()` smoke helpers in `tailtriage-tokio` and `tailtriage-axum` were removed without replacement because they were not product behavior. The `tailtriage-cli` command-line package and library documentation target remain supported, but its artifact loader, analyzer-option builder/error, and analyzer-options help renderer are now internal CLI plumbing: use `tailtriage-analyzer` for in-process analysis, `tailtriage-core` for generic Run validation/normalization, and invoke the CLI for saved-artifact command policy.

- Simplified the analyzer API to one checked `analyze_run` operation plus separate `render_text`, `render_json`, and `render_json_pretty` functions. Migrate `try_analyze_run(...)` to `analyze_run(...)?`, `Analyzer` methods to the free function, `analyze_run_json*` / `try_analyze_run_json*` to analysis followed by `render_json*`, and analyzer strict wrappers to `tailtriage_core::validate_run_strict(...)?` followed by analysis.

- Executor-pressure scoring now normalizes runnable queue depth by Tokio worker count when complete worker evidence is available. Historical artifacts retain exact legacy scoring, while partial, inconsistent, or invalid worker evidence uses a confidence-capped legacy fallback.

- Diagnostic validation responsibilities are now documented as typed analyzer-rule tests, deterministic artifact-pipeline regression, and local/manual live-workload validation.

- Committed analyzer fixtures now have a deterministic inventory, byte, formatting, and structural integrity lock checked before diagnostic execution.

- Diagnostic validation now separates analyzer-executed accuracy observations from pre-generated Report-contract checks.

- Native/core Run fixtures and stable tracing JSONL now have a deterministic equivalence contract for shared completed evidence and analyzer results, with explicit Run-only limitations.

- Route and temporal analysis now share internal request-scoped Run slicing and scoped-report projection while preserving their distinct runtime/in-flight attribution policies and existing serialized Report output.
- Simplified controller internals around one pure immutable template resolution path, one generation constructor, and one lifecycle-preserving finalizer. Direct template reload is now the single result-returning `reload_template(template)` API; migrate `try_reload_template(template)?` to `reload_template(template)?`, and handle the `Result` if the former panicking method was used. Direct and TOML reload remain transactional, affect only future generations, and create neither a capture generation nor a runtime sampler.
- CLI Run-artifact analysis is now strict by default. Error-level core findings stop report generation; warning-only findings remain accepted. The removed `--strict-artifact` option is replaced by the explicit `--allow-ambiguous-artifact` compatibility path, which emits every issue and analyzes only canonical normalized evidence. Tracing import `--strict` and permissive analyzer library defaults are unchanged.
- Suspect ranking now selects the primary after final evidence-aware confidence adjustment, ordering by final confidence, unchanged raw score, then stable suspect-kind rank while keeping raw-score ambiguity semantics.
- Analyzer completed queue/stage distributions exclude partial events; partial durations are treated as observed lower bounds, materially partial-dependent queue/stage suspects are capped at medium confidence, partial evidence remains visible through existing evidence-quality, warning, evidence, and confidence-note fields, and tracing intake remains completed-only.
- Added `completed: bool` to public `StageEvent` and `QueueEvent` structs with wire-compatible completed JSON; this is an intentional pre-1.0 Rust source break for exhaustive external struct literals, and constructor-based migration is recommended. Polled-then-dropped core/Tokio queue and stage helpers now record bounded partial evidence while capture remains open.
- Completed-span JSONL now writes retained original tracing sources rather than reconstructing span-shaped records from normalized Run events, preserving source identity and fields while retaining the documented representational limits.
- Core Run validation is now centralized in `tailtriage-core`, with strict validation and deterministic permissive normalization APIs for duplicate request IDs, request-scoped child integrity, required fields, schema version checks, and run-relative timing issues.
- Aligned documented local validation commands with CI baseline flags in `AGENTS.md` and `scripts/validate_all.py`.
- Updated `SPEC.md` and `docs/dev/DESIGN_NOTES.md` to describe current pre-0.4.0 governance, intake, analyzer, lifecycle, validation, and design-risk baselines without claiming future behavior.

## [0.3.0] - 2026-06-18

### Added

- New `tailtriage-tracing` crate for converting `tt.*` tracing span evidence into standard tailtriage `Run` artifacts.
- Optional `tailtriage` facade features for tracing intake integrations.
- JSONL tracing import support for persisted span records.
- Live in-memory tracing recorder APIs for collecting completed tracing spans and converting them into tailtriage runs.
- Optional Tokio session integration for coupling tracing intake with Tokio runtime sampling.
- CLI tracing import command for producing analyzable tailtriage Run JSON from tracing JSONL input.
- Semantic `tt.*` tracing field convention for request, stage, and queue spans.

### Changed

- Expanded the release surface from direct instrumentation-only workflows to include tracing-based intake workflows.
- Kept tracing import output aligned with the existing Run JSON artifact contract and analyzer path rather than introducing a separate tracing-specific analyzer.
- Tightened imported tracing evidence validation around required fields, malformed `tt.*` spans, duplicate request IDs, child-span correlation, timestamp ordering, and persistable zero-request artifacts.
- Added durable import warnings to Run metadata so conversion-quality issues remain visible during later analysis.

### Fixed

- Prevented persisted tracing imports from silently producing analyzer-hostile zero-request Run artifacts.
- Improved handling of tracing spans with missing optional outcome/success fields by defaulting conservatively while surfacing warnings.
- Improved correlation of imported stage and queue spans to retained request intervals, including truncation accounting when matching requests exceed capture limits.

## [0.2.0] - 2026-05-08

### Added

- Diagnostic validation suite: benchmark corpus, manifests, scorecards, CI checks, adversarial cases, and release snapshot tooling.
- Unified validation runner for diagnostic, mitigation, and operational profiles.
- First-class analyzer library crate, `tailtriage-analyzer`, for in-process analysis from Rust code.
- Richer analyzer reports: evidence quality, route breakdowns, conservative temporal segments, and optional report-surface validation.
- Tokio request-handle primitive helpers

### Changed

- Reworked analyzer scoring, warnings, confidence caps, ambiguity handling, and attribution logic to be more evidence-aware and conservative.
- Split analyzer internals and text rendering out of the CLI path.
- Updated docs around validation scope, analyzer/CLI responsibilities, and completed-run artifact contracts.

### Fixed

- Tightened confidence notes, temporal warnings, route-divergence validation, and validation output wording.
- Improved cross-platform CI coverage and artifact finalization behavior.

## [0.1.2] - 2026-04-25

### Added

- Default `tailtriage` crate as the main adoption surface.
- `tailtriage-controller` for live arm/disarm capture windows in long-running services.
- TOML-backed controller configuration with reload semantics and documented field references.
- Controller lifecycle handling, generation scoping, disabled-mode instrumentation, run-end policies, and auto-seal behavior.
- Tokio `RuntimeSampler` builder with capture-mode inheritance, explicit overrides, effective-config metadata, and controller lifecycle integration.
- Collector-stress and collector-limits measurement paths, scripts, tests, CI coverage, and operating docs.
- Improved Axum outcome classification and public controller examples.

### Changed

- Made `CaptureMode` a concrete core preset with clearer defaults and precedence rules.
- Separated core capture overhead from Tokio sampler runtime-cost reporting.
- Optimized saturated collector paths with lower-overhead drop tracking.
- Reworked docs around the default crate, controller usage, public examples, and crates.io onboarding.
- Expanded docs-contract checks across READMEs, examples, and TOML docs.

### Fixed

- docs.rs/crates.io publishing issues.
- Windows dependency, path, TOML fixture, and test issues.
- CLI JSON/report formatting drift.
- Run metadata and controller `service_name` precedence mismatches.
- Added `finalized_at_unix_ms` metadata for finalized artifacts.
- Hardened release CI, cargo-deny checks, docs-contract checks, and public example smoke tests.

## [0.1.1] - 2026-03-27

### Added

- Initial MVP release.
- Core request lifecycle instrumentation for queue, stage, in-flight, and completion timing.
- JSON run artifacts and CLI analysis.
- Evidence-ranked suspects for application queueing, blocking-pool pressure, executor pressure, and downstream stage latency.
- Optional Tokio runtime-pressure sampling.
- Optional Axum middleware/extractor ergonomics.
- Initial examples, demo workloads, diagnostics docs, and crates.io adoption guidance.
