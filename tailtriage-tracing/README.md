# tailtriage-tracing

`tailtriage-tracing` is a focused tracing intake bridge for `tailtriage`.

This crate defines public semantic convention constants and typed intake records that will be converted into `tailtriage_core::Run`.

## Scope in this first slice

This crate currently provides:

- `tt.*` semantic convention constants
- span and import data types
- import option and warning/error scaffolding

This crate does **not** yet provide:

- JSONL parsing
- live `tracing` subscriber/layer recording
- OpenTelemetry/OTLP ingestion or export
- CLI commands
- analyzer behavior changes

`tailtriage-tracing` is not a tracing backend and not an observability platform. It exists to help triage workflows by bridging tracing-shaped span data into the `tailtriage` run schema.
