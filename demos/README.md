# Demo guide: scenarios

The demos are intentionally small services for Tokio tail-latency triage. They are designed to exercise diagnosis behavior with deterministic and reviewable artifacts, not universal causality proof.

## Brief introduction

Check out [`docs/getting-started-demo.md`](docs/getting-started-demo.md) for a short introduction to the demos and how to run them.

## Stronges public proof demos

### `queue_service`

**What it simulates**

- Requests compete for a limited semaphore (`worker_permit`).
- Baseline has tighter capacity and slower work; mitigated raises capacity and shortens work.

**What it proves well**

- One of the strongest demos.
- Clear and convincing proof of queue-dominant latency.
- Good flagship public demo for first-time readers.

**What it does not prove**

- It is not proof of every production queue topology or all burst regimes.

**Realistic vs synthetic**

- Realistic service shape, intentionally simplified and deterministic.

**Why it belongs**

- It is the cleanest entry point for understanding queue-saturation diagnosis.

**Inspect first in report**

- `primary_suspect.kind`
- `p95_queue_share_permille`
- queue-depth and queue-share suspect evidence

### `downstream_service`

**What it simulates**

- Request flow with a tiny local precheck and a consistently slower downstream stage.
- No intentional queue bottleneck.

**What it proves well**

- One of the strongest demos.
- Very clean stage-dominance story.
- Strong public proof case for downstream-led latency.

**What it does not prove**

- It does not model all real downstream stack complexity (fanout, retries, connection behavior).

**Realistic vs synthetic**

- Realistic diagnosis shape with deliberately simple mechanics.

**Why it belongs**

- Complements `queue_service` with an equally clean non-queue dominant case.

**Inspect first in report**

- `primary_suspect.kind`
- `p95_service_share_permille`
- downstream-stage suspect evidence
- before/after p95 and suspect-score movement in `before-after-comparison.json`

### `db_pool_saturation_service`

**What it simulates**

- Bounded DB-pool admission using a semaphore (`db_pool`).
- Separate `db_query` stage timing.
- Baseline shrinks pool and slows query stage; mitigated does the reverse.

**What it proves well**

- One of the best additional demos.
- Shows queue-like admission bottleneck and downstream stage time in one common service shape.
- Demonstrates mixed attribution within a single request path.

**What it does not prove**

- Still a synthetic model of DB pool saturation.
- Not proof of behavior under a real DB client/driver stack.

**Realistic vs synthetic**

- Realistic enough to be highly credible, but intentionally modeled.

**Why it belongs**

- Strong bridge between pure queue and pure downstream stories.

**Inspect first in report**

- queue evidence for `queue(..., "db_pool")`
- stage-share evidence for `stage(..., "db_query")`
- before/after p95 and primary suspect score

## Supporting pattern demos

These are valuable and should remain first-class docs, but are best after the core three demos.

### `shared_state_lock_service`

**What it simulates**

- Contention on a shared `tokio::sync::RwLock` write lock.
- Lock wait recorded as queue-like time on `shared_state_write_lock`.
- Critical-section work recorded separately as `shared_state_critical_section`.

**What it proves well**

- Conceptually strong example of non-obvious queue-like waits.
- Demonstrates that queue here includes lock admission waits, not only channels/semaphores.
- Preserves critical-section stage attribution while surfacing lock admission pressure.

**What it does not prove**

- Not proof that all lock-contention patterns map identically in every production design.

**Realistic vs synthetic**

- Realistic contention pattern with explicit instrumentation choices.

**Why it belongs**

- Teaches how to instrument and triage lock-heavy paths without semantic ambiguity.

**Inspect first in report**

- queue evidence for `shared_state_write_lock`
- stage evidence for `shared_state_critical_section`
- primary suspect kind and score changes in mitigation

### `retry_storm_service`

**What it simulates**

