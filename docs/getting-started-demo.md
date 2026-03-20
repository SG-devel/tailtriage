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
```

## What each demo demonstrates

| Demo | Expected emphasis | Key fields |
| --- | --- | --- |
| `queue_service` | application queueing pressure | `primary_suspect.kind`, `p95_queue_share_permille`, suspect evidence |
| `blocking_service` | blocking-pool pressure | `primary_suspect.kind`, blocking-related evidence, p95 shares |
| `executor_pressure_service` | executor pressure / runnable backlog | `primary_suspect.kind`, runtime queue-depth evidence, low blocking-depth evidence |
| `downstream_service` | downstream-stage dominance | `primary_suspect.kind`, `p95_service_share_permille`, suspect evidence |

## If local results differ from fixtures

1. rerun on an otherwise idle machine
2. confirm script success first
3. compare fixture JSONs before interpreting local artifact drift
