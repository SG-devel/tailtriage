# Collector limits and pressure measurement

This page answers one question: how do bounded retention, truncation, artifact size, and resource
signals behave as a synthetic collector workload increases? For per-mode runtime overhead, use
[runtime cost](runtime-cost.md). For production capture choices, use [operations](operations.md).

## What is measured

`scripts/measure_collector_limits.py` runs the `demos/collector_stress` workload and records:

- retained counts, `limits_hit`, and dropped counters by evidence family;
- truncation onset and dropped-category progression;
- throughput and latency;
- artifact-size growth and peak-memory trends;
- optional runtime-sampler density effects.

Profiles have different jobs:

- `smoke` is a quick bounded check;
- `default` provides the deeper reference progression;
- `artifact_scaling` focuses on bounded artifact-growth behavior.

## Reproduce the evidence

```bash
python3 scripts/measure_collector_limits.py --profile default
python3 scripts/measure_collector_limits.py --profile artifact_scaling
python3 scripts/measure_collector_limits.py --profile smoke
```

Outputs under the selected artifact directory include
`collector-limits-<profile>-raw.jsonl` and
`collector-limits-<profile>-summary.json`. The summary identifies the first case with limits hit,
the first non-zero drop for each category, and artifact-size or memory growth-threshold crossings.

## CI and manual boundary

For an applicable code-changing pull request or `workflow_dispatch`, the operational CI job runs:

```bash
python3 scripts/measure_collector_limits.py --profile smoke
```

That bounded smoke checks the runner and visible retention/truncation/drop behavior on the CI
machine. The deeper `default` and `artifact_scaling` characterizations are manual/local. Generated
outputs are checked in their selected directories and are not uploaded as durable CI artifacts by
default.

## Interpret onset and resource signals

1. `limits_hit_runs > 0` marks a case in which the capture was not fully retained.
2. Non-zero dropped counters identify which family—requests, stages, queues, in-flight, or
   runtime—reached its bound.
3. Unsaturated points are the clearest basis for interpreting artifact-size growth.
4. After retention caps saturate, artifact bytes can flatten or fall even while workload pressure
   grows. That is a cap effect, not evidence that load diminished.
5. Compare memory, artifact, throughput, and latency together and within the same profile.

Truncated retained evidence is partial. This path characterizes visibility of retention and drops;
analyzer tests and the deterministic corpus separately own warning, evidence-quality, and diagnosis
downgrade behavior for partial or truncated input.

## What the measurement does not prove

Results are synthetic and machine/workload/profile scoped. They do not establish a universal safe
operating range, universal artifact or memory thresholds, root-cause certainty, or "no drops."
Sampler-density results likewise describe only the selected workload and cadence.

The concrete domain owner for runner/output mechanics is
[`validation/collector-limits/README.md`](../validation/collector-limits/README.md).
