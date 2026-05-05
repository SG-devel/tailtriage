# tailtriage-cli

`tailtriage-cli` loads `tailtriage` run artifacts and turns them into a triage report.

Install it after capture instrumentation is in place.

The binary name is:

```bash
tailtriage
```

## What this tool does

`tailtriage-cli` owns the analysis-side contract:

- load a captured artifact
- validate schema compatibility
- produce JSON or human-readable triage output
- rank likely bottleneck families
- emit evidence and next checks

The output is intended to guide the next investigation step. It does **not** prove root cause on its own.

## Installation

```bash
cargo install tailtriage-cli
```

## Minimal usage

Default text output:

```bash
tailtriage analyze tailtriage-run.json
```

Machine-readable JSON output:

```bash
tailtriage analyze tailtriage-run.json --format json
```

The CLI artifact loader requires at least one request event in `requests`.

## How to read the result

Read output in this order:

1. `primary_suspect.kind`
2. `primary_suspect.evidence[]`
3. `primary_suspect.next_checks[]`

Then run one targeted check, change one thing, and re-run under comparable load.

## Representative output shape

```json
{
  "request_count": 250,
  "p50_latency_us": 782227,
  "p95_latency_us": 1468239,
  "p99_latency_us": 1518551,
  "p95_queue_share_permille": 982,
  "p95_service_share_permille": 267,
  "inflight_trend": null,
  "warnings": [],
  "evidence_quality": {
    "request_count": 250,
    "queue_event_count": 250,
    "stage_event_count": 250,
    "runtime_snapshot_count": 0,
    "inflight_snapshot_count": 0,
    "requests": "present",
    "queues": "present",
    "stages": "present",
    "runtime_snapshots": "missing",
    "inflight_snapshots": "missing",
    "truncated": false,
    "dropped_requests": 0,
    "dropped_stages": 0,
    "dropped_queues": 0,
    "dropped_inflight_snapshots": 0,
    "dropped_runtime_snapshots": 0,
    "quality": "strong",
    "limitations": ["No runtime snapshots captured; executor and blocking-pressure interpretation is limited."]
  },
  "primary_suspect": {
    "kind": "application_queue_saturation",
    "score": 90,
    "confidence": "high",
    "evidence": ["Queue wait at p95 consumes 98.2% of request time."],
    "next_checks": ["Inspect queue admission limits and producer burst patterns."]
  },
  "secondary_suspects": []
}
```

`inflight_trend` may be `null` when no in-flight gauges were captured.

## What the report contains

A report can include:

- request count
- request latency percentiles (`p50`, `p95`, `p99`)
- p95 queue/service share summaries
- optional in-flight trend summary
- report warnings from analysis/report generation (for example truncation-related)
- evidence quality coverage summary for capture completeness and interpretability
- primary and secondary suspects

`tailtriage analyze` also prints loader/lifecycle warnings to stderr before the report. Those warnings are surfaced separately; they are not merged into the report `warnings` field.

Each suspect includes:

- `kind`
- `score`
- `confidence`
- `evidence[]`
- `next_checks[]`

## Artifact compatibility contract

The `tailtriage analyze` workflow expects a supported `tailtriage` run artifact with minimum required content.

Current contract:

- top-level `schema_version` is required
- missing `schema_version` is rejected
- non-integer `schema_version` is rejected
- unsupported `schema_version` is rejected
- current supported schema version is `1`
- `requests` must contain at least one request event
- artifacts with an empty `requests` array are rejected by the CLI loader

Library note:

- this crate's library analyzer API, `analyze::analyze_run(&Run)`, can analyze an in-memory `Run` with zero requests
- the stricter non-empty `requests` rule applies to CLI artifact loading from disk

## Important interpretation notes

- suspects are investigation leads, not proof of root cause
- truncation warnings mean the diagnosis is based on partial retained data
- unfinished lifecycle warnings printed by the CLI indicate some requests were not completed cleanly
- `p95_queue_share_permille` and `p95_service_share_permille` are independent percentile summaries and do not need to sum to `1000`


## Scoring and warning behavior

Suspect ranking uses deterministic, proportional, evidence-aware scoring (0-100), not fixed suspect priority.

- Scores rank suspects **inside one report**; they are not probabilities.
- Confidence is score-derived ranking strength, not causal certainty.
- Strong downstream tail-stage contribution can outrank weak blocking/runtime signals.
- Strong queue pressure remains a high-confidence lead when queue share/depth evidence is dominant.

How to read before/after runs:

- Compare p95 latency movement first.
- Confirm primary suspect kind/rank and evidence direction.
- Use score movement as supporting context, not a standalone pass/fail rule.

Why a score can stay flat or rise after mitigation:

- Scores are relative to the evidence mix in each capture.
- If total latency drops but the remaining tail is still dominated by one suspect family, that suspect score can remain high or increase.
- This does not by itself mean mitigation failed when p95 and relevant evidence improve.

`warnings[]` may include:

- evidence-quality warnings (for example low request counts or missing signal families)
- ambiguity warnings when top suspects are genuinely close after calibration
- additive truncation warnings when capture limits drop events

## Suspect kinds

The current report surface includes these suspect kinds:

- `application_queue_saturation`
- `blocking_pool_pressure`
- `executor_pressure_suspected`
- `downstream_stage_dominates`
- `insufficient_evidence`

## When the result is `insufficient_evidence`

Usually the next step is to add more structure to capture:

- add queue wrappers around suspected waits
- add stage wrappers around suspected downstream work
- optionally add runtime sampling if runtime pressure is unclear
- re-run under comparable load

## What this tool does not do

`tailtriage-cli` does not capture instrumentation data.

Use capture-side crates for that:

- `tailtriage`: recommended capture-side entry point
- `tailtriage-core`: direct instrumentation primitives
- `tailtriage-controller`: repeated bounded windows
- `tailtriage-tokio`: runtime-pressure sampling
- `tailtriage-axum`: Axum request-boundary integration
