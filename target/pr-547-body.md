## Summary

Final merge-readiness cleanup pass for tracing intake/docs parity without expanding scope.

This change tightens wording and public guidance after the tracing import fixes, shared writer path, feature gating, unified capture config, temporal containment checks, and parity validation landed.

## What changed

### Public command/API wording cleanup
- Removed stale mention of `tracing-subscriber-fmt-json` as a supported `--input-format` value.
- Kept `fmt().json` mentions only as non-supported ordinary tracing-log JSON context.
- Removed stale/contradictory references to tracing-specific retention knobs in favor of the shared core capture model.
- Ensured public docs consistently describe tracing intake as a narrow bridge into `tailtriage_core::Run`.

### Coherent tracing workflow story
- Native capture remains the default recommendation.
- Tracing intake remains first-class for correctly instrumented completed `tt.*` spans.
- Offline import writes **Run JSON** first; analysis is a separate step.
- Direct tracing sessions also write **Run JSON** via the same robust writer/finalization path as native capture.
- Persisted Run artifacts intended for CLI analysis require at least one request event.
- Library snapshots can still be zero-request for in-process inspection.
- Runtime-pressure evidence still requires runtime snapshots / Tokio sampler coupling.
- Explicitly no OTel/OTLP scope expansion and no analyzer semantic rewrite.

### Shared capture configuration and retention parity
- Tracing import/native capture are documented as sharing `CaptureMode` + `CaptureLimits` semantics for request/stage/queue retention.
- Tokio tracing session runtime snapshot retention is documented through the same core model (`mode`, `capture_limits`, `capture_limits_override`), with no tracing-only runtime-retention builder.
- Kept parity framing bounded to triage consistency checks, not root-cause proof.

### Validation claims and parity gates
- Validation docs remain bounded and truthful:
  - temporal containment validation for imported stage/queue spans,
  - CI-gated tracing/native parity,
  - CI-gated tiny-limit retention parity,
  - feature-gated live tracing dependencies,
  - machine/workload/profile scope for runtime-cost outputs.

## Supported CLI tracing import formats

`tailtriage import tracing-json ... --input-format <value>` supports:
- `auto`
- `tailtriage-span-jsonl`

`tailtriage-span-jsonl` enforces wrapper-only parsing of:

```json
{"format":"tailtriage.tracing-span.v1","span":{...}}
```

Ordinary `tracing_subscriber::fmt().json()` log output is intentionally rejected.

## Scope / non-claims
- No OTel/OTLP intake or exporter behavior added.
- No observability-backend scope added.
- No analyzer behavior rewrite.
- Suspects remain evidence-ranked triage leads, not proof of root cause.
