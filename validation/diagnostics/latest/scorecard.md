# Diagnostic validation scorecard (initial)

| Area | Status | Notes |
|---|---|---|
| queue saturation | Validated in deterministic fixtures | queue before/sample + mixed/cold-start/db-pool support. |
| downstream dominance | Validated in deterministic fixtures | downstream before/after/sample + retry/shared-lock scenarios. |
| db/pool wait | Partially validated | covered via db-pool scenario labels; broaden with non-demo corpora later. |
| blocking-pool pressure | Validated in deterministic fixtures | blocking before/sample coverage included. |
| executor pressure | Validated in deterministic fixtures | executor before/sample plus mixed coverage. |
| mixed bottlenecks | Partially validated | mixed baseline + synthetic ambiguity case. |
| insufficient evidence | Validated in deterministic fixtures | dedicated synthetic insufficient-evidence case. |
| truncation handling | Partially validated | synthetic truncation warning case; add real truncated captures later. |
| runtime overhead | Measured separately | see `docs/runtime-cost.md`. |
| collector limits | Measured separately | see `docs/collector-limits.md`. |
| real service validation | Planned | add curated real-service anonymized artifacts. |
