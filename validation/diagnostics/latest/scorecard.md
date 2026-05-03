# Diagnostic validation scorecard (initial)

| Area | Status | Notes |
|---|---|---|
| queue saturation | Validated in deterministic fixtures | before/after + sample corpus coverage |
| downstream dominance | Validated in deterministic fixtures | before/after + sample corpus coverage |
| db/pool wait | Validated in deterministic fixtures | db-pool before/after labeled as queue-family admission pressure |
| blocking-pool pressure | Validated in deterministic fixtures | blocking before/after + sample |
| executor pressure | Validated in deterministic fixtures | executor sample/before/after |
| mixed bottlenecks | Partially validated | mixed baseline/mitigated + synthetic weak ambiguity |
| insufficient evidence | Validated in deterministic fixtures | synthetic insufficient-evidence case |
| truncation handling | Partially validated | synthetic truncation-warning case |
| runtime overhead | Measured separately | see docs/runtime-cost.md |
| collector limits | Measured separately | see docs/collector-limits.md |
| real service validation | Planned | future external workload corpus |
