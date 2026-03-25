# Architecture (MVP)

`tailtriage` is a file-based triage pipeline over one instrumented run plus optional runtime/framework adapters.

## Why focused crates

The project is split into focused crates so service instrumentation, Tokio runtime enrichment, framework adapters, and artifact diagnosis can evolve independently while staying on one shared run schema.

## Flow

1. Service code records request/queue/stage/in-flight signals via `tailtriage-core`.
2. Optional runtime snapshots are collected by `tailtriage-tokio`.
3. Optional framework boundary helpers are provided by adapter crates such as `tailtriage-axum`.
4. `tailtriage-core` writes one JSON run artifact (`Run`).
5. `tailtriage-cli` ranks suspects from that artifact.

## Crate responsibilities

### `tailtriage-core`

- run schema (`Run`, metadata, events, snapshots)
- collection lifecycle (`Tailtriage::builder(...).build`, `shutdown`, `snapshot`)
- split request lifecycle API (`begin_request` / `begin_request_with` returning `StartedRequest { handle, completion }`)
- instrumentation wrappers on `RequestHandle` (`queue`, `stage`, `inflight`)
- completion wrappers on `RequestCompletion` (`finish`, `finish_ok`, `finish_result`)
- local JSON sink (`LocalJsonSink`)

### `tailtriage-tokio`

- runtime sampling (`RuntimeSampler`)
- runtime snapshot capture (`capture_runtime_snapshot`)
- request lifecycle starts via `Tailtriage::begin_request(...)` / `begin_request_with(...)`
- `RequestHandle` is instrumentation-only
- `RequestCompletion` is explicit finish-only

Some runtime metrics require `tokio_unstable`; unavailable fields are recorded as `None`.

### `tailtriage-axum`

- optional axum adapter middleware (`middleware`)
- optional axum request extractor (`TailtriageRequest`)
- framework-boundary request start/finish wiring with explicit handler instrumentation preserved

### `tailtriage-cli`

- parse run JSON
- compute request percentiles
- apply rule-based diagnosis ranking
- render text or JSON report

The CLI consumes run artifacts and does not need to be embedded into your service.

`shutdown()` does not auto-finish requests or fabricate request outcomes/timings. Unfinished pending requests are surfaced in run metadata warnings, and `strict_lifecycle(true)` can make `shutdown()` fail.

## Contract boundary

- Input: one local `Run` JSON artifact.
- Output: ranked suspects with evidence and next checks.
- Non-claim: proven root cause.
