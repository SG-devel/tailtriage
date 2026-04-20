# Collector stress methodology, findings, and operating guidance

This page documents the **collector-stress operating measurement path** for issue #107.

## Why this doc exists (and how it differs from `docs/runtime-cost.md`)

`docs/runtime-cost.md` explains a controlled runtime-overhead attribution path (baked-in/core/sampler/drop-path) on one shared scenario.

This page serves a different purpose: it records how we run the **collector stress path**, what outputs it produces, what trends were observed in a real run, and what those trends do and do not prove.

In short:

- `runtime-cost.md`: overhead attribution across fixed benchmark modes.
- `collector-limits.md` (this page): sustained-load operating behavior, retention/truncation behavior, artifact/memory growth, and sampler density behavior under stress-shaped event volume.

## Measurement path and methodology

The measured path is:

1. **Stress binary:** `demos/collector_stress` (release build).
2. **Orchestration script:** `scripts/measure_collector_limits.py`.
3. **Output artifacts:**
   - Raw JSONL: `demos/collector_stress/artifacts/collector-limits-<profile>-raw.jsonl`
   - Summary JSON: `demos/collector_stress/artifacts/collector-limits-<profile>-summary.json`

### Workload shape and measured dimensions

The default matrix in the script executes five named cases:

1. `baseline_shape`
2. `high_concurrency`
3. `heavy_event_shape`
4. `longer_run`
5. `sampler_dense` (sampler-enabled modes only)

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

## What was measured

From this path, we measure these dimensions directly:

1. **Contention behavior:** `baseline_shape` -> `high_concurrency` deltas.
2. **Sustained-load behavior:** `baseline_shape` -> `longer_run` for throughput/latency/memory/truncation.
3. **Artifact-size scaling:** `baseline_shape` -> `heavy_event_shape` by mode.
4. **Memory growth:** peak/end RSS fields, including case deltas.
5. **Runtime sampler density impact:** sampler baseline cadence vs `sampler_dense` override.

## What was observed (machine-scoped run on April 20, 2026)

The following observations are from a real `--profile default` run output written to:

- `demos/collector_stress/artifacts/collector-limits-default-raw.jsonl`
- `demos/collector_stress/artifacts/collector-limits-default-summary.json`

Treat these as run-local evidence, not timeless constants.

### 1) Contention behavior

In this run, moving from `baseline_shape` to `high_concurrency` increased throughput substantially in every mode (~+92% to +99%), while p95 latency rose moderately (~+2% to +8%).

So this particular run did **not** show a throughput-collapse breakpoint under the configured `high_concurrency` step.

### 2) Sustained-load behavior (`longer_run`)

- Throughput remained near baseline-shape levels across instrumented modes.
- Longer runs increased dropped counters materially in instrumented modes (especially dropped inflight snapshots and, in light modes, dropped requests/stages/queues).
- `limits_hit_runs` was consistently set for instrumented modes in baseline/high/heavy/longer cases.

This indicates pressure is visible through truncation counters before/while artifacts saturate retention limits.

### 3) Artifact-size scaling by event shape

In this specific run, `heavy_event_shape` did **not** increase artifact bytes versus `baseline_shape`; summary `artifact_growth_heavy_event_shape_pct` was negative for instrumented modes.

That means this run does not support a claim of universal monotonic artifact growth from denser shape settings once truncation is already active.

### 4) Memory growth by event volume

- Baseline mode (no artifact writing) had very low RSS compared with instrumented modes.
- Investigation-family modes showed much larger peak RSS than light-family modes.
- `longer_run` increased peak RSS noticeably in investigation-family modes (~+48% to +49% in this run’s summary signals), while light-family growth was small.

Because `time -v` was unavailable on this host, this run used in-process fallback memory fields.

### 5) Runtime sampler density impact

`sampler_dense` compared against sampler baselines produced only small changes in this run (close to flat, with slight improvement in one sampler mode and slight latency increase in the other).

Result: this run does not support a broad claim that denser sampler cadence always worsens throughput/latency.

### 6) Light vs investigation (where relevant)

Observed in this run:

- Investigation-family artifacts and memory were higher than light-family.
- Both families hit truncation limits under the configured stress matrix.
- Investigation-family dropped inflight snapshots even when dropped requests/stages/queues were zero in some cases.

### 7) Truncation / operating-limit behavior

The operating-limit signal in this run is explicit:

- `truncation_counts.limits_hit=true` across instrumented mode/case combinations in default profile.
- Non-zero dropped counters persist while runs still complete and produce analyzable output.

This is the practical signal for “collector pressure is active” in this measurement path.

## What these results do **not** prove

These measurements do **not** prove:

- universal cross-machine performance properties
- production behavior outside this measured path and parameter set
- root cause certainty (they provide evidence-ranked stress signals)
- that one run’s absolute numbers should be reused as fixed guidance

## Practical operating guidance (grounded only in measured behavior)

Based on observed output fields and this run’s behavior:

1. Watch `truncation_counts` first when running collector stress; treat `limits_hit` and dropped counters as primary operating-limit signals.
2. Compare **light vs investigation** using artifact bytes + memory + dropped counters together, not throughput alone.
3. Use `sampler_dense` as an empirical check per machine; do not assume cadence changes are always costly.
4. Keep claims run-scoped and include profile, case shape, and memory path (`external_time_v` vs fallback).
5. If you need stronger conclusions, run repeats (`--repeats`) and compare distributions instead of one-shot values.

Where data is insufficient (for example, broad sampler-cadence tradeoffs), state explicitly that more measured runs are needed.

## Follow-up implications and issue drafts

I cannot create GitHub issues from this environment, so here are focused drafts based on observed bottleneck/limit signals:

### Draft 1: Improve collector-limit pressure visibility in summary

**Title:** collector-limits: add explicit per-category saturation onset markers

**Body:**
- Problem: current summary reports dropped totals and limits-hit counts, but not the first point of saturation per category.
- Proposal: add derived fields for first-observed saturation markers by category (`requests/stages/queues/inflight/runtime`) per case/mode.
- Why: helps triage when pressure starts, not just that it happened.
- Scope: summary-only derived metadata; no capture semantics changes.

### Draft 2: Add repeated-run profile for collector-limits default matrix

**Title:** collector-limits: add documented `--repeats` guidance and sample comparison helper

**Body:**
- Problem: one-shot runs are noisy and can invert expected trends (e.g., heavy-event artifact deltas under active truncation).
- Proposal: document and optionally script a small repeated-run aggregation/report helper for default profile.
- Why: improves confidence in trend statements without introducing universal-number claims.
- Scope: scripts/docs only; no runtime behavior changes.

### Draft 3: Optional host-memory probe enhancement

**Title:** collector-limits: improve memory-path portability when `/usr/bin/time -v` is unavailable

**Body:**
- Problem: current host lacked compatible `time -v`, forcing in-process fallback memory fields.
- Proposal: add optional Linux `/proc` peak tracking path (guarded and clearly labeled) as another explicit measurement path.
- Why: better host portability while preserving explicit caveats.
- Scope: measurement script enhancement; keep current fallback and caveat behavior.

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
