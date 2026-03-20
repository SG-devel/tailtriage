# Diagnostics guide

This document explains what `tailtriage analyze` reports and how to use it.

## Report contents

`tailtriage analyze <run.json>` outputs:

- request count
- request latency percentiles (p50/p95/p99)
- p95 request-time shares:
  - `p95_queue_share_permille`
  - `p95_service_share_permille`
- optional in-flight trend summary
- ranked suspects (one primary, zero or more secondary)

Each suspect includes:

- `kind`
- `score`
- `confidence`
- `evidence[]`
- `next_checks[]`

## JSON fields (stable MVP shape)

| Field | Type | Meaning |
| --- | --- | --- |
| `request_count` | `usize` | Requests observed in the run. |
| `p50_latency_us` / `p95_latency_us` / `p99_latency_us` | `Option<u64>` | Request latency percentiles (microseconds). |
| `p95_queue_share_permille` | `Option<u64>` | Queue-time share of p95 request latency (0-1000). |
| `p95_service_share_permille` | `Option<u64>` | Service/stage share of p95 request latency (0-1000). |
| `inflight_trend` | `Option<InflightTrend>` | Dominant in-flight gauge trend when snapshots exist. |
| `primary_suspect` | `Suspect` | Highest-ranked suspect. |
| `secondary_suspects` | `Vec<Suspect>` | Remaining ranked suspects. |

## Interpreting shares quickly

- `1000` permille = 100%
- `500` permille = 50%

Rules of thumb:

- high queue share suggests queue saturation
- high service share plus dominant stage suggests downstream latency dominance
- use suspect evidence and next checks to choose one follow-up experiment

## Suspect kinds

- `ApplicationQueueSaturation`
- `BlockingPoolPressure`
- `ExecutorPressureSuspected`
- `DownstreamStageDominates`
- `InsufficientEvidence`

These are **evidence-ranked leads**, not causal proof.

## In-flight trend fields

When present:

- `gauge`
- `sample_count`
- `peak_count`
- `p95_count`
- `growth_delta`
- `growth_per_sec_milli`

Positive growth means in-flight work accumulated during the run.

## Practical workflow

1. Capture one run.
2. Analyze and inspect primary suspect evidence.
3. Change one thing.
4. Re-run under comparable load.
5. Compare p95 shares and suspect evidence.

For reproducible before/after demo workflows, see [getting-started-demo.md](getting-started-demo.md).
