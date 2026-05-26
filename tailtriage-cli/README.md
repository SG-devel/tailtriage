# tailtriage-cli

`tailtriage-cli` loads `tailtriage` run artifacts and turns them into a triage report.

Install it after capture instrumentation is in place.

The binary name is:

```bash
tailtriage
```

## What this tool does

`tailtriage-cli` owns the command-line artifact-analysis contract:

- load a captured artifact
- validate schema compatibility
- produce JSON or human-readable triage output
- invoke `tailtriage-analyzer` on loaded artifacts and rank likely bottleneck families
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

Import completed `tt.*` tracing span JSONL into Run JSON:

```bash
tailtriage import tracing-json spans.jsonl --service checkout --output tailtriage-run.json
```

With optional metadata flags, strict validation, and explicit format:

```bash
tailtriage import tracing-json spans.jsonl --service checkout --output tailtriage-run.json --service-version v1 --run-id run-42 --strict \
  --input-format tailtriage-span-jsonl
```


`tailtriage import tracing-json` imports **completed `tt.*` tracing span JSONL** into **Run JSON** (not Report JSON).

Recommended stable input format is the tailtriage wrapper JSONL shape:

```json
{"format":"tailtriage.tracing-span.v1","span":{...}}
```

`--input-format` values:
- `tailtriage-span-jsonl`
- `compatible`

Behavior:
- `tailtriage-span-jsonl` enforces wrapper-only parsing.
- `compatible` keeps compatibility parsing for pre-stable/internal normalized shapes and rejects ordinary tracing log JSON (including `fmt().json` output) early with setup guidance; timing is not guessed from JSONL line receive time.
- `compatible` is for pre-stable/internal normalized completed-span shapes with explicit start/end timestamps; it is not auto-detection and not generic tracing JSON import.

After import, run analysis separately:

```bash
tailtriage analyze tailtriage-run.json
```

Zero-request imports fail by design (the CLI loader requires at least one request).

When paths include spaces, quote them in shell usage:

```bash
tailtriage import tracing-json "fixtures/tracing spans.jsonl" --service checkout --output "runs/imported run.json"
```

The command imports completed `tt.*` tracing span records in the documented JSONL shape and writes Run JSON through the normal local JSON artifact writer, not Report JSON. Import warnings are printed to stderr as `warning: ...`. Analysis is a separate step: `tailtriage analyze tailtriage-run.json`.
Tracing import and native capture share the same CaptureMode/CaptureLimits semantics for request/stage/queue evidence retention. Offline CLI tracing import exposes request/stage/queue limit overrides because those are the evidence types it imports. It intentionally does not expose runtime-snapshot or in-flight-snapshot limit flags because this import path does not ingest those evidence types. Tracing-only imports do not fabricate runtime snapshots; executor/blocking-pressure interpretation remains limited unless runtime snapshots are also captured (for example via Tokio runtime sampling).
Malformed JSON input remains fatal. In non-strict mode, syntactically valid malformed/incomplete `tt.*` records are skipped with `warning: ...` lines.
`--service` must not be empty or whitespace.
Import fails when zero request events would be written (for example unrelated-only input or all-skipped malformed `tt.*` input), because `tailtriage analyze` requires at least one request in CLI-loaded run artifacts. The same non-empty-request rule applies before persisting completed-span JSONL artifacts in tracing intake sessions.

`tailtriage analyze <run.json> --format json` emits the same pretty Report JSON as `tailtriage_analyzer::render_json_pretty`.

The CLI artifact loader requires at least one request event in `requests`. This is a CLI artifact-loading rule, not an in-process `tailtriage-analyzer` requirement for already-constructed `Run` values.
CLI input is Run artifact JSON from disk. CLI does not consume Report JSON as input.

## Analyzer tuning flags

Start with default analyzer behavior first.

- `--analyzer-config <path>` loads analyzer options from TOML (`[analyzer]`, `schema_version = 1`).
- `--analyzer-set PATH=VALUE` applies one override (repeatable).
- `--help-analyzer-options` prints supported override paths and value formats.

