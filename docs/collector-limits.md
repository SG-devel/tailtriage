# Collector-stress measurement path

This document describes the dedicated **collector-stress** path introduced for issue #107.

## Why this path exists

`runtime_cost` and collector-limits stress measurement answer different questions:

- `runtime_cost` is for compact, comparable **overhead attribution** across fixed modes on one shared scenario.
- `collector_limits` is for **sustained-load operating limits** under higher concurrency and denser per-request event shapes.

This separation keeps one coherent triage-oriented measurement path per question, rather than growing a generic benchmark framework.

## What it measures

The `collector_stress` binary and `scripts/measure_collector_limits.py` orchestrator focus on measured output for:

- collector behavior at higher concurrency than the runtime-cost path
- queue/stage/inflight-heavy request shapes
- runtime sampler density impact in sampler-enabled modes
- sustained run behavior over configurable duration (and optional request cap)
- retained event growth and drop/truncation counters
- run artifact size growth
- memory behavior using a preferred Linux peak-RSS path when available (`/usr/bin/time -v`) with explicit in-process fallback

Supported modes in this path:

- `baseline`
- `core_light`
- `core_investigation`
- `core_light_tokio_sampler`
- `core_investigation_tokio_sampler`

## Default matrix (documented, manageable)

The default orchestrator profile keeps one manageable matrix with named stress dimensions:

1. `baseline_shape`: reference shape across all modes
2. `high_concurrency`: higher concurrency with the same shape
3. `heavy_event_shape`: denser queue/stage/inflight event shape
4. `longer_run`: larger event volume through longer duration
5. `sampler_dense`: higher runtime sampler density for sampler-enabled modes only

For routine validation and CI, run `--profile smoke` (small bounded matrix) instead of the default profile.

## What it does not prove

- It does **not** prove root cause.
- It does **not** redefine `CaptureMode` semantics.
- It does **not** redesign collector internals.
- It is **not** a portability benchmark suite for all platforms.

Memory behavior is machine-scoped. If preferred external RSS measurement is unavailable, the summary records fallback behavior and caveats explicitly.

## Structured output model

Each binary run emits one JSON record with these fields:

- `mode`
- `concurrency`
- `duration_secs` and optional `request_limit`
- `event_shape` (`queues_per_request`, `stages_per_request`, `inflight_cycles_per_request`, `work_ms`/`work_us`)
- `sampler_settings`
- `throughput_rps`
- `latency` summary (`count`, `p50_ms`, `p95_ms`, `p99_ms`, `max_ms`)
- `retained_counts`
- `truncation_counts` (dropped counters + `limits_hit`)
- `artifact` (`artifact_path`, `artifact_size_bytes`)
- `peak_memory`
- `measurement_notes`

The orchestrator writes:

- raw JSONL per run, e.g. `demos/collector_stress/artifacts/collector-limits-default-raw.jsonl`
- summary JSON, e.g. `demos/collector_stress/artifacts/collector-limits-default-summary.json`

Summary sections include:

- absolute metrics (throughput/latency/request completion)
- artifact-size summaries (binary-reported and script-measured bytes)
- memory summaries (peak/end RSS and path usage)
- truncation/limits-hit context
- mode + sampler + event-shape metadata
- measurement-quality caveats and conservative interpretation notes
- derived stress signals (including sampler-density impact)

## Commands

Run matrix orchestration (default matrix):

```bash
python3 scripts/measure_collector_limits.py --profile default
```

Run bounded smoke validation matrix:

```bash
python3 scripts/measure_collector_limits.py --profile smoke
```

## Policy reminder

Keep claims conservative and based on measured output artifacts generated for the current machine/run. Do not hardcode machine-specific “latest numbers” into docs.
