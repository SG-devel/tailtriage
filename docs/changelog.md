# Changelog

## Unreleased

### Changed

- Added a fresh dated MVP audit snapshot (`docs/mvp-audit-2026-03-20.md`) with current acceptance/milestone/scope verification results.
- Consolidated documentation structure around `docs/README.md` as the canonical docs index.
- Reduced duplication across README/user guide/diagnostics/demo docs while keeping MVP integration and diagnosis guidance intact.
- Simplified the historical MVP audit doc and removed stale cross-document references.
- Simplified `tailscope-cli` dependencies by removing a direct `tailscope-tokio` dependency; the CLI only consumes `tailscope-core` analyzer APIs.

### Development timeline (condensed)

- **Foundation (PRs #2-#17, 2026-03-18 to 2026-03-19):** Bootstrapped the Rust workspace, defined the run/report schema and local JSON sink, and shipped first-class request instrumentation plus Tokio runtime metrics sampling.
- **Diagnosis MVP (PRs #21-#34, 2026-03-19):** Implemented core diagnosis rules and fixtures, then connected reproducible queue/backpressure and blocking-contamination demos with end-to-end smoke coverage.
- **Signal quality and architecture hardening (PRs #42-#50, 2026-03-19):** Added explicit in-flight trend evidence, addressed issue-driven correctness gaps (including fixes tracked via #36/#37), modularized `tailscope-core`, and improved integration ergonomics.
- **Developer workflow and reproducibility (PRs #46-#71, 2026-03-19 to 2026-03-20):** Standardized demo tooling and helper scripts, introduced baseline/mitigated scenario workflows, and tightened portability + CI checks for repeatable local diagnosis runs.
- **Documentation and MVP readiness (PRs #51-#75, 2026-03-19 to 2026-03-20):** Expanded onboarding docs (quickstart, first-use, mental model, canonical integration path), aligned report contracts with regression tests, and completed repeated acceptance/scope audits to prepare a clear MVP handoff.