Precedence:

1. analyzer defaults
2. options loaded from `--analyzer-config`
3. one or more `--analyzer-set PATH=VALUE` overrides (last assignment to the same path wins)

Override parsing/validation errors fail fast so misspelled paths or invalid values are rejected rather than silently ignored.

Run artifact JSON remains CLI input. Report JSON remains analyzer/CLI output. Analyzer tuning changes report interpretation, not captured artifact contents.

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
    "limitations": ["Runtime snapshots are missing, limiting executor and blocking-pressure interpretation."]
  },
  "primary_suspect": {
    "kind": "application_queue_saturation",
    "score": 90,
    "confidence": "high",
    "evidence": ["Queue wait at p95 consumes 98.2% of request time."],
    "next_checks": ["Inspect queue admission limits and producer burst patterns."],
    "confidence_notes": []
  },
  "secondary_suspects": [],
  "route_breakdowns": [],
  "temporal_segments": []
}
```

`inflight_trend` may be `null` when no in-flight gauges were captured.

`route_breakdowns` is always present in JSON output and is usually an empty array. It is populated only when at least two captured routes have enough completed requests and route-level context adds signal, such as different route-level primary suspects or a large route p95 latency spread. The global `primary_suspect` remains the primary full-run triage lead. Route breakdowns are supporting context only. They use route-attributed request, queue, and stage events. Runtime snapshots and in-flight gauges are global signals, so they are intentionally not attributed to individual routes. Route-level summaries do not prove per-route root cause.

`temporal_segments` is always present in JSON output and is usually an empty array. It is populated only when conservative within-run early/late checks detect material signal movement. The global `primary_suspect` remains global and unchanged by segment generation. Temporal segments are within-run hints, not proof of phase-specific root cause. Report warnings can explicitly call out large early/late p95 movement. Runtime and in-flight phase attribution uses timestamp-filtered segment windows and is limited when segment-filtered samples are sparse; when early/late windows overlap under concurrency, that timestamp-filtered runtime/in-flight attribution is approximate.

## What the report contains

A report can include:

- request count
- request latency percentiles (`p50`, `p95`, `p99`)
- p95 queue/service share summaries
- optional in-flight trend summary
- report warnings from analysis/report generation (for example truncation-related)
- structured evidence quality coverage/status summary
- primary and secondary suspects

`tailtriage analyze` also prints loader/lifecycle warnings to stderr before the report. Those warnings are surfaced separately; they are not merged into the report `warnings` field.

Each suspect includes:

- `kind`
- `score`
- `confidence`
- `evidence[]`
- `next_checks[]`
- `confidence_notes[]` (present and empty unless evidence-aware caps affect confidence, or explicit ambiguity applies)

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

For Rust in-process usage, use `tailtriage-analyzer` directly (`analyze_run`, `render_text`, typed `Report`).
The stricter non-empty `requests` rule applies to CLI artifact loading from disk.
Loader, parse, validation, and render errors return a non-zero process exit through the CLI.

## Important interpretation notes

- suspects are investigation leads, not proof of root cause
- truncation warnings mean the diagnosis is based on partial retained data
- unfinished lifecycle warnings printed by the CLI indicate some requests were not completed cleanly
- `p95_queue_share_permille` and `p95_service_share_permille` are independent percentile summaries and do not need to sum to `1000`


## Scoring and warning behavior

Suspect ranking uses deterministic, proportional, evidence-aware scoring (0-100), not fixed suspect priority.

- Scores rank suspects **inside one report**; they are not probabilities.
- Confidence is score-derived ranking strength and may be evidence-quality capped; it is not causal certainty.
- `confidence_notes[]` explain caps, including sparse samples, truncation, missing instrumentation, ambiguous top scores, and partial-vs-missing runtime snapshot limits.
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

Persisted Run JSON intended for `tailtriage analyze` must include at least one completed request event; in-process library snapshots may still be zero-request for inspection.
