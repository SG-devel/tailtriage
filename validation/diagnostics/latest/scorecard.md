# Diagnostic validation scorecard (initial)

| Area | Status | Notes |
|---|---|---|
| queue saturation | Validated in deterministic fixtures | Demo-derived before/after queue cases in manifest. |
| downstream dominance | Validated in deterministic fixtures | Multiple service fixtures retain stage-dominance evidence. |
| db/pool wait | Partially validated | Covered by db-pool demo fixtures; broader pool variants planned. |
| blocking-pool pressure | Validated in deterministic fixtures | Blocking-before and sample fixtures plus warning handling. |
| executor pressure | Validated in deterministic fixtures | Executor-pressure fixtures plus runtime-gap synthetic warning case. |
| mixed bottlenecks | Partially validated | Mixed baseline/mitigated included; broader perturbations planned. |
| insufficient evidence | Validated in deterministic fixtures | Synthetic insufficient-evidence report case included. |
| truncation handling | Validated in deterministic fixtures | Synthetic truncation warning case included. |
| runtime overhead | Measured separately | See `docs/runtime-cost.md`; not part of this benchmark yet. |
| collector limits | Measured separately | See `docs/collector-limits.md`; not part of this benchmark yet. |
| real service validation | Planned | Needs external reproducible captures and repeated-run protocol. |
