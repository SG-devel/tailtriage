# Runtime cost measurement

This document covers the reproducible local benchmark path for tailtriage runtime-cost triage.

## Modes

- `baseline`: no `tailtriage` instrumentation.
- `light`: request + queue + stage + inflight instrumentation.
- `investigation`: light mode + dense runtime sampling + an additional `pre_work_marker` stage sleep (`300 µs`) that models richer investigation profile depth.

`investigation` is intentionally **not** a pure collector toggle. Treat it as an investigation-profile cost measurement, not proof of isolated instrumentation overhead.

## Canonical command

```bash
python3 scripts/measure_runtime_cost.py
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

- `REQUESTS`
- `CONCURRENCY`
- `WORK_MS`
- `WARMUP_ROUNDS`
- `ROUNDS`

## How the benchmark is run

- Modes are sampled in interleaved rounds with rotating order.
- Warmup rounds run first and are excluded from overhead summaries.
- Overhead is computed from per-round paired deltas versus baseline (same round), then summarized.
- Output includes dispersion (mean/median/min/max/stdev/CV), not only means.

## Output files

Written to `demos/runtime_cost/artifacts/`:

- `runtime-cost-raw.jsonl`
  - Includes `round`, `phase`, and `is_warmup` metadata for each sample.
- `runtime-cost-summary.json`
  - Includes per-mode dispersion metrics.
  - Includes paired overhead deltas vs baseline.
  - Includes machine-readable measurement quality and optional stability warning reasons.
- Per-mode run JSON files for instrumented runs.

## Reading noisy-machine results

Normal laptops can be noisy due to thermal drift, scheduler contention, and background load.

- Prefer running on an otherwise idle machine.
- Treat results as indicative unless `measurement_quality` is `stable`.
- If the script reports `noisy` or `unstable`, rerun under quieter conditions before drawing strong conclusions.

## Policy

- Do not hardcode machine-specific “latest numbers” in docs.
- Cite either fresh script output or committed fixture snapshots when making overhead claims.
- Interpret results as evidence-ranked suspects for runtime cost triage, not proof of root cause.
