# Architecture (MVP)

`tailtriage` is a file-based triage pipeline over one instrumented run.

## Why three crates

The project is split into three crates so service instrumentation, Tokio runtime enrichment, and artifact diagnosis can evolve independently while staying on one shared run schema.

## Flow

1. Service code records request/queue/stage/in-flight signals via `tailtriage-core`.
2. Optional runtime snapshots are collected by `tailtriage-tokio`.
3. `tailtriage-core` writes one JSON run artifact (`Run`).
4. `tailtriage-cli` ranks suspects from that artifact.

## Crate responsibilities

### `tailtriage-core`

- run schema (`Run`, metadata, events, snapshots)
- collection lifecycle (`Tailtriage::builder(...).build`, `shutdown`, `snapshot`)
- split request lifecycle API (`begin_request`/`begin_request_with` returning `StartedRequest`)
- instrumentation wrappers on `RequestHandle` (`queue`, `stage`, `inflight`)
- completion wrappers on `RequestCompletion` (`finish`, `finish_ok`, `finish_result`)
- local JSON sink (`LocalJsonSink`)

### `tailtriage-tokio`

- runtime sampling (`RuntimeSampler`)
- runtime snapshot capture (`capture_runtime_snapshot`)
- split request lifecycle instrumentation via `Tailtriage::begin_request(...)`, `RequestHandle` helpers, and explicit `RequestCompletion`

Some runtime metrics require `tokio_unstable`; unavailable fields are recorded as `None`.

### `tailtriage-cli`

- parse run JSON
- compute request percentiles
- apply rule-based diagnosis ranking
- render text or JSON report

The CLI consumes run artifacts and does not need to be embedded into your service.

## Contract boundary

- Input: one local `Run` JSON artifact.
- Output: ranked suspects with evidence and next checks.
- Non-claim: proven root cause.

`shutdown()` does not auto-finish requests or fabricate request outcomes/timings; unfinished lifecycle state is surfaced in run metadata warnings, and strict lifecycle mode can fail shutdown.
