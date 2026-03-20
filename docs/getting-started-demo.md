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

python3 scripts/demo_tool.py run downstream
python3 scripts/demo_tool.py validate downstream

python3 scripts/demo_tool.py run cold-start
python3 scripts/demo_tool.py validate cold-start
```

## What each demo demonstrates

| Demo | Expected emphasis | Key fields |
| --- | --- | --- |
| `queue_service` | application queueing pressure | `primary_suspect.kind`, `p95_queue_share_permille`, suspect evidence |
| `blocking_service` | blocking-pool pressure | `primary_suspect.kind`, blocking-related evidence, p95 shares |
| `downstream_service` | downstream-stage dominance | `primary_suspect.kind`, `p95_service_share_permille`, suspect evidence |
| `cold_start_burst_service` | cold-start burst triage (warmup + admission queueing) | `primary_suspect.kind`, `p95_*_share_permille`, warmup-stage suspect evidence |

## Cold-start demo interpretation

`cold_start_burst_service` includes two modes to compare diagnosis behavior:

- `before` / `baseline`: the first cohort of requests has a much slower `dependency_call` stage to simulate warmup under burst load.
- `after` / `mitigated`: warmup is pre-completed (no slow first cohort) and arrivals are more staggered.

Expected interpretation:

1. baseline report should rank either queue impact or warmup-driven service dominance as the leading suspect.
2. mitigated report should show lower p95 latency and a lower primary suspect score.
3. this is triage evidence: the suspect indicates likely pressure points, not proven root cause.

## If local results differ from fixtures

1. rerun on an otherwise idle machine
2. confirm script success first
3. compare fixture JSONs before interpreting local artifact drift
