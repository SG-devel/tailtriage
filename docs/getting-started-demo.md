# Getting started with demos

Use demos to validate diagnosis behavior with deterministic fixtures.

## Artifact policy

- `demos/*/artifacts/`: generated, untracked local outputs.
- `demos/*/fixtures/`: committed reference snapshots.

## Run + validate commands

```bash
python3 scripts/demo_tool.py run queue
python3 scripts/demo_tool.py validate queue

python3 scripts/demo_tool.py run blocking
python3 scripts/demo_tool.py validate blocking

python3 scripts/demo_tool.py run executor
python3 scripts/demo_tool.py validate executor

python3 scripts/demo_tool.py run downstream
python3 scripts/demo_tool.py validate downstream

python3 scripts/demo_tool.py run mixed
python3 scripts/demo_tool.py validate mixed
```

## What each demo demonstrates

| Demo | Expected emphasis | Key fields |
| --- | --- | --- |
| `queue_service` | application queueing pressure | `primary_suspect.kind`, `p95_queue_share_permille`, suspect evidence |
| `blocking_service` | blocking-pool pressure | `primary_suspect.kind`, blocking-related evidence, p95 shares |
| `executor_pressure_service` | executor pressure / runnable backlog | `primary_suspect.kind`, runtime queue-depth evidence, low blocking-depth evidence |
| `downstream_service` | downstream-stage dominance | `primary_suspect.kind`, `p95_service_share_permille`, suspect evidence |
| `mixed_contention_service` | queue + downstream contention together | baseline includes both suspects; mitigation should shift rank and/or score |

## Mixed-contention expected rank behavior

- Baseline profile intentionally keeps both contention sources visible in report evidence:
  - application queue saturation from semaphore worker limits
  - downstream-stage latency from a deterministic slow-stage ratio
- One of these is expected to be the primary suspect, and the other should appear in the ranked suspects list.
- Mitigation profile reduces one bottleneck (worker-limit queueing), and validation expects a rank/score shift in the primary suspect.

## If local results differ from fixtures

1. rerun on an otherwise idle machine
2. confirm script success first
3. compare fixture JSONs before interpreting local artifact drift
