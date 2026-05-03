# Diagnostic validation scorecard (deterministic + adversarial)

| Area | Status | Notes |
|---|---|---|
| queue saturation | Initial deterministic evidence | queue before/sample + mixed/cold-start/db-pool support. |
| downstream dominance | Initial deterministic evidence | downstream before/after/sample + retry/shared-lock scenarios. |
| blocking-pool pressure | Initial deterministic evidence | blocking before/sample coverage included. |
| executor pressure | Initial deterministic evidence | executor before/sample plus mixed coverage. |
| low request count humility | Initial deterministic adversarial coverage | synthetic sparse-sample case enforces low-confidence insufficient-evidence output. |
| missing queue instrumentation | Initial deterministic adversarial coverage | synthetic case requires warning and queue-saturation visibility in top-2. |
| missing stage instrumentation | Initial deterministic adversarial coverage | synthetic case requires warning and downstream visibility in top-2. |
| missing runtime snapshots | Initial deterministic adversarial coverage | synthetic case requires warning and executor/blocking ambiguity in top-2. |
| missing optional runtime fields | Initial deterministic adversarial coverage | synthetic case checks partial-runtime warning and conservative confidence. |
| truncation/partial artifacts | Initial deterministic adversarial coverage | synthetic truncation case enforces warning and non-high confidence. |
| weak blocking vs strong downstream | Initial deterministic adversarial coverage | synthetic mixed case keeps downstream as top-1 with blocking as secondary. |
| blocking-correlated stage | Initial deterministic adversarial coverage | synthetic mixed case requires blocking top-1 with downstream in top-2. |
| noise-only insufficient evidence | Initial deterministic adversarial coverage | synthetic no-dominant-signal case enforces low-confidence insufficient-evidence. |
| high latency + missing instrumentation | Initial deterministic adversarial coverage | synthetic case prevents overconfident specific suspects with missing explanatory signals. |
| runtime overhead | Measured separately | see `docs/runtime-cost.md`. |
| collector limits | Measured separately | see `docs/collector-limits.md`. |
| real service validation | Planned | add curated real-service anonymized artifacts. |

Adversarial rows above are deterministic synthetic validation coverage, not real-service validation.
