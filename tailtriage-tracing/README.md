# tailtriage-tracing

`tailtriage-tracing` is a focused triage intake bridge for tracing-shaped span
records. It converts finished span records into `tailtriage_core::Run` by
reusing the `run_from_span_records` conversion core.

This crate is intentionally narrow:

- It defines semantic convention keys (`tt.*`) for triage-oriented span fields.
- It defines typed intake records and import option/result types.
- It imports JSONL with explicit completed-span timestamps.
- It does **not** provide a live `tracing_subscriber::Layer`.
- It does **not** implement OpenTelemetry or OTLP.
- It does **not** change `tailtriage-analyzer` behavior.

## Supported JSONL shape (Phase 1)

Phase 1 supports records that can recover a completed span from one line and
include explicit unix-millisecond start/end timestamps.

Stable normalized contract used by tests:

```json
{
  "span": {
    "name": "http.request",
    "id": "s1",
    "parent_id": "p1",
    "started_at_unix_ms": 1700000000000,
    "finished_at_unix_ms": 1700000000120,
    "fields": {
      "tt.kind": "request",
      "tt.request_id": "req-42",
      "tt.route": "/checkout",
      "tt.success": true
    }
  }
}
```

Also accepted aliases for timestamps:

- `start_unix_ms` for `started_at_unix_ms`
- `end_unix_ms` for `finished_at_unix_ms`

Field flattening accepts `tt.kind` under:

- `fields.tt.kind`
- `span.fields.tt.kind`
- top-level `tt.kind`

`tracing-subscriber` JSON can vary by formatter/configuration. This importer
only accepts close-event records when those records carry explicit start/end
unix-ms timestamps. It does not guess missing timing from log receive time.

## Notes

The normalized JSONL shape above is the stable contract for tests and examples.
CLI examples can document how to emit this shape directly or use future live
recorder support.
