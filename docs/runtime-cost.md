# Runtime-cost measurement

This page answers one question: what cost does each `tailtriage` integration mode add under the
repository's shared synthetic workload? For retention pressure rather than per-mode cost, use
[collector limits](collector-limits.md). For production capture decisions, use
[operations](operations.md).

## What is measured

`scripts/measure_runtime_cost.py` runs these categories as separate release-mode processes:

- baseline without tailtriage instrumentation;
- collector present without request-context calls;
- native core capture in light and investigation modes;
- each native mode with the Tokio sampler;
- intentionally saturated native drop paths;
- tracing light mode, with sampler, and with an intentionally saturated drop path.

The summary attributes baked-in cost, core-mode cost, incremental sampler cost, post-limit/drop-path
cost, and tracing-versus-native ratios. Separate processes allow process-global tracing subscriber
installation without contaminating later modes.

This is a synthetic measurement. Its results are machine, workload, and profile scoped; they are
not stable constants or universal production guarantees.

## Reproduce a representative measurement

```bash
python3 scripts/measure_runtime_cost.py
```

Defaults are 6,000 requests, concurrency 64, 3 ms work, two warmup rounds, and six measured rounds.
Override them with `--requests`, `--concurrency`, `--work-ms`, `--warmup-rounds`, and `--rounds`
(or the corresponding uppercase environment variables). `--print-json` prints the full summary in
addition to the compact table.

By default outputs are written under `demos/runtime_cost/artifacts/`:

- `runtime-cost-raw.jsonl` contains per-sample records;
- `runtime-cost-summary.json` contains aggregates, attribution, and measurement-quality labels.

Records include the instrumentation family, sampler/drop-path flags, retained evidence counts,
runtime snapshot count, finalization/analyze/render timings, lifecycle/sampler metadata, and the
artifact path. Missing metrics are `null`, not estimates.

## Bounded CI smoke

For an applicable code-changing pull request or `workflow_dispatch`, the operational job runs:

```bash
python3 scripts/measure_runtime_cost.py \
  --requests 4000 \
  --concurrency 32 \
  --work-ms 4 \
  --rounds 4 \
  --warmup-rounds 1 \
  --artifact-dir demos/runtime_cost/artifacts/ci-smoke
```

This bounded smoke applies the producer's tracing/native evidence checks and broad hard sanity
limits: tracing p95 must be at most 1.10 times native and tracing throughput at least 0.90 times
native. Ratios outside a 2% parity band (p95 above 1.02 or throughput below 0.98) warn while still
inside the hard limits. The CI smoke requests four measured rounds, matching the current minimum
for a stable-quality classification. `insufficient_data`, `noisy`, and `unstable` are
measurement-quality warnings themselves; separate producer sanity or tracing/native parity
violations can still make the command fail.

CI checks outputs in place and does not upload a durable runtime-cost artifact by default. The
deeper default measurement is manual/local. The smoke is regression-oriented evidence on the CI
machine, not full benchmark characterization.

## Interpret the output

- Compare **baked-in overhead** with baseline to isolate collector-present cost when request
  instrumentation is skipped.
- Compare **core mode overhead** to evaluate light versus investigation without sampling.
- Compare **incremental runtime sampler overhead** with the matching unsampled native mode.
- Interpret **post-limit/drop-path overhead** only for deliberately saturated cases, alongside drop
  counters and truncation.
- Treat small movements inside the 2% band as noise-compatible parity, not a reason to change the
  default native-integration recommendation.
- Rerun noisy results on a quieter machine before drawing a stronger conclusion.

`CaptureMode` does not auto-start runtime sampling. Tracing spans alone do not provide runtime
pressure evidence; tracing Tokio-session measurements need runtime snapshots. These semantics are
important to attribution but do not make this workload representative of every production service.

The concrete domain owner for runner/output mechanics is
[`validation/runtime-cost/README.md`](../validation/runtime-cost/README.md).

