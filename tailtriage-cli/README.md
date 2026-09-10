# tailtriage-cli

`tailtriage-cli` is the command-line package for turning a saved `tailtriage` Run into a focused
Tokio tail-latency triage report. The report contains evidence-ranked suspects and next checks;
suspects are investigation leads, not proof of root cause.

The installed binary is `tailtriage`. This package retains a documentation-only Rust library
target, but exposes no command-loading or configuration API. Use `tailtriage-analyzer` for
in-process analysis and typed reports, and `tailtriage-core` for generic Run validation and
normalization.

## Install and analyze a saved Run

```bash
cargo install tailtriage-cli
```

Analyze a saved **Run JSON** artifact produced by a tailtriage capture or import path:

```bash
tailtriage analyze tailtriage-run.json
```

Text is the default output. To write pretty canonical **Report JSON** to stdout instead:

```bash
tailtriage analyze tailtriage-run.json --format json
```

The first-use workflow is:

```text
Run JSON input
-> tailtriage analyze
-> evidence-ranked Report
-> inspect evidence and next_checks
-> perform one targeted check
-> capture and re-run under comparable conditions
```

Run JSON and Report JSON have different jobs:

- **Run JSON** is captured or imported evidence and is input to `tailtriage analyze`. Run schema,
  lifecycle, finalization, and artifact policies apply to it.
- **Report JSON** is the analysis result from `tailtriage analyze --format json`. It serializes the
  analyzer's typed `Report`; it is not a Run artifact and is not valid input to `tailtriage
  analyze`.

Tracing span JSONL, described below, is a third input shape. It is neither Run JSON nor Report
JSON.

Use `tailtriage --help` and each command's `--help` for the complete command and flag reference.

## Read the result and choose a next check

Start with `primary_suspect`, the strongest global lead. Read its `evidence` to understand the
observations behind the ranking, then choose one item from `next_checks`. The
`secondary_suspects` are alternative leads. Route and temporal sections provide supporting
context rather than replacing the global lead.

The current machine diagnosis kinds are:

- `application_queue_pressure` (application queue pressure)
- `blocking_pool_pressure`
- `executor_pressure`
- `downstream_stage_dominance`
- `insufficient_evidence`

A suspect's `score` ranks it within that report; it is not a probability. `confidence` describes
support for the evidence and ranking, not causal certainty. Report warnings and
`evidence_quality` expose interpretation limits. `insufficient_evidence` means the retained
evidence does not support a stronger lead: instrument one suspected queue or stage, or add runtime
sampling, then capture and re-run under comparable load.

Completed queue and stage distributions contain completed observations. Separately labeled
partial observations are elapsed lower bounds from first poll until Drop. Drop does not prove
that the underlying operation completed, failed, was cancelled, or stopped. A candidate that
materially relies on selected lower-bound evidence cannot exceed Medium confidence under the
current analyzer policy. Truncation and missing evidence can also limit interpretation. In all
cases, a suspect remains a triage lead rather than root-cause proof.

## Optional: import tailtriage tracing span JSONL

Convert completed tailtriage tracing spans into a saved Run before analyzing them:

```bash
tailtriage import tracing-spans-jsonl spans.jsonl \
  --service checkout \
  --output tailtriage-run.json
tailtriage analyze tailtriage-run.json
```

The stable file-import format is one versioned wrapper per line:

```json
{"format":"tailtriage.tracing-span.v1","span":{...}}
```

Only this stable wrapper shape is supported. Ordinary `tracing_subscriber::fmt().json()` output,
raw top-level span records, and unversioned envelopes are unsupported. Malformed JSON is fatal.
Syntactically valid wrappers with malformed or incomplete semantic `tt.*` evidence follow the
selected tracing import policy described below; this input is not a generic tracing archive.

A request candidate follows the tailtriage tracing convention and includes `tt.kind = "request"`,
`tt.request_id`, `tt.route`, and valid source/timing data. Stage and queue evidence reuse the same
logical request ID. Within one Run, `request_id` identifies one completed tailtriage request or
work item; it is not automatically a broader external trace ID that may repeat. Optional request
outcome and child details include `tt.outcome`, `tt.success`, and `tt.depth_at_start`. Exact tracing
field and `SpanRecord` contracts belong to `tailtriage-tracing` public Rustdoc.

### Duration derivation and precise validation

Tracing conversion derives elapsed duration in this order:

1. Supplied `duration_us` is authoritative elapsed duration and is retained.
2. Without explicit duration, complete ordered `started_at_run_us` and `finished_at_run_us`
   derive `finished_at_run_us - started_at_run_us`.
3. Otherwise, coarse `finished_at_unix_ms - started_at_unix_ms` is converted to microseconds with
   the implementation's saturating arithmetic. This fallback is not high-precision timing.

Core duration-consistency validation is separate. A `duration_mismatch` occurs only when a
complete, valid run-relative interval exists and
`abs(duration_us - (finished_at_run_us - started_at_run_us)) > 2,000` microseconds. Unix-ms bounds
are not substituted as a generic mismatch oracle when precise offsets are absent.

When both run-relative endpoints are absent, core reports
`precise_interval_validation_unavailable` as a warning and authoritative duration evidence remains
usable. That warning-only limitation does not make default strict saved-Run analysis fail. Exactly
one endpoint is an error-level partial interval; a complete inverted interval is also invalid; and
a complete interval beyond the 2,000-microsecond tolerance is an error-level mismatch. Permissive
normalization can clear these invalid optional offsets while retaining otherwise usable
authoritative duration evidence. It does not repair precision from wall-clock timestamps.

### Import retention and policy

Optional metadata and retention controls are:

```text
--service-version <VERSION>
--run-id <RUN_ID>
--strict
--mode <light|investigation>
--max-requests <N>
--max-stages <N>
--max-queues <N>
```

The mode and limits use tracing/core capture-limit semantics for request, stage, and queue evidence
that this offline importer can ingest. It does not ingest runtime snapshots, in-flight snapshots,
or Tokio sampler state, and there are no corresponding import flags. Missing runtime evidence can
limit executor- and blocking-pressure interpretation; it does not show that runtime pressure was
absent.

`--max-requests 0` is rejected because this command must be capable of persisting a Run with at
least one completed request for later CLI analysis. More generally, import rejects zero-request
output before persistence. This is a CLI persistence condition, not a universal core capture-limit
or Run-validity rule. The tracing helper that enforces persistability checks only for at least one
completed request; it is not a complete artifact-validity checker.

## Optional: tune analyzer interpretation

Start with analyzer defaults. When retained evidence needs deliberate threshold tuning, use:

```text
--analyzer-config <TOML>
--analyzer-set PATH=VALUE
tailtriage analyzer-options
```

For example:

```bash
tailtriage analyze tailtriage-run.json \
  --analyzer-set queueing.trigger_permille=450
