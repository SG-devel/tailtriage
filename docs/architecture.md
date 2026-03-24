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
- instrumentation wrappers (`request`, `queue`, `stage`, `inflight`)
- local JSON sink (`LocalJsonSink`)

### `tailtriage-tokio`

- runtime sampling (`RuntimeSampler`)
- runtime snapshot capture (`capture_runtime_snapshot`)
- request context instrumentation via `Tailtriage::request(...)` and `RequestContext` helpers

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
