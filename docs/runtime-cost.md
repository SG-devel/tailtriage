# Runtime cost measurement

This page describes the repository's runtime-overhead measurement path.

Use this path when you want overhead attribution across `tailtriage` integration modes on one shared synthetic workload shape.

For sustained stress/limits behavior instead, use [collector-limits.md](collector-limits.md).

## What this path measures

Measured categories:

- `baseline` (no tailtriage instrumentation)
- `baked_in_no_request_context` (collector initialized, request-context calls omitted)
- `core_light`
- `core_investigation`
- `core_light_tokio_sampler`
- `core_investigation_tokio_sampler`
- `core_light_drop_path` (intentionally saturated limits)
- `core_investigation_drop_path` (intentionally saturated limits)

Derived attribution sections in summary output:

- Baked-in overhead
- Core mode overhead
- Tokio mode overhead
- Incremental runtime sampler overhead
- Post-limit / drop-path overhead

## What this path does not measure

It does not provide:

- universal production guarantees
- cross-machine constants
- full collector-stress operating limits

Results are machine-scoped, workload-scoped synthetic measurements from this repository.

## Canonical command

```bash
python3 scripts/measure_runtime_cost.py
```

The script builds `demos/runtime_cost` in release mode and runs warmup + measured rounds.

## Inputs and knobs

CLI options (with equivalent env vars):

- `--requests` (`REQUESTS`, default `6000`)
- `--concurrency` (`CONCURRENCY`, default `64`)
- `--work-ms` (`WORK_MS`, default `3`)
- `--warmup-rounds` (`WARMUP_ROUNDS`, default `2`)
- `--rounds` (`ROUNDS`, default `6`)

## Artifacts emitted

Path: `demos/runtime_cost/artifacts/`

- `runtime-cost-raw.jsonl` (per-sample raw records)
- `runtime-cost-summary.json` (aggregates + overhead attribution + quality labels)

## Interpreting results

- Use **Baked-in overhead** to isolate collector-present cost when request instrumentation is skipped.
- Use **Core mode overhead** to compare light vs investigation without runtime sampler startup.
- Use **Incremental runtime sampler overhead** to isolate sampler contribution from same-mode core baselines.
- Use **Post-limit / drop-path overhead** only for intentionally saturated-limit behavior.

If `measurement_quality` reports noisy/unstable, rerun on a quieter machine state before drawing stronger conclusions.

## Semantics reminder

- `CaptureMode` changes retention defaults; it does not auto-start the runtime sampler.
- Post-limit overhead improvements come from cheaper drop-path handling after limits are hit, while preserving drop counters and truncation visibility.


## Operational validation runner

Use `scripts/run_operational_validation.py --domain runtime-cost` for manual/local runtime-cost validation records, summary JSON, and optional scorecard output under `target/operational-validation/`.

These outputs are machine/workload/profile scoped measurements. p95/p99 overhead ratios are measured outputs, not universal production guarantees. Missing metrics are emitted as `null` rather than guessed.
