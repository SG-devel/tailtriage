# Changelog

## Unreleased

### Milestones: how `tailtriage` became a launchable MVP

This changelog now tells the condensed product story of the repository so far, based on merged pull requests.

1. **Foundation and onboarding (early MVP framing).**
   The project established a clear first-use workflow, a newcomer mental model, and a unified demo command surface (`demo_tool`) so contributors and first-time users could run/validate scenarios consistently. Early docs and CI work focused on making deterministic demo validation repeatable and understandable.

2. **Deterministic triage demos became the proof surface.**
   The repository added realistic Tokio-tail-latency triage scenarios (executor pressure, mixed contention, downstream stage slowness, DB pool saturation, shared lock contention, retry storm, cold-start burst), then hardened fixtures and validators so diagnosis outcomes stayed stable enough to trust in CI.

3. **MVP scope sharpened around Tokio tail-latency triage.**
   Positioning tightened away from broad observability claims toward one purpose: evidence-ranked suspects and next checks for async Rust services. During this phase, the project was renamed from `tailscope` to `tailtriage`, and docs were re-centered on honest product fit/non-fit.

4. **Unified public API landed and was cleaned up.**
   The core integration converged on a single builder/request-context lifecycle model, then iterated for safety and ergonomics (typed outcomes, clearer finish semantics, split started-request flow, compatibility cleanup). This was the key technical milestone that made integration coherent rather than fragmented.

5. **Analysis contract and artifact schema stabilized.**
   JSON/reporting semantics were tightened (including suspect-kind normalization and explicit schema versioning), capture limits/truncation behavior became explicit, and CLI artifact validation/error messaging was hardened to support partial instrumentation while preserving trustworthy diagnosis outputs.

6. **Launch-readiness gates moved from aspiration to enforcement.**
   The repo added release-gate docs, publish metadata, workspace/package hygiene, rustdoc completeness checks, cross-crate integration tests, external-consumer smoke tests, and launch-critical example smoke checks. Demo validation expanded across debug/release profiles with variance-reduction work to keep rankings resilient.

7. **Public teaching surface was made consistent and user-first.**
   README, user guide, docs index, demo docs, and crate landing pages were repeatedly synchronized so the same canonical onboarding path is taught everywhere, with clear language about what each demo proves and what it does not.

8. **Framework support matured without bloating core scope.**
   Axum adoption started as examples, then became first-class adapter support, and finally split into dedicated `tailtriage-axum` crate so `tailtriage-tokio` stayed framework-agnostic while preserving a practical integration path.

9. **Repository hygiene for public launch was finalized.**
   Contribution/security policies, license checks (`cargo deny` setup), metadata alignment, dependency/license tightening, and documentation polish completed the repo’s transition from private iteration to a launchable MVP toolchain.

### Current MVP state (condensed)

`tailtriage` now presents a coherent launchable MVP: a Tokio-focused triage toolkit with a unified integration API, deterministic and credibility-tiered demos, stable diagnosis artifacts, evidence-ranked suspects with next checks, and enforceable quality gates that keep the teaching surface and implementation aligned.
