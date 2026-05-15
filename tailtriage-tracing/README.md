# tailtriage-tracing

`tailtriage-tracing` is a focused intake bridge surface for tracing-shaped span
records that are intended for future conversion into `tailtriage_core::Run` inputs.

This crate is intentionally narrow:

- It defines semantic convention keys (`tt.*`) for triage-oriented span fields.
- It defines typed intake records and import option/result types.
- It does **not** yet convert records into `tailtriage_core::Run`.
- It does **not** parse JSONL.
- It does **not** install or provide a `tracing_subscriber::Layer`.
- It does **not** implement OpenTelemetry or OTLP.
- It does **not** change `tailtriage-analyzer`.

## Intended field shape

Typical span fields are expected to follow this shape:

- request: `tt.kind`, `tt.request_id`, `tt.route`
- stage: `tt.kind`, `tt.request_id`, `tt.stage`
- queue: `tt.kind`, `tt.request_id`, `tt.queue`, `tt.depth_at_start`

Use this crate when you want a coherent, explicit bridge surface for importing
span-like data into future tailtriage import paths.
