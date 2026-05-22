# Runtime cost measurement

This page describes the repository's runtime-overhead measurement path.

Use this path when you want overhead attribution across `tailtriage` integration modes on one shared synthetic workload shape.

For sustained stress/limits behavior instead, use [collector-limits.md](collector-limits.md).
For production rollout and operations decisions that combine this data with capture-mode and troubleshooting guidance, see [operations.md](operations.md).

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
- `tracing_light`
- `tracing_light_tokio_sampler`
- `tracing_light_drop_path`

Derived attribution sections in summary output:

- Baked-in overhead
- Core mode overhead
- Tokio mode overhead
- Incremental runtime sampler overhead
- Post-limit / drop-path overhead
- tracing-vs-native ratio summaries

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
Each mode is executed as a separate process; this keeps process-global tracing subscriber installation valid for tracing modes.

## CI smoke policy

CI runs one bounded runtime-cost smoke on the Ubuntu extended release leg:

```bash
python3 scripts/measure_runtime_cost.py \
  --requests 4000 \
  --concurrency 32 \
  --work-ms 4 \
  --rounds 4 \
  --warmup-rounds 1 \
  --artifact-dir demos/runtime_cost/artifacts/ci-smoke
python3 scripts/validate_runtime_cost_summary.py \
  --raw demos/runtime_cost/artifacts/ci-smoke/runtime-cost-raw.jsonl \
  --summary demos/runtime_cost/artifacts/ci-smoke/runtime-cost-summary.json
```

This is a bounded diagnostic sanity check only. It enforces tracing/native parity hard checks (p95 <= 1.10x native and throughput >= 0.90x native), a 2% soft warning band (p95 > 1.02x or throughput < 0.98x), and required tracing evidence shape, not rigorous performance benchmarking. CI validates runtime-cost output in-place and does not upload runtime-cost artifacts by default. CI logs print compact runtime-cost tables by default, while full JSON remains in artifacts (`runtime-cost-summary.json`) and can be printed locally with `--print-json`. Full runtime-cost measurement remains a local/developer-run path via the canonical command above. Results remain machine/workload/profile scoped.

## Inputs and knobs

CLI options (with equivalent env vars):

- `--requests` (`REQUESTS`, default `6000`)
- `--concurrency` (`CONCURRENCY`, default `64`)
- `--work-ms` (`WORK_MS`, default `3`)
- `--warmup-rounds` (`WARMUP_ROUNDS`, default `2`)
- `--rounds` (`ROUNDS`, default `6`)
- `--print-json` (print full summary JSON after compact report)

## Artifacts emitted

Path: `demos/runtime_cost/artifacts/`

- `runtime-cost-raw.jsonl` (per-sample raw records)
- `runtime-cost-summary.json` (aggregates + overhead attribution + quality labels)

Per-mode records now include instrumentation family (`baseline` / `native` / `tracing`), runtime sampler/drop-path flags, run evidence counts, runtime snapshot counts, artifact finalization/analyze/render timings, sampler-metadata presence, inflight support, lifecycle warning count, and artifact path.

## Interpreting results

- Use **Baked-in overhead** to isolate collector-present cost when request instrumentation is skipped.
- Use **Core mode overhead** to compare light vs investigation without runtime sampler startup.
- Use **Incremental runtime sampler overhead** to isolate sampler contribution from same-mode core baselines.
- Use **Post-limit / drop-path overhead** only for intentionally saturated-limit behavior.

If `measurement_quality` reports noisy/unstable, CI reports warnings but does not fail solely for that quality classification; rerun on a quieter machine state before drawing stronger conclusions.
If `measurement_quality` is `insufficient_data` after expected measured rounds, CI fails.
Numbers are directional and machine/workload/profile scoped; this bounded smoke catches meaningful regressions, not rigorous benchmark conclusions.

## Semantics reminder

- `CaptureMode` changes retention defaults; it does not auto-start the runtime sampler.
- Tracing modes measure tailtriage semantic `tt.*` tracing spans (not OTel/OTLP export).
- Tracing spans alone do not imply runtime-pressure evidence; runtime-pressure evidence requires Tokio-session runtime snapshots.
- In tracing Tokio-session sampler mode, runtime snapshot retention is configured through shared core capture limits (`capture_limits_override`), not a tracing-only retention API.
- Post-limit overhead improvements come from cheaper drop-path handling after limits are hit, while preserving drop counters and truncation visibility.

## Operational validation runner

Use `python3 scripts/run_operational_validation.py --domain runtime-cost` for manual/local runtime-cost validation that emits JSONL records, stable summary JSON, and an optional scorecard. Results are machine/workload/profile scoped and should be treated as measurements, not universal guarantees. Missing metrics are emitted as `null` rather than guessed.


Native remains the default instrumentation path because it is direct, explicit, and complete. Tracing is a first-class intake bridge for teams already instrumented with tracing or preferring span-shaped instrumentation. Small wins/losses inside the 2% warning band are treated as parity, not as a reason to change the default recommendation.
