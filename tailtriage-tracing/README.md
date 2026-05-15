# tailtriage-tracing

`tailtriage-tracing` is the tracing intake bridge for `tailtriage`.

It defines the data model and semantic convention keys for span-shaped inputs that can be converted into `tailtriage_core::Run` artifacts.

## Current scope

This crate currently provides:

- semantic convention constants for `tt.*` fields
- span record and import option data types
- warnings and import error types for future import flows

This crate does **not** yet provide:

- JSONL parsing
- live `tracing` Layer capture
- OpenTelemetry or OTLP import/export
- analyzer changes

The goal is to keep a narrow triage-focused intake surface that complements `tailtriage` diagnostics, without becoming a tracing backend or observability platform.
