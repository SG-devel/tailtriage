# Diagnostic validation scorecard (initial)

| Area | Status | Notes |
|---|---|---|
| queue saturation | Initial deterministic evidence | queue before/sample + mixed/cold-start/db-pool support. |
| downstream dominance | Initial deterministic evidence | downstream before/after/sample + retry/shared-lock scenarios. |
| db/pool wait | Partially validated | covered via db-pool scenario labels; broaden with non-demo corpora later. |
| blocking-pool pressure | Initial deterministic evidence | blocking before/sample coverage included. |
| executor pressure | Initial deterministic evidence | executor before/sample plus mixed coverage. |
| mixed bottlenecks | Partially validated | mixed baseline + synthetic ambiguity case. |
| insufficient evidence | Initial deterministic evidence | dedicated synthetic insufficient-evidence case. |
| truncation handling | Partially validated | synthetic truncation warning case; add real truncated captures later. |
| runtime overhead | Measured separately | see `docs/runtime-cost.md`. |
| collector limits | Measured separately | see `docs/collector-limits.md`. |
| real service validation | Planned | add curated real-service anonymized artifacts. |
