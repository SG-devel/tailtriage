# Runtime cost measurement

This document defines the reproducible runtime-cost measurement path for `tailscope` MVP modes.

## What is measured

For each mode (`baseline`, `light`, `investigation`) the harness reports:

- throughput (requests/second)
- end-to-end latency p50
- end-to-end latency p95
- end-to-end latency p99

The summary also computes relative overhead for `light` and `investigation` vs `baseline`.

## Harness

- Binary: `demos/runtime_cost`
- Canonical script: `python3 scripts/measure_runtime_cost.py`
- Compatibility wrapper: `scripts/measure_runtime_cost.sh`

The harness runs a queueing workload with bounded concurrency and fixed simulated work per request.

Mode behavior:

- `baseline`: no `tailscope` instrumentation
- `light`: request + queue + stage + inflight instrumentation
- `investigation`: same as light, plus an additional stage marker and dense Tokio runtime sampling

## Reproduce

From repo root:

```bash
python3 scripts/measure_runtime_cost.py
```

Optional tuning via environment variables:

- `REQUESTS` (default `1200`)
- `CONCURRENCY` (default `48`)
- `WORK_MS` (default `3`)
- `ITERATIONS` (default `5`)

Artifacts written to `demos/runtime_cost/artifacts/`:

- `runtime-cost-raw.jsonl`
- `runtime-cost-summary.json`
- per-mode run JSON files for instrumented modes

## Latest local run (2026-03-19)

The following values come from `demos/runtime_cost/artifacts/runtime-cost-summary.json` generated in this repository on **2026-03-19**:

| mode | throughput (req/s) | p50 (ms) | p95 (ms) | p99 (ms) |
|---|---:|---:|---:|---:|
| baseline | 10859.91 | 50.21 | 92.71 | 96.27 |
| light | 10607.71 | 52.23 | 97.41 | 101.07 |
| investigation | 7958.86 | 72.07 | 132.07 | 137.31 |

Relative overhead vs baseline:

- `light`: throughput **-2.32%**, p50 **+4.02%**, p95 **+5.08%**, p99 **+4.98%**
- `investigation`: throughput **-26.71%**, p50 **+43.54%**, p95 **+42.47%**, p99 **+42.64%**

## Notes and limits

- Results are workload- and machine-specific.
- These numbers are provided as an honest sample from one reproducible run path, not as universal guarantees.
- Future runtime-cost claims should cite fresh output from this script/harness.
