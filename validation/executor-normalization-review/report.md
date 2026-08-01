# Prompt 20 executor-normalization evidence

Recommendation: **approve unchanged**.

This evidence-only draft derives evidence-ranked review findings; projections are not proof of root cause.

## Criteria

| # | Result | Derivation |
|---:|:---:|---|
| 1 | pass | source constants and boundaries |
| 2 | pass | direct contributions |
| 3 | pass | exact worker scale projection |
| 4 | pass | quantization table retained |
| 5 | pass | monotonic depth |
| 6 | pass | fixed backlog nonincreasing |
| 7 | pass | integer p95 indexes |
| 8 | pass | u128 domain |
| 9 | pass | legacy public comparisons |
| 10 | pass | worker classifications |
| 11 | pass | missing-local classifications |
| 12 | pass | cap projections |
| 13 | pass | first five controls |
| 14 | pass | complete extreme |
| 15 | pass | recorded two-run cmp |
| 16 | pass | recorded clean-tree checks |

## Integer percentile indexes

- count 1: index 0, value 0
- count 2: index 1, value 1
- count 3: index 2, value 2
- count 19: index 18, value 18
- count 20: index 19, value 19
- count 21: index 19, value 19
- count 39: index 37, value 37
- count 40: index 38, value 38
- count 41: index 38, value 38
- count 99: index 94, value 94
- count 100: index 95, value 95
- count 101: index 95, value 95

## Derived comparisons
- Legacy cases: 15; matching: 15.
- Worker modes: 7/7.
- Missing-local cases: 4/4.
- Cap projections: 5/5.

## Competing controls
- strong_blocking: primary=None, false High executor primary=false; suspects=[]
- downstream: primary=executor_pressure, false High executor primary=false; suspects=[('executor_pressure', 62, 'low')]
- application_queue: primary=None, false High executor primary=false; suspects=[]
- sparse_runtime: primary=executor_pressure, false High executor primary=false; suspects=[('executor_pressure', 89, 'medium')]
- mixed_ambiguity: primary=executor_pressure, false High executor primary=false; suspects=[('executor_pressure', 79, 'medium')]
- complete_worker_extreme: primary=executor_pressure, false High executor primary=false; suspects=[('executor_pressure', 97, 'high')]

## Overflow
- Maximum intermediate width: 75 bits; all fit u128=True.

## Failed and questionable cases
- Failed: none.
- Questionable: Projected normalization remains review evidence and is not current analyzer behavior.
