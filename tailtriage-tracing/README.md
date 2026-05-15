# tailtriage-tracing

`tailtriage-tracing` is a focused intake bridge surface for tracing-shaped span
records that import into `tailtriage_core::Run` for tailtriage triage workflows.

This crate is intentionally narrow:

- It defines semantic convention keys (`tt.*`) for triage-oriented span fields.
- It defines typed intake records and import option/result types.
- It converts in-memory `SpanRecord` values into `tailtriage_core::Run`.
- It imports newline-delimited JSON (JSONL) into `SpanRecord` values.
- It does **not** install or provide a `tracing_subscriber::Layer`.
- It does **not** implement OpenTelemetry or OTLP.
- It does **not** change `tailtriage-analyzer`.
- It does **not** add CLI import flags in this phase.

## Supported JSONL shape in this phase

Phase 1 intentionally supports records that can reconstruct a **completed span**
with explicit unix-millisecond timestamps.

Stable contract used by tests (recommended):

```json
{
  "span": {
    "name": "http.request",
    "id": "span-1",
    "parent_id": "root-1",
    "started_at_unix_ms": 1700000000000,
    "finished_at_unix_ms": 1700000000120,
    "fields": {
      "tt.kind": "request",
      "tt.request_id": "req-42",
      "tt.route": "/checkout"
    }
  }
}
```

Accepted timestamp keys:

- `started_at_unix_ms` / `finished_at_unix_ms`
- alias: `start_unix_ms` / `end_unix_ms`

Accepted `tt.kind` locations:

- `fields.tt.kind`
- `span.fields.tt.kind`
- top-level `tt.kind`

The importer also accepts close-event-like records when they include enough data
for one completed span in one record (name + `tt.kind` + start/end unix-ms).

Because direct `tracing-subscriber` JSON output varies by configuration, this
phase does **not** claim automatic parsing for all tracing JSON formats. If the
close record does not carry both start and finish unix-ms timestamps, it is not
converted in this phase.

## Intended field shape

Typical span fields are expected to follow this shape:

- request: `tt.kind`, `tt.request_id`, `tt.route`
- stage: `tt.kind`, `tt.request_id`, `tt.stage`
- queue: `tt.kind`, `tt.request_id`, `tt.queue`, `tt.depth_at_start`

Use this crate when you want a coherent, explicit bridge surface for importing
span-like data into tailtriage triage workflows.
