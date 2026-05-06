# Changelog

## [] - Unreleased

### Added

- Diagnostic validation suite: benchmark corpus, manifests, scorecards, CI checks, adversarial cases, and release snapshot tooling.
- Unified validation runner for diagnostic, mitigation, and operational profiles.
- First-class analyzer library crate, `tailtriage-analyzer`, for in-process analysis from Rust code.
- Richer analyzer reports: evidence quality, route breakdowns, conservative temporal segments, and optional report-surface validation.

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
