# Diagnostics guide

This guide explains how `tailtriage analyze` turns one run artifact into a triage report.

## Read one report quickly

1. Check `primary_suspect.kind`.
2. Read `primary_suspect.evidence[]`.
3. Read `primary_suspect.next_checks[]`.
4. Use p95 share fields as directional context.
5. Run one targeted check, then re-run and compare.

Ranking is rule-based triage guidance. Suspects are leads, not proof of root cause.

## Artifact schema contract

`tailtriage-cli` requires top-level `schema_version`.

- missing `schema_version` is rejected
- non-integer `schema_version` is rejected
- unsupported `schema_version` is rejected

Current supported schema version: `1`.

## Report contents

`tailtriage analyze <run.json>` outputs:

- request count
- request latency percentiles (`p50`, `p95`, `p99`)
- p95 queue/service share summaries
- optional in-flight trend summary
- warnings (analyzer/report warnings, especially truncation-related)
- ranked suspects (primary + secondary)

Each suspect includes:

- `kind`
- `score`
- `confidence`
- `evidence[]`
- `next_checks[]`

`p95_queue_share_permille` and `p95_service_share_permille` are independent percentile summaries and do not need to sum to `1000`.

## Field reference (stable report shape)

- `request_count`: number of requests observed in the run artifact.
- `p50_latency_us` / `p95_latency_us` / `p99_latency_us`: request latency percentiles in microseconds.
- `p95_queue_share_permille`: p95 queue-time share per request (0..1000 scale).
- `p95_service_share_permille`: p95 service-time share per request (0..1000 scale).
- `warnings[]`: analyzer/report warnings, especially truncation-related warnings from captured-data limits. Loader/lifecycle warnings (including unfinished-request warnings) are emitted separately by the CLI loader to stderr before the report output.
- `primary_suspect`: highest-ranked suspect with evidence and next checks.
- `secondary_suspects[]`: additional ranked suspects.
- `inflight_trend` (optional): dominant in-flight gauge trend summary when snapshots exist.

## Suspect kinds

- `application_queue_saturation`
- `blocking_pool_pressure`
- `executor_pressure_suspected`
- `downstream_stage_dominates`
- `insufficient_evidence`

## Runtime-pressure caveat

On stable Tokio, runtime snapshots always include `alive_tasks` and `global_queue_depth`.
Fields such as `local_queue_depth`, `blocking_queue_depth`, and `remote_schedule_count` require `tokio_unstable` and may be `None`.

That can reduce separation confidence between blocking-pool and executor suspects.

## Truncation interpretation

If truncation counters are non-zero, treat the diagnosis as partial-data triage. Increase limits and re-run before making stronger conclusions.

## Practical loop

1. Capture one run.
2. Analyze.
3. Follow one next check.
4. Change one thing.
5. Re-run under comparable load.
6. Compare suspect movement and p95 shares.