- Intermittently failing/slow downstream with explicit retries.
- Per-attempt stages (`downstream_attempt_N`) plus full-loop `downstream_total`.
- Mitigation changes retry count, backoff, jitter, and circuit-break-like cooldown behavior.

**What it proves well**

- One of the most product-interesting demos.
- Shows downstream dominance can come from retry policy, not just one slow call.
- Strong diagnosis-pattern demo for advanced readers.

**What it does not prove**

- More conceptually advanced and more instrumentation-shaped than core proof demos.

**Realistic vs synthetic**

- Plausible service behavior, but intentionally structured to isolate retry-policy effects.

**Why it belongs**

- Helps prevent misleading latency interpretation when retries dominate service share.

**Inspect first in report**

- `primary_suspect.kind`
- service-share evidence for `downstream_total`
- retry-policy-oriented `next_checks`

### `mixed_contention_service`

**What it simulates**

- Combined queue pressure (semaphore admission) and downstream slowness (periodic slow stage).
- Mitigation mainly reduces admission contention so rank/score can shift.

**What it proves well**

- Multiple suspects can coexist in one diagnosis.
- Useful supporting demo showing the analyzer is not single-cause-only.

**What it does not prove**

- Less crisp than queue-only or downstream-only stories, so not ideal as first public proof.

**Realistic vs synthetic**

- Realistic mixed-bottleneck shape with controlled deterministic contours.

**Why it belongs**

- Important second-wave demo for multi-factor triage interpretation.

**Inspect first in report**

- top two suspects and their evidence
- whether both queue and downstream leads remain visible
- before/after suspect-rank or score shift

### `cold_start_burst_service`

**What it simulates**

- Early cohort pays extra `cold_start_stage` delay while burst traffic competes for admission.
- Mitigation reduces cold cohort and increases admission capacity.

**What it proves well**

- Useful warmup-plus-burst explanation.
- Shows how stage-level pathology can induce queue effects.

**What it does not prove**

- Less universal than queue/downstream/DB-pool scenarios.
- Models cold start with explicit stage instrumentation, not full platform/framework startup behavior.

**Realistic vs synthetic**

- Plausible but scenario-specific.

**Why it belongs**

- Broadens triage examples beyond steady-state bottlenecks.

**Inspect first in report**

- evidence tied to `cold_start_stage`
- queue-share impact and p95 changes
- primary suspect score reduction after mitigation

## More synthetic analyzer-contract demos

These remain useful and should stay documented, but docs should treat them as more synthetic proofs.

### `blocking_service`

**What it simulates**

- Requests dispatch to `spawn_blocking` workloads.
- Baseline constrains `max_blocking_threads` and uses longer blocking work.
- Runtime snapshots include synthetic `blocking_queue_depth` signals.

**What it proves well**

- Directionally useful for exercising blocking-pool-pressure diagnosis behavior.

**What it does not prove**

- More synthetic than a strongest real-world proof case.

**Realistic vs synthetic**

- Intentionally synthetic analyzer-contract demo.

**Why it belongs**

- Keeps blocking-pressure diagnosis pathways directly testable.

**Inspect first in report**

- `primary_suspect.kind`
- blocking-related evidence and runtime depth signals
- mitigation impact on suspect score

### `executor_pressure_service`

**What it simulates**

- Fanout-heavy request handling with repeated CPU turns and frequent scheduling.
- Baseline uses fewer worker threads and heavier fanout.
- Runtime snapshots include runnable-depth signals.

**What it proves well**

- Useful for exercising executor-pressure diagnosis and rank behavior.

**What it does not prove**

- Also more synthetic, because runtime backlog signals are modeled more explicitly than production services naturally expose.

**Realistic vs synthetic**

- Intentionally synthetic analyzer-contract demo.

**Why it belongs**

- Preserves explicit coverage of executor-pressure diagnosis behavior.

**Inspect first in report**

- executor-pressure suspect evidence
- runnable queue-depth signals
- contrast with blocking-depth evidence
