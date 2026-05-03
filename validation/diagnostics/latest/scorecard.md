# Diagnostic validation scorecard

Status labels describe deterministic corpus coverage only. Synthetic adversarial rows validate report/benchmark humility behavior and are not real-service validation.

| Area | Status | Notes |
|---|---|---|
| queue saturation | Initial deterministic evidence | Demo fixtures plus synthetic missing-stage coverage. |
| downstream dominance | Initial deterministic evidence | Demo fixtures plus weak-blocking/strong-downstream adversarial coverage. |
| blocking-pool pressure | Initial deterministic evidence | Demo fixtures plus blocking-correlated-stage adversarial coverage. |
| executor pressure | Initial deterministic evidence | Demo fixtures plus missing-runtime and partial-runtime-field adversarial coverage. |
| low request count | Initial deterministic adversarial coverage | Synthetic sparse-sample case enforces insufficient-evidence + low confidence ceiling. |
| missing queue/stage/runtime instrumentation | Initial deterministic adversarial coverage | Synthetic no-queue, no-stage, and no-runtime cases enforce warnings and conservative confidence. |
| missing optional runtime fields | Initial deterministic adversarial coverage | Synthetic partial-runtime-fields case enforces warning and medium confidence ceiling. |
| truncation / partial artifact | Initial deterministic adversarial coverage | Synthetic truncated artifact case enforces warning, follow-up checks, and confidence ceiling. |
| noise-only / insufficient evidence | Initial deterministic adversarial coverage | Synthetic noise-only and high-latency-with-missing-instrumentation cases enforce low-confidence insufficient-evidence fallback. |
| mixed-signal top-2 behavior | Initial deterministic adversarial coverage | Synthetic mixed cases enforce true-cause visibility in required top-2. |
| runtime overhead | Measured separately | See `docs/runtime-cost.md`. |
| collector limits | Measured separately | See `docs/collector-limits.md`. |
| real service validation | Planned | Add curated anonymized real-service artifacts in future work. |
