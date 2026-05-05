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
- evidence_quality (structured coverage/completeness/interpretability summary)
- ranked suspects (primary + secondary)
- optional `route_breakdowns[]` supporting context when route divergence adds signal

Each suspect includes:

- `kind`
- `score`
- `confidence`
- `evidence[]`
- `next_checks[]`
- `confidence_notes[]` (present and empty unless confidence is capped by evidence limits or explicit ambiguity applies)

`p95_queue_share_permille` and `p95_service_share_permille` are independent percentile summaries and do not need to sum to `1000`.

## Field reference (stable report shape)

- `request_count`: number of requests observed in the run artifact.
- `p50_latency_us` / `p95_latency_us` / `p99_latency_us`: request latency percentiles in microseconds.
- `p95_queue_share_permille`: p95 queue-time share per request (0..1000 scale).
- `p95_service_share_permille`: p95 service-time share per request (0..1000 scale).
- `warnings[]`: analyzer/report warnings, especially truncation-related warnings from captured-data limits. Loader/lifecycle warnings (including unfinished-request warnings) are emitted separately by the CLI loader to stderr before the report output.
- `evidence_quality`: structured signal coverage status, truncation counters, and overall evidence quality (`strong`/`partial`/`weak`).
- `primary_suspect`: highest-ranked suspect with evidence and next checks.
- `secondary_suspects[]`: additional ranked suspects.
- `route_breakdowns[]`: route-scoped supporting summaries (bounded, optional, and empty when route-level output would duplicate global triage).
- `inflight_trend` (optional): dominant in-flight gauge trend summary when snapshots exist.

`primary_suspect` remains the global primary triage lead. Route breakdowns are supporting context and do not change global suspect ranking.
Route breakdowns only use route-attributed request/queue/stage evidence. Runtime and in-flight snapshots are global signals and are not attributed per route in breakdown scoring.

## Suspect kinds

- `application_queue_saturation`
- `blocking_pool_pressure`
- `executor_pressure_suspected`
- `downstream_stage_dominates`
- `insufficient_evidence`


## Proportional ranking model

Ranking is proportional and evidence-weighted, not fixed suspect precedence.

- Queue, blocking, executor, and downstream suspects each score from observed evidence strength.
- Strong downstream tail-request contribution can rank above weak blocking/runtime pressure.
- Strong queue pressure still ranks high when queue-share/depth signals are materially dominant.

Treat score as within-report ordering guidance, not an absolute SLA or certainty metric.

## How the analyzer ranks suspects

The analyzer is deterministic and rule-based. It does not use probabilistic or ML inference.

- `score` is a **relative evidence-ranking score within one report**.
- `score` is **not** a probability and **not** absolute severity across different captures.
- `confidence` starts from score bands and is then capped by evidence quality limits (sparse/missing/truncated/ambiguous evidence); it reflects triage ranking confidence, not causal certainty.

Signal families used for scoring:

- **Queue saturation**: p95 queue-share, queue-depth signal, in-flight growth (when present), and sample quality.
- **Blocking pool pressure**: p95/peak blocking queue depth, nonzero blocking-sample coverage, and sample quality.
- **Executor pressure**: global queue depth, local queue depth, alive-task signal (when present), in-flight growth, and sample quality.
- **Downstream dominance**: eligible stage samples, stage p95, cumulative stage share, and tail-request contribution.

Downstream candidate selection filters out very low-sample stages before ranking. That keeps sparse stage noise from outranking better-supported leads.

Blocking-looking stage names (for example `spawn_blocking`-style paths) can corroborate blocking-pool pressure when runtime blocking signals are strong; they are not always treated as independent downstream root-cause leads.

Warnings show interpretation limits (missing signal families, sparse coverage, ambiguous close scores, truncation). Warnings are additive and do not claim root cause.

As always: suspects are leads for next checks, not proof.

## Warning semantics

`warnings[]` is additive and can include multiple classes together:

- evidence-quality warnings (sparse requests, missing queue/stage/runtime signals, runtime field gaps)
- ambiguity warnings when top suspect scores are close
- truncation warnings when capture limits dropped events

Warnings lower interpretation confidence; they do not automatically invalidate suspect ranking.

## Evidence quality semantics

`evidence_quality` describes capture completeness and interpretation limits. It does **not** claim causal certainty, and suspects remain evidence-ranked leads, not proof.

- `requests`: `missing`, `partial`, `truncated`, or `present` based on completed-request count and request drops.
- `queues`, `stages`, `runtime_snapshots`, `inflight_snapshots`: per-family coverage status.
- `quality`:
  - `weak`: sparse/missing request evidence, request truncation, or no explanatory evidence families.
  - `partial`: non-request truncation or major evidence-family limitations.
  - `strong`: enough request evidence, queue or stage evidence present, no truncation limits active.

Runtime snapshots are optional input. Missing runtime snapshots add a limitation for executor/blocking interpretation, but they do not by themselves force `quality` to `partial` when queue/stage evidence is otherwise strong.

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
