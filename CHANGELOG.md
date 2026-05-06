# Changelog

All notable changes to this project are documented here.

## [] - Unreleased

### Added

- Added diagnostic validation infrastructure, including a benchmark corpus, validation manifests, scorecard output, and CI checks for deterministic diagnostic behavior.
- Added adversarial diagnostic cases and confidence-ceiling checks for ambiguous, partial, or conflicting evidence.
- Added repeated-run validation tooling for diagnostic stability, mitigation movement, and operational profiles.
- Added a unified validation runner, `scripts/validate_all.py`, for profile orchestration.
- Added versioned/manual diagnostic validation snapshot generation for release review.
- Added richer analyzer report surfaces:
  - structured `evidence_quality`
  - route-level breakdowns
  - conservative early/late temporal segments
  - optional analyzer-report validations in the diagnostic benchmark
- Added `tailtriage-analyzer` as a dedicated analyzer crate, making analysis callable directly from Rust code as well as through the CLI.

### Changed

- Reworked analyzer scoring, warning generation, confidence capping, ambiguity handling, and temporal/overlap attribution to make reports more evidence-aware and conservative.
- Split analyzer rendering and scoring internals into smaller modules before extracting the analyzer as a first-class library crate.
- Updated documentation to explain the analyzer/CLI split and the completed-run artifact contract.

### Fixed

- Tightened validation warning wording, optional benchmark output, confidence-note truthfulness, route-divergence coverage, and temporal segment warning semantics.
- Added cross-platform CI coverage and simplified artifact finalization behavior.

## [0.1.2] - 2026-04-25

### Added

- Added the `tailtriage` default crate as the primary adoption surface, while keeping focused subcrates available for lower-level integrations.
- Added the `tailtriage-controller` crate for live arm/disarm capture windows in long-running services.
- Added controller generation lifecycle management, request admission/completion binding, disabled-mode request instrumentation, and controller examples.
- Added TOML-backed controller configuration with startup defaults, reload semantics for future activations, runtime sampler template settings, and documented field references.
- Added controller run-end policies, including saturation handling and optional auto-seal behavior.
- Added a Tokio `RuntimeSampler` builder with `CaptureMode` inheritance, explicit overrides, effective-config metadata, retention clamping, and lifecycle scoping to active controller generations.
- Added collector-stress and collector-limits measurement paths, orchestration scripts, bounded smoke coverage, deterministic summary tests, and operating guidance.
- Added richer Axum outcome handling by mapping HTTP status codes to more useful request outcomes and allowing configurable classification.
- Added public controller examples and refreshed public example lists for crates.io/docs.rs users.

### Changed

- Made `CaptureMode` a concrete core preset and clarified mode defaults, sampler precedence, and runtime-cost attribution.
- Separated core capture-mode overhead from Tokio sampler runtime-cost reporting.
- Optimized saturated collector paths with lower-overhead drop tracking.
- Rebalanced documentation around the default crate, controller adoption path, public examples, and published-crate onboarding.
- Expanded docs-contract validation to keep README content, crate docs, examples, and TOML documentation aligned.

### Fixed

- Fixed docs.rs and crates.io publishing gaps across public crates.
- Fixed Windows-specific dependency, path, TOML fixture, and test issues.
- Fixed CLI JSON/report formatting drift and added release-facing metadata such as `rust-version`.
- Fixed run metadata and controller `service_name` precedence mismatches.
- Added `finalized_at_unix_ms` metadata to distinguish finalized artifacts.
- Hardened CI with cargo-deny coverage, docs-contract checks, release smoke checks, public example smoke checks, and collector-limit tests.

## [0.1.1] - 2026-03-27

### Added

- Initial MVP release for Tokio tail-latency triage.
- Added core request lifecycle instrumentation for queue, stage, in-flight, and completion timing.
- Added JSON run artifact output and CLI analysis for saved artifacts.
- Added evidence-ranked suspect output for the initial bottleneck families:
  - application queueing
  - blocking-pool pressure
  - executor pressure
  - downstream stage latency
- Added optional Tokio runtime-pressure sampling.
- Added optional Axum middleware/extractor ergonomics.
- Added initial public examples, demo workloads, diagnostics documentation, and crates.io adoption guidance.

### Notes

- Suspects were intentionally presented as triage leads with next checks, not as root-cause proof.
