# Collector stress methodology, findings, and operating guidance

This page documents the **collector-stress operating measurement path** for issue #107.

## Why this doc exists (and how it differs from `docs/runtime-cost.md`)

`docs/runtime-cost.md` explains a controlled runtime-overhead attribution path (baked-in/core/sampler/drop-path) on one shared scenario.

This page serves a different purpose: it records how we run the **collector stress path**, what outputs it produces, what trends were observed in a real run, and what those trends do and do not prove.

In short:

- `runtime-cost.md`: overhead attribution across fixed benchmark modes.
- `collector-limits.md` (this page): sustained-load operating behavior, retention/truncation behavior, artifact/memory growth, and sampler density behavior under stress-shaped event volume.

Use this distinction when choosing a measurement path:

- Runtime overhead attribution -> `runtime-cost.md`
- Sustained-load collector limits -> this page + `scripts/measure_collector_limits.py`
- Artifact-size scaling under stress-shaped event volume -> this page
- Memory-growth behavior under stress-shaped event volume -> this page

## Measurement path and methodology

The measured path is:

1. **Stress binary:** `demos/collector_stress` (release build).
2. **Orchestration script:** `scripts/measure_collector_limits.py`.
3. **Output artifacts:**
   - Raw JSONL: `demos/collector_stress/artifacts/collector-limits-<profile>-raw.jsonl`
   - Summary JSON: `demos/collector_stress/artifacts/collector-limits-<profile>-summary.json`

Treat this as the canonical collector-limits measurement path for issue #107.

### Workload shape and measured dimensions

The default matrix now includes an explicit pressure progression, plus two orthogonal stress checks:

1. `low_concurrency` (lower-pressure point)
2. `baseline_shape` (mid-pressure reference)
3. `high_concurrency` (higher-pressure point on the same shape)
4. `heavy_event_shape` (event-density change at mid concurrency)
5. `longer_run` (event-volume/duration change at mid concurrency)
6. `sampler_dense` (sampler-enabled modes only; cadence stress check)

This keeps one coherent measurement path while making onset/range interpretation practical.

Across supported modes:

- `baseline`
- `core_light`
- `core_investigation`
- `core_light_tokio_sampler`
- `core_investigation_tokio_sampler`

Each run reports (per row in raw JSONL):

- workload metadata (`mode`, `concurrency`, duration/request controls, `event_shape`, `sampler_settings`)
- throughput + latency (`throughput_rps`, `latency`)
- retention/truncation state (`retained_counts`, `truncation_counts`)
- artifact metadata (`artifact.artifact_path`, `artifact.artifact_size_bytes`, script-measured size)
- memory metadata (`peak_memory`, `memory_measurement`)
- notes (`measurement_notes`)

### Memory measurement method

The script tries external peak RSS first with `time -v` (`external_time_v`). If unavailable, it falls back to in-process memory fields (`in_process_fallback`) and records caveats in summary `measurement_quality.limitations`.

## What is measured and derived

From this path, we measure these dimensions directly:

1. **Concurrency progression:** `low_concurrency` -> `baseline_shape` -> `high_concurrency` with a fixed event shape.
2. **Event-density effect:** `baseline_shape` -> `heavy_event_shape`.
3. **Event-volume/duration effect:** `baseline_shape` -> `longer_run`.
4. **Runtime sampler density impact:** sampler baseline cadence vs `sampler_dense` override.
5. **Derived onset markers** in `collector_pressure_onset_markers`:
   - first case where `limits_hit_runs > 0`
   - first case where each dropped category becomes non-zero
   - first case where artifact growth crosses the configured threshold (currently +25% vs `baseline_shape`)
   - first case where memory growth crosses the configured threshold (currently +25% vs `baseline_shape`)

## Regime interpretation model (comfortable vs onset vs stressed)

Interpret each mode using the derived onset markers:

1. **Comfortable / unsaturated regime**
   - Typical signal: `first_limits_hit_case = null` and all `first_nonzero_dropped_case_by_category.* = null`.
   - Usually represented by `low_concurrency` when the host/workload can stay unsaturated there.

2. **Onset regime**
   - Typical signal: first non-null onset marker appears (for limits-hit, dropped categories, or growth threshold crossings).
   - This is the practical “pressure starts here” boundary for machine-scoped guidance.

3. **Clearly stressed regime**
   - Typical signal: multiple non-null onset markers and/or repeated limits-hit with persistent dropped counters across heavier cases (`high_concurrency`, `heavy_event_shape`, `longer_run`).
   - This regime is useful for confirming behavior under pressure, not for claiming universal operating limits.

## Machine-scoped example (April 20, 2026 run)

A bounded default run (`--profile default --modes core_light`) produced onset markers in summary output:

- `first_limits_hit_case = low_concurrency`
- first non-zero dropped category:
  - `dropped_inflight_snapshots = low_concurrency`
  - `dropped_requests/stages/queues = baseline_shape`
  - `dropped_runtime_snapshots = null`
- no +25% threshold crossing for artifact-size or peak-RSS growth in this bounded run.

Interpretation for this machine/run:

- the configured `low_concurrency=32` point is already in **onset/stressed** territory for core-light capture (not comfortable/unsaturated);
- onset signals are still useful because they localize which retained categories begin dropping earliest;
- this run supports practical onset-marker guidance, but does **not** provide a comfortable unsaturated bound for this mode on this host.

## What these results do **not** prove

These measurements do **not** prove:

- universal cross-machine performance properties
- production behavior outside this measured path and parameter set
- root cause certainty (they provide evidence-ranked stress signals)
- that one run’s absolute numbers should be reused as fixed guidance

## Practical operating guidance (grounded only in measured behavior)

Tie guidance to measured onset markers, not guesses:

1. Use `collector_pressure_onset_markers.per_mode[].first_limits_hit_case` as the first collector-pressure boundary.
2. Use `first_nonzero_dropped_case_by_category` to identify which retention category starts dropping first (`requests/stages/queues/inflight/runtime`).
3. Use growth-threshold markers to flag potential storage/memory pressure transitions; thresholds are intentionally conservative (+25%) and machine-scoped.
4. Treat `low_concurrency` as your practical “comfortable check,” `baseline_shape` as mid-point, and `high_concurrency`/`longer_run` as stress checks.
5. Compare **light vs investigation** with onset markers + artifact bytes + memory + dropped counters together, not throughput alone.
6. Use `sampler_dense` as an empirical, per-machine check; do not assume cadence impact direction without measured output.
7. Keep claims run-scoped and include profile, case IDs, repeat count, and memory path (`external_time_v` vs fallback).

Where data is insufficient (for example, broad sampler-cadence tradeoffs), state explicitly that more measured runs are needed.

## Commands

Default matrix:

```bash
python3 scripts/measure_collector_limits.py --profile default
```

Quick smoke matrix:

```bash
python3 scripts/measure_collector_limits.py --profile smoke
```

## Policy reminder

Do not hardcode one machine’s “latest numbers” as timeless truth. Keep claims tied to the measured path, emitted fields, and artifact files from the run being discussed.
