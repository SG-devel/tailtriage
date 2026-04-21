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

```bash
tailtriage analyze tailtriage-run.json --format json
```

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
  "warnings": [],
  "primary_suspect": {
    "kind": "application_queue_saturation",
    "score": 90,
    "confidence": "high",
    "evidence": [
      "Queue wait at p95 consumes 98.2% of request time."
    ],
    "next_checks": [
      "Inspect queue admission limits and producer burst patterns."
    ]
  },
  "secondary_suspects": []
}
```

## What the report contains

A report can include:

- request count
- request latency percentiles (`p50`, `p95`, `p99`)
- p95 queue/service share summaries
- optional in-flight trend summary
- warnings, including truncation and lifecycle context
- primary and secondary suspects

Each suspect includes:

- `kind`
- `score`
- `confidence`
- `evidence[]`
- `next_checks[]`

## Artifact compatibility contract

The analyzer expects a compatible `tailtriage` run artifact.

Current contract:

- top-level `schema_version` is required
- missing `schema_version` is rejected
- non-integer `schema_version` is rejected
- unsupported `schema_version` is rejected
- current supported schema version is `1`

## Important interpretation notes

- suspects are investigation leads, not proof of root cause
- truncation warnings mean the diagnosis is based on partial retained data
- unfinished lifecycle warnings mean some requests were not completed cleanly
- `p95_queue_share_permille` and `p95_service_share_permille` are independent percentile summaries and do not need to sum to `1000`

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

- `tailtriage`
- `tailtriage-core`
- `tailtriage-controller`
- `tailtriage-tokio`
- `tailtriage-axum`

## Related crates

- `tailtriage`: recommended capture-side entry point
- `tailtriage-core`: direct instrumentation primitives
- `tailtriage-tokio`: runtime-pressure sampling
- `tailtriage-controller`: repeated bounded windows
