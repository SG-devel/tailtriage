# Runtime cost measurement

This document covers the reproducible local benchmark path for tailtriage runtime-cost triage.

## Scenario family

All measurements use the same shared request scenario (same request shape, concurrency model, and simulated work) so comparisons stay interpretable.

## Modes and attribution

The runtime-cost demo benchmarks these categories:

- `baseline`: no `tailtriage` instrumentation.
- `core_light`: `tailtriage-core` in `CaptureMode::Light`, no Tokio sampler.
- `core_investigation`: `tailtriage-core` in `CaptureMode::Investigation`, no Tokio sampler.
- `core_light_tokio_sampler`: core light plus `RuntimeSampler` (Tokio-mode defaults inherited from light).
- `core_investigation_tokio_sampler`: core investigation plus `RuntimeSampler` (Tokio-mode defaults inherited from investigation).
- `core_light_drop_path`: core light with intentionally tiny capture limits to exercise post-limit drop behavior.

Important attribution rules for this benchmark:

- Core mode overhead is measured without sampler startup.
- Tokio sampler overhead is measured in sampler-enabled modes, not attributed to core-only modes.
- Investigation mode in this demo does not add synthetic stage sleeps or extra fake work.

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
- Overhead is computed from per-round paired deltas.
- Output includes dispersion (mean/median/min/max/stdev/CV), not only means.

## Output files

Written to `demos/runtime_cost/artifacts/`:

- `runtime-cost-raw.jsonl`
  - Includes `round`, `phase`, and `is_warmup` metadata for each sample.
  - Includes per-run truncation/drop counters for instrumented modes.
- `runtime-cost-summary.json`
  - Includes absolute metrics for each mode.
  - Includes explicit deltas from baseline under these headings:
    - `Core mode overhead`
    - `Tokio mode overhead`
    - `Post-limit / drop-path overhead`
  - Includes explicit incremental sampler deltas under:
    - `Incremental runtime sampler overhead`
  - The current matrix does **not** include a distinct baked-in / near-no-op mode, so no separate `Baked-in overhead` section is reported.
  - Includes machine-readable measurement quality and optional stability warning reasons.
  - Includes sample-count context (`measured_rounds`, `samples_per_mode`, and minimum rounds required for `stable`).

## Reading noisy-machine results

Normal laptops can be noisy due to thermal drift, scheduler contention, and background load.

- Prefer running on an otherwise idle machine.
- Treat results as indicative unless `measurement_quality` is `stable`.
- The script requires at least 4 measured rounds before it can classify a run as `stable`; lower counts are reported as `insufficient_data`.
- If the script reports `noisy` or `unstable`, rerun under quieter conditions before drawing strong conclusions.

## Policy

- Do not hardcode machine-specific “latest numbers” in docs.
- Cite either fresh script output or committed fixture snapshots when making overhead claims.
- Interpret results as evidence-ranked suspects for runtime cost triage, not proof of root cause.
