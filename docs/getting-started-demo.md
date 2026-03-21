# Getting started with demos

Use demos to validate diagnosis behavior with deterministic fixtures.

For a scenario-by-scenario explanation of what each demo simulates, what triage it is intended to exercise, and what setup is shared, see **[`demos/README.md`](../demos/README.md)**.

## Recommended public progression

To evaluate `tailtriage` quickly and honestly, run demos in this order:

1. `queue_service`
2. `downstream_service`
3. `db_pool_saturation_service`
4. `shared_state_lock_service`
5. `retry_storm_service`
6. `mixed_contention_service`
7. `cold_start_burst_service`
8. `blocking_service`
9. `executor_pressure_service`

This order reflects explanatory clarity and public credibility, not implementation completeness.

## Demo tiers and what they are for

### Core public proof demos

These are the strongest public proof cases:

- `queue_service`
- `downstream_service`
- `db_pool_saturation_service`

### Supporting pattern demos

These are valuable second-wave demos:

- `shared_state_lock_service`
- `retry_storm_service`
- `mixed_contention_service`
- `cold_start_burst_service`

### More synthetic analyzer-contract demos

These remain useful, but are more synthetic than the first two tiers:

- `blocking_service`
- `executor_pressure_service`

## Artifact policy

- `demos/*/artifacts/`: generated, untracked local outputs.
- `demos/*/fixtures/`: committed reference snapshots.

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
python3 scripts/demo_tool.py validate executor
```

## Quick interpretation map by demo

Use this table for first-pass diagnosis reading. Suspects are leads, not proof.

| Demo | What it proves well | What it does not prove | First report fields to inspect |
| --- | --- | --- | --- |
| `queue_service` | Clear, convincing queue-dominant latency story; one of the strongest flagship demos. | Not a proof of every real queue topology under production burst behavior. | `primary_suspect.kind`, `p95_queue_share_permille`, queue-depth evidence. |
| `downstream_service` | Very clean stage-dominance story; one of the strongest public proof cases. | Not a full model of all downstream path complexity. | `primary_suspect.kind`, `p95_service_share_permille`, downstream stage evidence. |
| `db_pool_saturation_service` | Strong split between DB admission wait (`db_pool`) and DB stage time (`db_query`) in one common service shape. | Still a synthetic DB-pool model, not proof under a real DB driver/client stack. | Queue wait evidence for `db_pool` plus stage-share evidence for `db_query`. |
| `shared_state_lock_service` | Strong lock-contention framing: lock admission wait as queue-like pressure plus separate critical-section stage attribution. | Not a claim that all lock contention behaves identically across real services. | Queue evidence for `shared_state_write_lock` and stage evidence for `shared_state_critical_section`. |
| `retry_storm_service` | Product-interesting diagnosis pattern: downstream dominance from retry policy, not only one slow call. | More advanced and instrumentation-shaped than core proof demos. | `primary_suspect.kind`, `downstream_total` service-share evidence, retry-policy next checks. |
| `mixed_contention_service` | Shows multiple suspects can coexist and ranking can shift after mitigation. | Less crisp than queue-only or downstream-only proof cases. | Top two suspects, evidence mix across queue and downstream, before/after rank shift. |
| `cold_start_burst_service` | Useful warmup-plus-burst pattern with both stage and admission effects. | Less universal than queue/downstream/DB-pool scenarios. | Evidence tied to `cold_start_stage`, queue-share impact, before/after p95 change. |
| `blocking_service` | Directionally useful for exercising blocking-pool diagnosis behavior. | More synthetic than a strongest real-world proof case. | Blocking-pressure suspect evidence and blocking-related runtime signals. |
| `executor_pressure_service` | Useful for exercising executor-pressure diagnosis and runnable-backlog evidence. | More synthetic because backlog signals are modeled explicitly. | Executor-pressure suspect evidence, runtime queue-depth signals, blocking-depth contrast. |

## CI validation coverage

The documented demo surface matches the CI validation surface. In `.github/workflows/ci.yml`, the `CI` workflow validates:

- `queue`
- `downstream`
- `db-pool`
- `shared-lock`
- `retry-storm`
- `mixed`
- `cold-start`
- `blocking`
- `executor`

## Runtime-cost demo path (separate from triage scenarios)

`runtime_cost` is a measurement demo that uses a separate script entrypoint rather than `scripts/demo_tool.py`.

Use:

```bash
python3 scripts/measure_runtime_cost.py
```

For mode definitions, metrics, and interpretation details, see **[`docs/runtime-cost.md`](./runtime-cost.md)**.

## Demo fixture drift guard and refresh workflow

`python3 scripts/check_demo_fixture_drift.py` regenerates demo analysis outputs and fails if committed fixtures are stale.

When analyzer output changes intentionally, refresh fixtures with:

```bash
python3 scripts/check_demo_fixture_drift.py --refresh
```

Then review the fixture diffs, commit them, and re-run the drift guard to confirm the refresh is complete.

## If local results differ from fixtures

1. rerun on an otherwise idle machine
2. confirm script success first
3. compare fixture JSONs before interpreting local artifact drift
