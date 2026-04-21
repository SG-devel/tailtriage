# Getting started with demos

Demos are deterministic triage exercises. They provide reproducible diagnosis behavior for this repository's scenarios, not universal causality proof.

## Recommended first demos

If you run three first, run:

- `queue_service`
- `downstream_service`
- `db_pool_saturation_service`

Scenario details: [demos/README.md](../demos/README.md)

## Additional useful demos

- `shared_state_lock_service`
- `retry_storm_service`
- `mixed_contention_service`
- `cold_start_burst_service`

Synthetic analyzer-contract demos:

- `blocking_service`
- `executor_pressure_service`

## Baseline diagnosis contract

| Scenario      | Expected baseline primary suspect | Required supporting signal                               |
| ------------- | --------------------------------- | -------------------------------------------------------- |
| `queue`       | `application_queue_saturation`    | Queue evidence on primary suspect                        |
| `downstream`  | `downstream_stage_dominates`      | Stage-dominance evidence on primary suspect              |
| `db-pool`     | `application_queue_saturation`    | Queue pressure on DB admission path                      |
| `shared-lock` | `application_queue_saturation`    | Queue wait/depth evidence from lock contention           |
| `retry-storm` | `downstream_stage_dominates`      | Elevated service-share evidence from retry-heavy stage   |
| `mixed`       | `application_queue_saturation`    | Downstream suspect also appears as secondary             |
| `blocking`    | `blocking_pool_pressure`          | Blocking queue depth evidence remains visible            |
| `cold-start`  | `application_queue_saturation`    | Evidence mentions `cold_start_stage` and/or queue impact |
| `executor`    | `executor_pressure_suspected`     | Runtime snapshot pressure + executor suspect score       |

## Run and validate

```bash
python3 scripts/demo_tool.py run queue
python3 scripts/demo_tool.py validate queue

python3 scripts/demo_tool.py run downstream
python3 scripts/demo_tool.py validate downstream

python3 scripts/demo_tool.py run db-pool
python3 scripts/demo_tool.py validate db-pool
```

Run any other scenario with the same pattern.

## Before/after comparison usage

Use fixture-backed before/after runs to evaluate one mitigation at a time:

- compare p95 movement
- compare suspect/evidence movement
- treat results as triage evidence for the next step

## Artifact policy

- `demos/*/artifacts/`: generated, untracked outputs
- `demos/*/fixtures/`: committed deterministic references
