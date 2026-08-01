# Prompt 20 executor-normalization evidence

Recommendation: **approve unchanged**.

Evidence-ranked projections are triage leads, not proof of root cause.

## Criteria

| # | Result | Derivation |
|---:|:---:|---|
| 1 | pass | source constants |
| 2 | pass | contribution boundaries |
| 3 | pass | exact scale invariance |
| 4 | pass | quantization rules |
| 5 | pass | monotonic contribution |
| 6 | pass | fixed absolute backlog |
| 7 | pass | integer p95 |
| 8 | pass | u128 domain |
| 9 | pass | legacy observables |
| 10 | pass | typed worker provenance |
| 11 | pass | missing-local projection |
| 12 | pass | independent cap composition |
| 13 | pass | competing controls |
| 14 | pass | complete extreme |
| 15 | pass | two byte comparisons |
| 16 | pass | allowed worktree |

## Competing controls

| Control | Current competitor | Normalized p95 | Cap trace | Ambiguity | Ordering | Match |
|---|---|---:|---|---|---|:---:|
| strong_blocking | blocking_pool_pressure | 250 | [] | ['blocking_pool_pressure'] | ['blocking_pool_pressure'] | True |
| downstream | downstream_stage_dominates | 500 | [] | ['downstream_stage_dominates'] | ['downstream_stage_dominates', 'executor_pressure'] | True |
| application_queue | application_queue_saturation | 250 | [] | ['application_queue_saturation'] | ['application_queue_saturation'] | True |
| sparse_runtime | None | 8000 | [{'cap': 'medium', 'before': 'high', 'after': 'medium', 'note': 'Sparse runtime evidence.'}] | ['downstream_stage_dominates'] | ['downstream_stage_dominates', 'executor_pressure'] | True |
| mixed_ambiguity | application_queue_saturation | 4000 | [{'cap': 'medium', 'before': 'medium', 'after': 'medium', 'note': 'Ambiguity cluster.'}] | ['executor_pressure', 'application_queue_saturation'] | ['application_queue_saturation', 'executor_pressure'] | True |
| complete_worker_extreme | None | 8000 | [] | ['executor_pressure'] | ['executor_pressure', 'application_queue_saturation'] | True |

## Derived comparisons
- Source truth and boundaries: True; 11/11.
- Scale groups: 4/4.
- Quantization cells: 25/25.
- Monotonic workers: 5/5; failures: 0.
- Fixed-backlog groups: 6/6.
- Legacy cases and paired checks: 17/17; 9/9.
- Worker provenance: 7/7.
- Missing-local: 4/4; failures: [].
- Cap composition: 5/5.

## Failed and questionable cases
- Failed: none.
- Questionable: Projected normalization is review evidence, not current analyzer behavior.
