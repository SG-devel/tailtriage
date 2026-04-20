# Collector-stress measurement path

This document describes the dedicated **collector-stress** path introduced for issue #107.

## Why this path exists

`runtime_cost` and `collector_stress` answer different questions:

- `runtime_cost` is for compact, comparable **overhead attribution** across fixed modes on one shared scenario.
- `collector_stress` is for **sustained-load operating limits** under higher concurrency and denser per-request event shapes.

This separation keeps one coherent triage-oriented measurement path per question, rather than growing a generic benchmark framework.

## What it measures

The `collector_stress` binary and orchestrator focus on measured output for:

- collector behavior at higher concurrency than the runtime-cost path
- queue/stage/inflight-heavy request shapes
- runtime sampler density impact in sampler-enabled modes
- sustained run behavior over configurable duration (and optional request cap)
- retained event growth and drop/truncation counters
- run artifact size growth
- Linux process memory readings via `/proc/self/status` (`VmRSS`, `VmHWM`)

Supported modes in this path:

- `baseline`
- `core_light`
- `core_investigation`
- `core_light_tokio_sampler`
- `core_investigation_tokio_sampler`

## What it does not prove

- It does **not** prove root cause.
- It does **not** redefine `CaptureMode` semantics.
- It does **not** redesign collector internals.
- It is **not** a portability benchmark suite for all platforms.

Memory behavior is primarily supported on Linux through `/proc/self/status`. On non-Linux environments, memory fields are intentionally reported as unavailable with explicit notes.

## Structured output model

Each binary run emits one JSON record with these fields:

- `mode`
- `concurrency`
- `duration_secs` and optional `max_requests`
- `event_shape` (`queues_per_request`, `stages_per_request`, `inflight_transitions_per_request`, `work_ms`)
- `sampler_settings`
- `throughput_rps`
- `latency` summary (`count`, `p50_ms`, `p95_ms`, `p99_ms`, `max_ms`)
- `retained_events`
- `truncation` (dropped counters + `limits_hit`)
- `artifact` (`artifact_path`, `artifact_size_bytes`)
- `memory`
- `measurement_notes`

The Python orchestrator writes:

- `demos/collector_stress/artifacts/collector-stress-raw.jsonl`
- `demos/collector_stress/artifacts/collector-stress-summary.json`

## Commands

Run a single stress case directly:

```bash
cargo run --release --manifest-path demos/collector_stress/Cargo.toml -- \
  --mode core_light \
  --duration-secs 30 \
  --concurrency 256 \
  --queues-per-request 6 \
  --stages-per-request 4 \
  --inflight-transitions-per-request 6 \
  --work-ms 2
```

Run matrix orchestration and summary:

```bash
python3 scripts/measure_collector_stress.py
```

## Policy reminder

Keep claims conservative and based on measured output artifacts generated for the current machine/run. Do not hardcode machine-specific “latest numbers” into docs.
