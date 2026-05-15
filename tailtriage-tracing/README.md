# tailtriage-tracing

`tailtriage-tracing` is a focused intake bridge surface for tracing-shaped span
records that can be converted into `tailtriage_core::Run` inputs.

This crate is intentionally narrow:

- It defines semantic convention keys (`tt.*`) for triage-oriented span fields.
- It defines typed intake records and import option/result types.
- It converts typed `SpanRecord` values with `run_from_span_records`.
- It imports JSONL from readers/paths when records contain completed span timing.
- It does **not** install or provide a `tracing_subscriber::Layer`.
- It does **not** implement OpenTelemetry or OTLP.
- It does **not** change `tailtriage-analyzer`.

## JSONL import support in this phase

Public APIs:

- `import_jsonl_reader(reader, options)`
- `import_jsonl_path(path, options)`

Supported stable contract (recommended for tests and integrations):

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

Notes:

- Importer accepts `started_at_unix_ms`/`finished_at_unix_ms` and aliases `start_unix_ms`/`end_unix_ms`.
- Field flattening accepts `fields.tt.kind`, `span.fields.tt.kind`, or top-level `tt.kind`.
- Scalars can be strings, bools, numbers, or null.
- Empty lines are ignored.
- Malformed JSON line input is an import error in both strict and non-strict mode.

## tracing-subscriber JSON caveat

Direct `tracing-subscriber` JSON output can vary by formatter configuration. In
this phase, the importer supports:

- normalized completed-span JSONL (shape above), and
- close-event-like records only when they include explicit start/end unix-ms timestamps.

It does not guess missing timing from line receive time and does not claim broad
automatic parsing for every tracing JSON variant.

## Intended field shape

Typical span fields are expected to follow this shape:

- request: `tt.kind`, `tt.request_id`, `tt.route`
- stage: `tt.kind`, `tt.request_id`, `tt.stage`
- queue: `tt.kind`, `tt.request_id`, `tt.queue`, `tt.depth_at_start`
