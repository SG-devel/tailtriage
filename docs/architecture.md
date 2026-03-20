# Architecture (MVP)

`tailscope` is a file-based diagnosis pipeline over one instrumented run.

## Flow

1. Service code records request/queue/stage/in-flight signals via `tailscope-core`.
2. Optional runtime snapshots are collected by `tailscope-tokio`.
3. `tailscope-core` writes one JSON run artifact (`Run`).
4. `tailscope-cli` ranks suspects from that artifact.

## Crate responsibilities

### `tailscope-core`

- run schema (`Run`, metadata, events, snapshots)
- collection lifecycle (`Tailscope::init`, `flush`, `snapshot`)
- instrumentation wrappers (`request`, `queue`, `stage`, `inflight`)
- local JSON sink (`LocalJsonSink`)

### `tailscope-tokio`

- runtime sampling (`RuntimeSampler`)
- runtime snapshot capture (`capture_runtime_snapshot`)
- macro re-export: `#[instrument_request]`

Note: some runtime metrics require `tokio_unstable`; unavailable fields are recorded as `None`.

### `tailscope-cli`

- parse run JSON
- compute request percentiles
- apply rule-based diagnosis ranking
- render text or JSON report

## Contract boundary

- Input: one local `Run` JSON artifact.
- Output: ranked suspects with evidence and next checks.
- Non-claim: proven root cause.

## Recommended integration sequence

1. init one collector
2. wrap request handlers
3. instrument key queue waits
4. instrument key downstream stages
5. optionally enable runtime sampler
6. flush and analyze
