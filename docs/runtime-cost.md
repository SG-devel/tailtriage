# Runtime cost measurement

This document covers the reproducible local benchmark path for tailtriage runtime-cost triage.

## Modes

- `baseline`: no `tailtriage` instrumentation.
- `light`: request + queue + stage + inflight instrumentation.
- `investigation`: light mode + dense runtime sampling + an additional `pre_work_marker` stage sleep (`300 µs`) that models richer investigation profile depth.

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

The script builds `demos/runtime_cost` in **release mode** once, then executes the release binary directly for all warmup and measured rounds.

## Defaults and knobs

Defaults are selected to improve signal-to-noise on ordinary development machines while keeping runtime practical:

- `--requests` (default `6000`)
- `--concurrency` (default `64`)
- `--work-ms` (default `3`)
- `--warmup-rounds` (default `2`)
- `--rounds` (default `6`)

Equivalent environment variables are also supported:

- `REQUESTS` (default `6000`)
- `CONCURRENCY` (default `48`)
- `WORK_MS` (default `3`)
- `ROUNDS` (default `8`)
- `WARMUP_ROUNDS` (default `2`)
- `RUNTIME_COST_PROFILE` (`dev` or `release`, default `release`)

## How the benchmark is run

- Modes are sampled in interleaved rounds with rotating order.
- Warmup rounds run first and are excluded from overhead summaries.
- Overhead is computed from per-round paired deltas versus baseline (same round), then summarized.
- Output includes dispersion (mean/median/min/max/stdev/CV), not only means.

## Output files

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
