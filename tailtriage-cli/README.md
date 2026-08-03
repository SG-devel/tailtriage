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

Import completed tailtriage `tt.*` tracing span JSONL into Run JSON:

```bash
tailtriage import tracing-spans-jsonl spans.jsonl --service checkout --output tailtriage-run.json
```

With optional metadata flags and strict tracing-span validation:

```bash
tailtriage import tracing-spans-jsonl spans.jsonl --service checkout --output tailtriage-run.json --service-version v1 --run-id run-42 --strict
```


`tailtriage import tracing-spans-jsonl` imports **completed tailtriage `tt.*` tracing span JSONL** into **Run JSON** (not Report JSON).

Accepted `tt.*` field types match tracing intake:

- `tt.success`: optional bool; strings `"true"` and `"false"` are also accepted case-insensitively.
- `tt.depth_at_start`: optional non-negative integer. Do not record it with debug formatting.
- `tt.outcome`: optional non-empty string.
- `tt.kind`, `tt.request_id`, `tt.route`, `tt.stage`, and `tt.queue`: scalar strings. `tt.request_id` must be the unique tailtriage request ID for one completed logical request/work item in one Run, not a broad external trace ID that can repeat.

Recommended stable input format is the tailtriage wrapper JSONL shape:

```json
{"format":"tailtriage.tracing-span.v1","span":{...}}
```

Only `tailtriage.tracing-span.v1` wrapper records are accepted for tracing JSONL file import. Files produced by current `TracingSession` completed-span JSONL output remain supported. Pre-stable/internal JSONL must be regenerated with the current writer or converted externally into `tailtriage.tracing-span.v1` before import. Ordinary `tracing_subscriber::fmt().json()` output is unsupported.

After import, run analysis separately:

```bash
tailtriage analyze tailtriage-run.json
```

Zero-request imports fail by design (the CLI loader requires at least one request).

When paths include spaces, quote them in shell usage:

```bash
tailtriage import tracing-spans-jsonl "fixtures/tracing spans.jsonl" --service checkout --output "runs/imported run.json"
```

Import behavior checklist:

- Imports completed tailtriage `tt.*` tracing span JSONL records in the documented shape.
- Writes Run JSON through the normal local JSON artifact writer, not Report JSON.
- Keeps analysis as a separate step: `tailtriage analyze tailtriage-run.json`.
- Prints import warnings to stderr as `warning: ...`.
- Uses the same `CaptureMode`/`CaptureLimits` semantics as native capture for request/stage/queue evidence retention.
- Exposes request/stage/queue limit overrides because those are the evidence types offline CLI tracing import ingests.
- Does not expose runtime-snapshot or in-flight-snapshot limit flags because this import path does not ingest those evidence types.
- Does not fabricate runtime snapshots; executor/blocking-pressure interpretation remains limited unless runtime snapshots are also captured, for example via Tokio runtime sampling.
- Treats malformed JSON input as fatal.
- In non-strict mode, skips syntactically valid malformed/incomplete `tt.*` records with `warning: ...` lines.
- Prefers complete run-relative monotonic offsets when deriving or validating elapsed duration; Unix-ms bounds are the fallback when complete run-relative offsets are absent.
- Treats `duration_us` as authoritative elapsed-time evidence when supplied and consistent with the selected timing source.
- When `duration_us` is absent, derives duration from complete run-relative offsets first, then from Unix-ms wall-clock bounds.
- Rejects `duration_us` mismatches against the selected timing source in `--strict` mode.
- Warns but keeps `duration_us` in non-strict mode when supplied duration and the selected timing source differ beyond tolerance.
- Requires `--service` to be non-empty and not whitespace-only.
- Fails when zero request events would be written, such as unrelated-only input or all-skipped malformed `tt.*` input, because `tailtriage analyze` requires at least one request in CLI-loaded run artifacts.
- Applies the same non-empty-request rule before persisting completed-span JSONL artifacts in tracing intake sessions.

`tailtriage analyze <run.json> --format json` emits the same pretty Report JSON as `tailtriage_analyzer::render_json_pretty`. Run JSON decoding and schema-envelope errors are CLI-owned; generic completed-Run integrity is delegated to `tailtriage-core`. CLI analysis strictly validates the original artifact by default: any error-level core finding stops report generation, while warning-only findings remain accepted.

Use `--allow-ambiguous-artifact` only as an explicit compatibility escape hatch. It runs canonical core permissive normalization, emits every original issue to stderr as `warning: permissive Run normalization <code> at <location>: <message>`, and analyzes normalized evidence only. Excluded or cleared evidence cannot contribute to scoring, while canonical validation summaries remain in Report `warnings[]` and `evidence_quality.limitations[]`. Analyzer library entry points remain permissive by default.

Migration: remove the former `--strict-artifact` option from strict scripts because strictness is now the default. A formerly permissive `tailtriage analyze run.json` invocation must add `--allow-ambiguous-artifact` to retain permissive behavior. Tracing import `tailtriage import tracing-spans-jsonl --strict` remains a separate malformed/incomplete `tt.*` parser and import policy; it does not control Run-artifact validation.

```text
old strict:     tailtriage analyze run.json --strict-artifact
new default:    tailtriage analyze run.json
old permissive: tailtriage analyze run.json
new permissive: tailtriage analyze run.json --allow-ambiguous-artifact
```

After core normalization, the CLI artifact loader requires at least one retained request event in `requests`. This is a command-level artifact-loading rule, not an in-process `tailtriage-analyzer` or generic core-integrity requirement.
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

## What the report contains

A report summarizes request latency and retained evidence, then provides a primary suspect, possible secondary suspects, `evidence[]`, `next_checks[]`, `confidence`, warnings, and `evidence_quality`. Optional route and temporal sections are supporting context; they do not replace the global primary lead.

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
- Run JSON schema version 2 is the current Run JSON schema version
- `metadata.finalized_at_unix_ms` is the sole run-level finalization timestamp; Event-level completion timestamps remain unchanged
- active in-memory snapshots may use `metadata.finalized_at_unix_ms: null`, but persisted CLI artifacts require numeric finalization
- Schema-v1 Run JSON is rejected and must be regenerated with a current tailtriage version
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



## Partial queue/stage evidence

Completed queue and stage distributions exclude partial observations. Partial durations are observed lower bounds: tailtriage observed the helper from first poll until Drop, not proof that the underlying operation completed, failed, or stopped. Partial evidence remains visible in event totals, evidence-quality limitations, top-level warnings, and suspect evidence.

Queue/service public p95 fields remain completed-only. A queue or downstream-stage suspect materially relying on an observed-lower-bound path cannot exceed medium confidence; partial evidence that does not affect selected eligibility or score does not automatically cap a completed candidate. Partial stage `success = false` is not interpreted as a completed operation failure.

Global, route, and temporal projections share this policy. Tracing imports remain completed-only. Completed-only Report JSON and text remain unchanged; mixed or partial Runs may change scores or ranking only when explicitly labeled lower-bound evidence is selected and qualified. Suspects remain triage leads, not root-cause proof.
