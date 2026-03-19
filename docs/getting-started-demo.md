# Getting started with demos

Use this page for a fast first run of each MVP demo, then jump to the deeper policy and validation references.

- Artifact policy (tracked fixtures vs generated artifacts): see the README Demos section, [Artifact policy (tracked vs generated)](../README.md#artifact-policy-tracked-vs-generated).
- Validation workflow details: use the `scripts/validate_*_demo.py` commands described in the README demo subsections.

## Queue service demo (`demos/queue_service`)

**Scenario:** The service starts with heavy application-level queueing, then shows an improved run with queue pressure reduced.

**Run command sequence:**

```bash
python3 scripts/run_queue_demo.py && python3 scripts/validate_queue_demo.py
```

**Artifacts appear in:**

- Generated outputs: `demos/queue_service/artifacts/`
- Checked-in reference fixtures: `demos/queue_service/fixtures/`

**Expected analyzer fields (and why they matter):**

- `primary_suspect.kind`: should indicate queue saturation in the slow run, which tells you to inspect admission/queue behavior first.
- `p95_queue_share_permille`: should be high before and much lower after, showing whether queue waiting dominates p95 latency.
- `request_latency_us.p95`: should drop meaningfully in the improved run, confirming user-visible tail-latency improvement.

**If your output differs:**

If suspect ranking or p95 queue share does not move as expected, rerun on an idle machine and compare against `demos/queue_service/fixtures/*-analysis.json` to confirm you are evaluating the same baseline.

## Blocking service demo (`demos/blocking_service`)

**Scenario:** The service exhibits blocking-pool contention before mitigation, then demonstrates improved behavior with reduced blocking pressure.

**Run command sequence:**

```bash
python3 scripts/run_blocking_demo.py && python3 scripts/validate_blocking_demo.py
```

**Artifacts appear in:**

- Generated outputs: `demos/blocking_service/artifacts/`
- Checked-in reference fixtures: `demos/blocking_service/fixtures/`

**Expected analyzer fields (and why they matter):**

- `primary_suspect.kind`: should remain focused on blocking-pool pressure, indicating the dominant bottleneck class.
- `runtime_pressure.blocking_queue_depth.p95`: should decrease in the improved run, showing reduced blocking-pool backlog.
- `request_latency_us.p95`: should trend down after mitigation, confirming latency impact from lowering blocking contention.

**If your output differs:**

If blocking depth or p95 latency does not improve, ensure no other CPU-heavy tasks are running and rerun the demo before comparing to `demos/blocking_service/fixtures/*-analysis.json`.

## Downstream service demo (`demos/downstream_service`)

**Scenario:** The service latency is dominated by a slow downstream stage rather than local queueing or blocking backlog.

**Run command sequence:**

```bash
python3 scripts/run_downstream_demo.py && python3 scripts/validate_downstream_demo.py
```

**Artifacts appear in:**

- Generated outputs: `demos/downstream_service/artifacts/`
- Checked-in reference fixture: `demos/downstream_service/fixtures/`

**Expected analyzer fields (and why they matter):**

- `primary_suspect.kind`: should indicate downstream-stage dominance, narrowing investigation to external/service-stage dependencies.
- `dominant_stage.name`: should identify the slow stage, giving you a concrete target for follow-up instrumentation or dependency checks.
- `dominant_stage.p95_share_permille`: should be high, proving most p95 time is spent in that stage rather than in queue wait.

**If your output differs:**

If the dominant stage is missing or the suspect changes, verify the demo completed successfully and compare with `demos/downstream_service/fixtures/sample-analysis.json` before troubleshooting local environment effects.
