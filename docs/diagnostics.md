# tailscope diagnostics guide (MVP)

This document explains what the analyzer currently reports and how to interpret it.

## Report shape

`tailscope analyze <run.json>` returns:

- request count
- request p50/p95/p99 latency
- p95 request-time share for queue wait (permille)
- p95 request-time share for service time (permille)
- one primary suspect
- zero or more secondary suspects

Each suspect includes:

- kind
- score
- confidence
- evidence (human-readable)
- recommended next checks

## Request-time share metrics

The report includes two explicit request-time share fields:

- `p95_queue_share_permille`
- `p95_service_share_permille`

Both are measured in permille (0-1000) across requests, where:

- `1000` = 100.0% of request time
- `500` = 50.0% of request time

Interpretation guidance:

- high `p95_queue_share_permille` (for example 300+ = 30%+) points to application-level queueing pressure
- high `p95_service_share_permille` with a dominant stage points to downstream/service-time bottlenecks
- queue + service shares are complementary at request level in current MVP heuristics (queue wait is clamped to request latency)

## Suspect kinds

## `ApplicationQueueSaturation`

Typical evidence:

- queue wait consumes large request share (p95 queue share high)
- queue depth samples rise

What to check next:

- admission limits, producer burstiness
- worker parallelism and queue policies

## `BlockingPoolPressure`

Typical evidence:

- blocking queue depth p95 above zero/sustained
- tails align with `spawn_blocking` backlog

What to check next:

- blocking callsites and workload duration
- synchronous CPU or blocking I/O in hot paths

## `ExecutorPressureSuspected`

Typical evidence:

- runtime global queue depth p95 elevated
- broad scheduler pressure signal

What to check next:

- long-running polls and missing yields
- uneven fan-out / task scheduling behavior

## `DownstreamStageDominates`

Typical evidence:

- one stage dominates p95 and cumulative latency

What to check next:

- dependency behind that stage
- retries/timeouts/circuit behavior under load

## `InsufficientEvidence`

Typical evidence:

- sparse queue/stage/runtime signals
- weak or conflicting attribution data

What to check next:

- add targeted queue/stage instrumentation
- capture runtime snapshots during the workload window

## Confidence model

Current confidence mapping is score-based:

- `high` for strong scores
- `medium` for moderate scores
- `low` otherwise

Confidence is an internal ranking confidence, not a statistical confidence interval.

## Practical workflow

1. Run workload and capture one run JSON.
2. Analyze with CLI (`text` and `json` when needed).
3. Start from the primary suspect evidence.
4. Validate suspect with one targeted experiment.
5. Re-run and compare before/after output.

## Limitations

- No cross-service correlation.
- Rule-based heuristics may miss mixed-cause incidents.
- Quality depends on instrumentation coverage.
- Runtime metrics are partially constrained on stable Tokio without `tokio_unstable`.
