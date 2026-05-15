# tailtriage-tracing

`tailtriage-tracing` is the tracing intake bridge for `tailtriage`.

This crate defines semantic-convention constants and import-facing data types that will be used to convert tracing-shaped span records into `tailtriage_core::Run`.

## Scope (first slice)

This crate currently provides:

- `tt.*` semantic field-name constants
- data types for span-like records and import options
- warning and error types for future import workflows

This crate does **not** yet provide:

- JSONL parsing
- a `tracing::Layer`
- CLI commands
- OpenTelemetry or OTLP intake/export
- analyzer behavior changes

`tailtriage-tracing` is not a tracing backend, not an observability platform, and not a replacement analyzer.
