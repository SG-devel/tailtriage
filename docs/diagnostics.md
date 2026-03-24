# Diagnostics guide

This document explains how `tailtriage analyze` produces a triage report and how to use it.

## Run artifact schema contract

`tailtriage-cli` requires every input run artifact to include a top-level `schema_version` integer. The current supported value is `1`.

Loader behavior is strict:

- missing `schema_version` is rejected
- non-integer `schema_version` is rejected
- unsupported `schema_version` is rejected

## Report contents

`tailtriage analyze <run.json>` outputs:

- request count
- request latency percentiles (p50/p95/p99)
- p95 per-request share percentiles:
  - `p95_queue_share_permille`
  - `p95_service_share_permille`
- optional in-flight trend summary
- optional truncation warnings when capture limits were hit
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
| `p95_queue_share_permille` | `Option<u64>` | 95th percentile of per-request queue-time share (0-1000). |
| `p95_service_share_permille` | `Option<u64>` | 95th percentile of per-request service-time share (0-1000). |
| `inflight_trend` | `Option<InflightTrend>` | Dominant in-flight gauge trend when snapshots exist. |
| `warnings` | `Vec<String>` | Analyzer warnings, including capture truncation context from run artifacts. |
| `primary_suspect` | `Suspect` | Highest-ranked suspect. |
| `secondary_suspects` | `Vec<Suspect>` | Remaining ranked suspects. |

The two p95 share fields are independent percentile summaries over different per-request series. They are not complementary totals and are not expected to sum to `1000`.

## Interpreting shares quickly

- `1000` permille = 100%
- `500` permille = 50%
- `p95_queue_share_permille` and `p95_service_share_permille` are each a percentile over their own distribution, so they should not be added together.

Rules of thumb:

- high queue share suggests queue saturation
- high service share plus dominant stage suggests downstream latency dominance
- use suspect evidence and next checks to choose one follow-up experiment

## Suspect kinds

- `application_queue_saturation`
- `blocking_pool_pressure`
- `executor_pressure_suspected`
- `downstream_stage_dominates`
- `insufficient_evidence`

These are **evidence-ranked leads**, not causal proof.

### Executor pressure vs blocking-pool pressure

- `executor_pressure_suspected` emphasizes runtime scheduler backlog signals (for example, elevated global/local runtime queue depth with in-flight growth).
- `blocking_pool_pressure` emphasizes `spawn_blocking` backlog signals (for example, elevated blocking queue depth evidence).
- If blocking queue depth remains low/absent while runtime queue depth rises, prefer executor-pressure next checks before blocking-pool tuning.

Runtime-signal availability caveat:

- On stable Tokio, `RuntimeSampler` always captures `alive_tasks` and `global_queue_depth`.
- `local_queue_depth`, `blocking_queue_depth`, and `remote_schedule_count` require `tokio_unstable` and are otherwise `None`.
- As a result, blocking-pool vs executor suspect separation may be weaker on stable builds; treat ranking as directional triage and prioritize follow-up checks.

## In-flight trend fields

When present:

- `gauge`
- `sample_count`
- `peak_count`
- `p95_count`
- `growth_delta`
- `growth_per_sec_milli`

Positive growth means in-flight work accumulated during the run.

## Truncation interpretation

If the run artifact has non-zero `truncation` counters, treat the report as diagnosis from partial data. Prioritize re-running with higher capture limits for truncated sections before ruling suspects in or out.

## Practical triage workflow

1. Capture one run.
2. Analyze and inspect primary suspect evidence.
3. Run the suggested next check for that suspect.
4. Change one thing.
5. Re-run under comparable load.
6. Compare p95 shares and suspect evidence.

For reproducible before/after demo workflows, see [getting-started-demo.md](getting-started-demo.md).
