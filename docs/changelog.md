# Changelog

## Unreleased

### Changed

- Split axum adoption helpers into a dedicated `tailtriage-axum` crate so `tailtriage-tokio` remains framework-agnostic.
- Migration for axum users: `tailtriage_tokio::axum::{middleware, TailtriageRequest}` → `tailtriage_axum::{middleware, TailtriageRequest}`.
- Made launch-facing docs user-first for public GitHub onboarding, with source/workspace as the primary path.
- Marked crates.io install/dependency snippets as post-publish guidance instead of launch-day defaults.
- Corrected demo/CI wording to match workflow coverage: all listed demos in dev+release except `executor` in release only.
- Demoted maintainer launch/readiness/ops docs from the main user onboarding path.
- Sharpened demo honesty language to distinguish strongest proof demos, supporting demos, and synthetic analyzer-contract demos.
