# Changelog

## [0.4.0] - Unreleased

### Changed

- Completed-span JSONL now writes retained original tracing sources rather than reconstructing span-shaped records from normalized Run events, preserving source identity and fields while retaining the documented representational limits.
- Core Run validation is now centralized in `tailtriage-core`, with strict validation and deterministic permissive normalization APIs for duplicate request IDs, request-scoped child integrity, required fields, schema version checks, and run-relative timing issues.
- Aligned documented local validation commands with CI baseline flags in `AGENTS.md` and `scripts/validate_all.py`.
- Updated `SPEC.md` and `DESIGN_NOTES.md` to describe current pre-0.4.0 governance, intake, analyzer, lifecycle, validation, and design-risk baselines without claiming future behavior.

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
