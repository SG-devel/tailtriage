# tailtriage-tracing

`tailtriage-tracing` is a focused intake bridge for tracing-shaped span records
that can be converted into `tailtriage_core::Run` for tail-latency triage.

This crate is intentionally narrow:

- It defines semantic convention keys (`tt.*`) for triage-oriented span fields.
- It defines typed intake records and import option/result types.
- It converts `SpanRecord` values using `run_from_span_records`.
- It imports newline-delimited JSON through `import_jsonl_reader` and `import_jsonl_path`.
- It does **not** add CLI commands yet.
- It does **not** install or provide a live `tracing_subscriber::Layer`.
- It does **not** implement OpenTelemetry or OTLP.
- It does **not** change `tailtriage-analyzer`.

## Supported JSONL shape (Phase 1)

Phase 1 JSONL import supports completed-span records with explicit unix-ms start/end timestamps.
The stable contract for tests and docs is this normalized line shape:

```json
{
  "span": {
    "name": "http.request",
    "id": "abc",
    "parent_id": "root",
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

Also accepted:

- `start_unix_ms` / `end_unix_ms` timestamp aliases.
- flattened `tt.kind` locations in `fields.tt.kind`, `span.fields.tt.kind`, or top-level `tt.kind` in a record.
- close-event-like records (`event`/`message` contains `close` or `closed`) when they include a span name, `tt.kind`, and explicit unix-ms start/end timestamps.

Direct `tracing-subscriber` JSON varies by formatter configuration. This importer does **not** claim broad automatic parsing of all tracing JSON outputs. In this phase, it expects completed-span records that already include explicit start/end unix-ms timestamps.

## Intended field shape

Typical span fields:

- request: `tt.kind`, `tt.request_id`, `tt.route`
- stage: `tt.kind`, `tt.request_id`, `tt.stage`
- queue: `tt.kind`, `tt.request_id`, `tt.queue`, `tt.depth_at_start`

Use this crate when you want a coherent, explicit bridge for importing
span-like data into tailtriage triage workflows.
