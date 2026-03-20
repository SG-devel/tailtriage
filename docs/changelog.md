# Changelog

## Unreleased

### Changed

- Consolidated documentation structure around `docs/README.md` as the canonical docs index.
- Reduced duplication across README/user guide/diagnostics/demo docs while keeping MVP integration and diagnosis guidance intact.
- Simplified the historical MVP audit doc and removed stale cross-document references.
- Simplified `tailscope-cli` dependencies by removing a direct `tailscope-tokio` dependency; the CLI only consumes `tailscope-core` analyzer APIs.
