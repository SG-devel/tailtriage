# tailscope diagnostics guide (MVP)

This document explains what the analyzer currently reports and how to interpret it.

## Report shape

`tailscope analyze <run.json>` returns:

- request count
- request p50/p95/p99 latency
- p95 request-time share for queue wait (permille)
- p95 request-time share for service time (permille)
- optional dominant in-flight trend summary (`inflight_trend`)
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

## In-flight trend metrics

When in-flight snapshots are present, the report emits a dominant gauge summary:

- `inflight_trend.gauge`
- `inflight_trend.sample_count`
- `inflight_trend.peak_count`
- `inflight_trend.p95_count`
- `inflight_trend.growth_delta`
- `inflight_trend.growth_per_sec_milli` (count/sec in milli-units)

In text output, this summary is rendered on one explicit line:

- `inflight_trend gauge=<name> samples=<n> peak=<count> p95=<count> growth_delta=<delta> growth_per_sec_milli=<value>`

Interpretation guidance:

- positive `growth_delta` means in-flight work accumulated over the run window
- high `peak_count`/`p95_count` plus high queue share strengthens queue saturation suspicion
- positive growth plus elevated runtime global queue depth can reinforce executor pressure signals

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

### Queue demo before/after example

`scripts/run_queue_demo.sh` now emits `before` (pathological baseline) and `after` (mitigated) analyses, plus a machine-readable comparison JSON at `demos/queue_service/artifacts/before-after-comparison.json`.

In the current checked-in fixtures:

- `before-analysis.json` reports `ApplicationQueueSaturation` with score `90` and p95 latency `1,682,454us`
- `after-analysis.json` reports p95 latency `24,745us` with queue share reduced from `981` to `5` permille

Use this pattern for diagnosis validation: keep load shape constant, adjust one mitigation lever, and compare suspect ranking plus p95-level shares.

### Blocking demo before/after example

`scripts/run_blocking_demo.sh` now emits `before` (blocking-pool-constrained baseline) and `after` (mitigated) analyses, plus a machine-readable comparison JSON at `demos/blocking_service/artifacts/before-after-comparison.json`.

In the current checked-in fixtures:

- `before-analysis.json` reports `BlockingPoolPressure` with score `80`, p95 latency `3,524,739us`, and blocking queue depth p95 of `244`
- `after-analysis.json` reports p95 latency `82,559us` with the same suspect score but lower blocking queue depth p95 (`39`)

This demo validates mitigation by comparing both latency and at least one pressure signal (`blocking_queue_depth_p95`, suspect score, or p95 share) instead of relying on suspect kind changes alone.

## Limitations

- No cross-service correlation.
- Rule-based heuristics may miss mixed-cause incidents.
- Quality depends on instrumentation coverage.
- Runtime metrics are partially constrained on stable Tokio without `tokio_unstable`.
