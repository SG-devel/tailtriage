# Collector limits and stress guidance

This page describes the repository's sustained collector-stress measurement path.

Use this path when you want to understand truncation onset, dropped-category progression, artifact-size growth, and memory trends under stress-shaped synthetic workloads.

For runtime-overhead attribution across fixed modes, use [runtime-cost.md](runtime-cost.md).

## What this path measures

The path runs the `demos/collector_stress` workload matrix through `scripts/measure_collector_limits.py` and records:

- throughput and latency
- retained counts and truncation/drop counters
- artifact-size growth
- peak memory trends
- optional runtime sampler density effects

Profiles:

- `default` (reference progression)
- `artifact_scaling` (bounded scaling-focused progression)
- `smoke` (quick validation)

## What outputs it emits

Artifacts are written under `demos/collector_stress/artifacts/`:

- `collector-limits-<profile>-raw.jsonl`
- `collector-limits-<profile>-summary.json`

Summary output includes onset helpers such as:

- first case where limits are hit
- first case where each dropped category becomes non-zero
- growth-threshold crossings for artifact size and memory

## Interpreting onset and truncation signals

Treat these as practical warning markers in the measured matrix:

1. `limits_hit_runs > 0` means capture is no longer fully retained for that case.
2. non-zero dropped counters show which data category saturates first (`requests`, `stages`, `queues`, `inflight`, `runtime`).
3. once truncation is active, artifact bytes can flatten or invert because retained output is capped.

Interpret artifact-size trends most confidently on mostly-unsaturated points.

## Artifact-size and memory guidance (bounded claims)

- Use growth trends as machine-scoped operating guidance, not universal limits.
- Compare modes and cases with truncation context, not throughput alone.
- Treat memory and artifact thresholds as conservative local indicators for your current machine/workload shape.

## What this path does not prove

It does not prove:

- universal production behavior
- fixed safe operating ranges for all environments
- root cause certainty

Like runtime-cost data, these are synthetic, machine-scoped, workload-scoped measurements from this repository.

## Commands

Default profile:

```bash
python3 scripts/measure_collector_limits.py --profile default
```

Artifact-scaling profile:

```bash
python3 scripts/measure_collector_limits.py --profile artifact_scaling
```

Quick smoke profile:

```bash
python3 scripts/measure_collector_limits.py --profile smoke
```


## Operational collector-limit validation
Use `python3 scripts/run_operational_validation.py --domain collector-limits` for manual/local collector-limit validation. The claim is bounded and honest: drops are bounded, visible, and diagnosis is downgraded or warned appropriately when limits are reached. This does **not** claim the collector never drops. Expect visible drop counters, partial/truncation warnings, and downgrade/warning signals in analysis outputs.
