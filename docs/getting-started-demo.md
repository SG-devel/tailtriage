# Getting started with demos

Demos provide deterministic triage exercises. They give reproducible evidence for diagnosis behavior, not universal causality proof.

## Demo tiers

See [`../demos/README.md`](../demos/README.md) for scenario details.

### Strongest public proof demos for first-time evaluation

If you only run three demos, run these three:

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

These are intentionally more synthetic and are best treated as contract exercises for suspect behavior.

## CI validation coverage

In `.github/workflows/ci.yml`, CI validates all demos, except `executor`, in **both** `dev` and `release` profiles. `executor` is validated in `release` only due to long runtime in `dev`.

## Before/after comparison guidance

Use fixture-backed before/after results as a reproducible mitigation comparison loop:

- compare one baseline run and one mitigated run
- inspect p95 movement and suspect/evidence movement
- treat it as evidence for the next decision, not proof of universal root cause

## Run + validate commands

```bash
python3 scripts/demo_tool.py run queue
python3 scripts/demo_tool.py validate queue

python3 scripts/demo_tool.py run downstream
python3 scripts/demo_tool.py validate downstream

python3 scripts/demo_tool.py run db-pool
python3 scripts/demo_tool.py validate db-pool

python3 scripts/demo_tool.py run shared-lock
python3 scripts/demo_tool.py validate shared-lock

python3 scripts/demo_tool.py run retry-storm
python3 scripts/demo_tool.py validate retry-storm

python3 scripts/demo_tool.py run mixed
python3 scripts/demo_tool.py validate mixed

python3 scripts/demo_tool.py run cold-start
python3 scripts/demo_tool.py validate cold-start

python3 scripts/demo_tool.py run blocking
python3 scripts/demo_tool.py validate blocking

python3 scripts/demo_tool.py run executor
python3 scripts/demo_tool.py validate executor --profile release
```
