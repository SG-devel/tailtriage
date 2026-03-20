# Getting started with demos

Use demos to validate diagnosis behavior with deterministic fixtures.

For a scenario-by-scenario explanation of what each demo simulates, what triage it is intended to exercise, and what setup is shared, see **[`demos/README.md`](../demos/README.md)**.

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

python3 scripts/demo_tool.py run cold-start
python3 scripts/demo_tool.py validate cold-start
```

## What each demo demonstrates

| Demo | Expected emphasis | Key fields |
| --- | --- | --- |
| `queue_service` | application queueing pressure | `primary_suspect.kind`, `p95_queue_share_permille`, suspect evidence |
| `blocking_service` | blocking-pool pressure | `primary_suspect.kind`, blocking-related evidence, p95 shares |
| `executor_pressure_service` | executor pressure / runnable backlog | `primary_suspect.kind`, runtime queue-depth evidence, low blocking-depth evidence |
| `downstream_service` | downstream-stage dominance | `primary_suspect.kind`, `p95_service_share_permille`, suspect evidence |
| `mixed_contention_service` | queue + downstream contention together | baseline includes both suspects; mitigation should shift rank and/or score |
| `cold_start_burst_service` | cold-start cohort causes warmup drag and burst queueing | baseline evidence references `cold_start_stage` and/or queue pressure; mitigation lowers p95 and primary suspect score |

## Mixed-contention expected rank behavior

- Baseline profile intentionally keeps both contention sources visible in report evidence:
  - application queue saturation from semaphore worker limits
  - downstream-stage latency from a deterministic slow-stage ratio
- One of these is expected to be the primary suspect, and the other should appear in the ranked suspects list.
- Mitigation profile reduces one bottleneck (worker-limit queueing), and validation expects a rank/score shift in the primary suspect.

## Cold-start burst expected interpretation

- `before` intentionally sends a burst while an initial cohort pays a larger `cold_start_stage` delay.
- Diagnosis should rank either queue saturation or downstream-stage dominance as the top suspect and include evidence tied to warmup stage share and/or queue impact.
- `after` applies a mitigated profile (smaller cold cohort + staggered startup + more admission capacity), and validation expects:
  - lower `p95_latency_us`
  - lower primary suspect score

## If local results differ from fixtures

1. rerun on an otherwise idle machine
2. confirm script success first
3. compare fixture JSONs before interpreting local artifact drift
