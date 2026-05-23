## Summary

Final merge-readiness cleanup pass for tracing intake docs/examples/public wording after tracing import fixes, writer reuse, feature gating, unified config, temporal validation, and parity validation.

This change does **not** redesign the feature and does **not** change analyzer semantics.

## Public contract (cleaned and aligned)

- Native capture remains the default path.
- Tracing is a first-class **intake bridge** for correctly instrumented completed `tt.*` spans.
- Offline tracing import is a two-step flow:
  1) `tailtriage import tracing-json ...` writes **Run JSON**
  2) `tailtriage analyze ...` runs analysis separately.
- Direct tracing sessions (`run_json_path(...)`) write Run JSON through the same robust writer path used by native capture sinks.
- Tracing import/native capture share `CaptureMode`/`CaptureLimits` retention semantics for request/stage/queue evidence.
- Persisted Run JSON artifacts intended for CLI analysis require at least one request event.
- In-process/library snapshots may still be zero-request for inspection.
- Runtime-pressure evidence still requires runtime snapshots/Tokio sampler coupling.

## CLI tracing import formats

Supported `--input-format` values remain:

- `tailtriage-span-jsonl` (wrapper-only stable format)
- `auto` (compatibility parser for older normalized shapes with early rejection guidance for unsupported ordinary tracing logs)

Stale claims about supporting ordinary `tracing_subscriber::fmt().json()` logs as import artifacts were removed; that format remains unsupported as an intake artifact.

## Scope boundaries reiterated

- No OTel/OTLP support added.
- No observability backend added.
- No analyzer rewrite.
- Suspects remain evidence-ranked triage leads, not proof of root cause.

## Validation and parity coverage reflected

Docs now consistently reflect existing validation coverage including:

- temporal containment validation for imported stage/queue spans,
- feature-gated live tracing dependencies,
- CI-gated tracing/native parity checks,
- CI-gated tiny-limit retention parity checks.

## Examples/docs cleanup

- Removed stale or contradictory wording around unsupported formats and retention knobs.
- Kept shared capture-mode/limit guidance consistent across top-level docs and crate READMEs.
- Kept examples aligned with request/stage/queue `tt.*` field conventions and supported CLI import formats.