```

Configuration precedence is:

1. `AnalyzeOptions` defaults
2. `--analyzer-config` TOML
3. repeatable `--analyzer-set` overrides in command order; the last assignment to a path wins

Invalid configuration fails analysis rather than being silently ignored. `tailtriage
analyzer-options` prints the exhaustive installed inventory of paths, defaults, types, and effects
without requiring a Run. Exact analyzer configuration and Report contracts belong to
`tailtriage-analyzer` public Rustdoc.

Tuning changes interpretation of retained evidence, not the captured Run artifact. It cannot
restore evidence that was missing, excluded, or dropped.

## Saved-Run artifact and strictness policy

The current supported Run schema is version 2. For `tailtriage analyze`:

- top-level `schema_version` is required, and unsupported versions fail;
- malformed or truncated JSON and incompatible JSON shape fail;
- persisted input must be finalized: `metadata.finalized_at_unix_ms` must be numeric;
- an active/unfinalized snapshot with `metadata.finalized_at_unix_ms = null` is rejected before
  analysis; and
- normalization must leave at least one retained request.

The finalization and nonempty-request rules are CLI persisted-artifact requirements. They do not
imply that every in-memory typed `Run` accepted by `tailtriage-analyzer` must meet them; in
particular, the in-process analyzer can analyze a zero-request typed Run. `tailtriage-core` public
Rustdoc owns the exact generic validation and normalization contracts.

### Default saved-Run analysis

`tailtriage analyze run.json` strictly validates the original Run's generic integrity. Any
error-level core validation finding stops report generation. Warning-only findings, including
missing optional run-relative precision, remain accepted.

### `--allow-ambiguous-artifact`

This explicit compatibility escape hatch prints canonical findings for the original Run as
warnings on stderr and analyzes evidence under core permissive normalization semantics. It can
exclude invalid evidence or clear invalid optional precision, and it still fails if no request
remains after normalization.

It does not accept arbitrary input, disable decoding/schema/finalization requirements, suppress
warnings, reconstruct lost evidence, or make the original input strictly valid.

### Tracing import strictness is separate

Tracing import is permissive by default: where implemented, it warns and skips source-invalid
semantic tracing records, then applies canonical permissive core handling. Adding `--strict` to
`tailtriage import tracing-spans-jsonl` instead controls tracing-source conversion/import policy.
Strict import can reject malformed or incomplete tailtriage semantic evidence treated as a strict
violation and error-level core findings in the converted Run.

Import `--strict` does not control later saved-Run analysis. Conversely,
`--allow-ambiguous-artifact` does not control JSONL parsing or tracing conversion.

## Output and failure behavior

- Successful analysis writes a text Report to stdout by default, or pretty Report JSON with
  `--format json`.
- Loader, lifecycle, and permissive-artifact warnings are written separately to stderr where
  applicable. A warning printed by the CLI is not automatically inserted into the typed Report's
  `warnings` field.
- Decode, schema, finalization, strict validation, configuration, and rendering failures exit
  non-zero. Failed strict validation does not emit a normal Report to stdout.
- Tracing import warnings are written as warning lines to stderr. Successful import writes Run JSON
  to `--output`; import does not emit an analyzer Report. Import failure exits non-zero.
- `tailtriage analyzer-options` writes its inventory to stdout and requires no Run artifact.

The CLI captures no evidence itself. Capture or import a bounded Run, use the Report to select one
targeted next check, and compare a later capture under comparable conditions. Missing or partial
instrumentation limits what the evidence can support; the tool is not an observability backend,
distributed tracing system, or root-cause proof engine.
