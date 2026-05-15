# tailtriage-tracing

Focused tracing intake examples for `tailtriage` triage workflows.

This crate is intentionally narrow: it shows how tracing span data can feed tailtriage triage artifacts and analysis. It is not a telemetry backend.

## Examples

- `examples/live_recorder.rs` shows a scoped subscriber with a `TracingRecorder`, one request span, one stage span, shutdown, and in-memory analysis via `tailtriage-analyzer`.
- `examples/tracing_spans.jsonl` is a small normalized JSONL fixture with one request, one stage, and one queue completed-span record.
