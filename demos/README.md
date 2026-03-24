# Demo guide: scenarios, realism tiers, and triage intent

The demos are intentionally small services for Tokio tail-latency triage. They are designed to exercise diagnosis behavior with deterministic, reviewable artifacts.

## First-time public path

If you only run three demos first, run:

```bash
python3 scripts/demo_tool.py validate queue
python3 scripts/demo_tool.py validate downstream
python3 scripts/demo_tool.py validate db-pool
```

## Demo honesty tiers

### Strongest public proof demos

- `queue_service`
- `downstream_service`
- `db_pool_saturation_service`

### Useful supporting demos

- `shared_state_lock_service`
- `retry_storm_service`
- `mixed_contention_service`
- `cold_start_burst_service`

### More synthetic analyzer-contract demos

- `blocking_service`
- `executor_pressure_service`

`blocking_service` and `executor_pressure_service` are valuable for diagnosis contract coverage but intentionally more synthetic than the strongest three demos.

## CI validation coverage (matches workflow)

CI validates all listed demos in `dev` and `release` **except** `executor`.

`executor` is validated in **release only**.

## Baseline diagnosis contract

| Scenario | Expected baseline primary suspect | Required supporting signal |
| --- | --- | --- |
| `queue` | `application_queue_saturation` | Queue evidence on primary suspect |
| `blocking` | `blocking_pool_pressure` | Blocking queue depth evidence remains visible |
| `executor` | `executor_pressure_suspected` | Runtime snapshot pressure + executor suspect score |
| `downstream` | `downstream_stage_dominates` | Stage-dominance evidence on primary suspect |
| `mixed` | `application_queue_saturation` | Downstream suspect also appears as secondary |
| `cold-start` | `application_queue_saturation` | Evidence mentions `cold_start_stage` and/or queue impact |
| `db-pool` | `application_queue_saturation` | Queue pressure on DB admission path |
| `shared-lock` | `application_queue_saturation` | Queue wait/depth evidence from lock contention |
| `retry-storm` | `downstream_stage_dominates` | Elevated service-share evidence from retry-heavy stage |

## Artifact policy

- `demos/*/artifacts/`: generated, untracked outputs
- `demos/*/fixtures/`: committed deterministic references

## Before/after comparison positioning

Use before/after fixtures as a reproducible mitigation comparison loop and confirmation aid. They provide practical evidence for whether a change helped in that scenario; they are not universal causal proof.

For scenario details and commands, see [`../docs/getting-started-demo.md`](../docs/getting-started-demo.md).
