# tailtriage-tracing

`tailtriage-tracing` is a focused intake bridge for converting tracing-shaped span
records into `tailtriage_core::Run` inputs.

This crate is intentionally narrow:

- It defines semantic convention keys (`tt.*`) for triage-oriented span fields.
- It defines public data types for imported span records and import options.
- It does **not** implement a tracing backend.
- It does **not** implement OpenTelemetry or OTLP.
- It does **not** replace `tailtriage-analyzer`.

Use this crate when you want a coherent, explicit bridge surface for importing
span-like data into the tailtriage triage workflow.
