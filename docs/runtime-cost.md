# Runtime cost measurement

This is the reproducible overhead measurement path for MVP modes.

## Modes

- `baseline`: no `tailtriage` instrumentation
- `light`: request + queue + stage + inflight instrumentation
- `investigation`: light mode + extra stage marker + dense runtime sampling

## Metrics reported

- throughput (req/s)
- latency p50/p95/p99
- paired overhead vs baseline (for `light` and `investigation`)
- spread metrics (mean/median/min/max/stdev/cv)
- stability signal (`stable` or `noisy`)

## Canonical command

```bash
python3 scripts/measure_runtime_cost.py                  # release profile default
python3 scripts/measure_runtime_cost.py --profile dev    # debug/dev comparison
```

Optional environment overrides:

- `REQUESTS` (default `6000`)
- `CONCURRENCY` (default `48`)
- `WORK_MS` (default `3`)
- `ROUNDS` (default `8`)
- `WARMUP_ROUNDS` (default `2`)
- `RUNTIME_COST_PROFILE` (`dev` or `release`, default `release`)

## Outputs

Written to `demos/runtime_cost/artifacts/`:

- `runtime-cost-raw.jsonl` (includes `round`, `phase`, `mode`, and `profile`)
- `runtime-cost-summary.json` (includes paired overhead and stability quality)
- per-mode run JSON files for instrumented modes

## Policy

- Do not hardcode machine-specific “latest numbers” in docs.
- Cite either fresh script output or committed fixture snapshots when making overhead claims.

## Interpretation notes

- `investigation` mode measures a richer investigation profile: light instrumentation plus dense runtime sampling and an additional marker stage in this demo workload.
- For production-like overhead triage, use `release`. Debug/dev runs are still useful for local development checks and relative behavior.
- Results can still be noisy on laptops; treat `noisy` stability output as a signal to rerun with more rounds or less machine contention.
