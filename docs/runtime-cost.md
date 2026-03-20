# Runtime cost measurement

This is the reproducible overhead measurement path for MVP modes.

## Modes

- `baseline`: no `tailtriage` instrumentation
- `light`: request + queue + stage + inflight instrumentation
- `investigation`: light mode + extra stage marker + dense runtime sampling

## Metrics reported

- throughput (req/s)
- latency p50/p95/p99
- relative overhead vs baseline (for `light` and `investigation`)

## Canonical command

```bash
python3 scripts/measure_runtime_cost.py
```

Optional environment overrides:

- `REQUESTS` (default `1200`)
- `CONCURRENCY` (default `48`)
- `WORK_MS` (default `3`)
- `ITERATIONS` (default `5`)

## Outputs

Written to `demos/runtime_cost/artifacts/`:

- `runtime-cost-raw.jsonl`
- `runtime-cost-summary.json`
- per-mode run JSON files for instrumented modes

## Policy

- Do not hardcode machine-specific “latest numbers” in docs.
- Cite either fresh script output or committed fixture snapshots when making overhead claims.
