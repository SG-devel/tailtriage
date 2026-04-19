# Diagnostics guide

This guide explains how `tailtriage analyze` turns one run artifact into a triage report.

## How to read one report in 30 seconds

1. Check `primary_suspect.kind`.
2. Read `primary_suspect.evidence[]`.
3. Read `primary_suspect.next_checks[]`.
4. Use `p95_queue_share_permille` and `p95_service_share_permille` as directional context.
5. Change one thing, rerun, and compare.

Ranking is rule-based and directional. Suspects are evidence-ranked leads, not proof of root cause.

## Run artifact schema contract

`tailtriage-cli` requires a top-level `schema_version` integer in every input artifact. Current supported value: `1`.

Loader behavior is strict:

- missing `schema_version` is rejected
- non-integer `schema_version` is rejected
- unsupported `schema_version` is rejected

Mode/config metadata in artifacts:

- `metadata.mode` stores the selected core capture mode.
- `metadata.effective_core_config` stores resolved core settings used for the run.
- `metadata.effective_tokio_sampler_config` stores resolved Tokio sampler settings when `RuntimeSampler` was started.

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

`p95_queue_share_permille` and `p95_service_share_permille` are independent percentiles over different per-request series, so they are not expected to sum to `1000`.

## Suspect kinds

- `application_queue_saturation`
- `blocking_pool_pressure`
- `executor_pressure_suspected`
- `downstream_stage_dominates`
- `insufficient_evidence`

## Executor pressure vs blocking-pool pressure

- `executor_pressure_suspected` emphasizes runtime scheduler backlog signals.
- `blocking_pool_pressure` emphasizes `spawn_blocking` backlog signals.
- If blocking queue depth stays low/absent while runtime queue depth rises, prioritize executor-focused next checks first.

Runtime-signal caveat:

- On stable Tokio, `RuntimeSampler` always captures `alive_tasks` and `global_queue_depth`.
- `local_queue_depth`, `blocking_queue_depth`, and `remote_schedule_count` require `tokio_unstable` and are otherwise `None`.
- Separation between blocking-pool and executor suspects can therefore be weaker on stable builds.

## In-flight trend fields

When present:

- `gauge`
- `sample_count`
- `peak_count`
- `p95_count`
- `growth_delta`
- `growth_per_sec_milli`

Positive growth indicates in-flight work accumulated during the run.

## Truncation interpretation

If artifact `truncation` counters are non-zero, treat the diagnosis as partial-data triage and rerun with higher capture limits before drawing stronger conclusions.

## Practical triage loop

1. Capture one run.
2. Analyze and inspect primary suspect evidence.
3. Run one recommended next check.
4. Change one thing.
5. Rerun with comparable load.
6. Compare suspect ranking and p95 shares.
