# Diagnostic validation scorecard (deterministic)

| Area | Status | Notes |
|---|---|---|
| low request count | Initial deterministic adversarial coverage | synthetic sparse-sample case enforces low-confidence insufficient-evidence fallback. |
| missing queue instrumentation | Initial deterministic adversarial coverage | synthetic no-queue-events case requires warning and top-2 queue visibility. |
| missing stage instrumentation | Initial deterministic adversarial coverage | synthetic no-stage-events case requires warning and top-2 downstream visibility. |
| missing runtime snapshots | Initial deterministic adversarial coverage | synthetic runtime-missing case requires warning and bounded confidence. |
| missing optional runtime fields | Initial deterministic adversarial coverage | synthetic partial-runtime-fields case requires warning and conservative confidence. |
| truncation / partial artifacts | Initial deterministic adversarial coverage | synthetic truncation case requires warning, bounded confidence, and rerun next checks. |
| weak blocking vs strong downstream | Initial deterministic adversarial coverage | synthetic mixed-signal case keeps downstream as clear top-1 with blocking as secondary. |
| blocking-correlated stage | Initial deterministic adversarial coverage | synthetic mixed-signal case keeps blocking as top-1 with downstream visible in top-2. |
| noise-only workload | Initial deterministic adversarial coverage | synthetic low-signal case enforces insufficient-evidence + low confidence. |
| high latency + missing explanatory instrumentation | Initial deterministic adversarial coverage | synthetic case enforces insufficient-evidence with instrumentation-focused next checks. |
| real service validation | Planned | add curated real-service anonymized artifacts. |

These adversarial rows are deterministic synthetic validation coverage, not real service validation.
