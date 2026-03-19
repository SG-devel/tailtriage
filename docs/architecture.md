# tailscope architecture (MVP)

This document describes how the current MVP implementation is structured.

## High-level flow

1. Application code records request/queue/stage/in-flight signals through `tailscope-core`.
2. Optional Tokio runtime sampling records runtime snapshots through `tailscope-tokio`.
3. `tailscope-core` writes one run artifact (`Run`) as JSON.
4. `tailscope-cli` reads the artifact and ranks diagnosis suspects.

## Crate responsibilities

## `tailscope-core`

Responsibilities:

- run schema (`Run`, metadata, event/snapshot structs)
- collection lifecycle (`Tailscope::init`, `flush`, `snapshot`)
- request wrapper (`request`)
- queue/stage wrappers (`queue(...).await_on(...)`, `stage(...).await_on(...)`)
- in-flight RAII tracking (`inflight`)
- local JSON sink (`LocalJsonSink`)

Design intent:

- explicit instrumentation boundaries
- low implementation complexity
- deterministic local artifact output

## `tailscope-tokio`

Responsibilities:

- runtime sampling loop (`RuntimeSampler`)
- runtime metric snapshot extraction (`capture_runtime_snapshot`)
- `#[instrument_request]` macro re-export

Notes:

- Some runtime metrics are unavailable without `tokio_unstable` and are recorded as `None`.
- Sampling is periodic and intentionally lightweight in normal use.

## `tailscope-cli`

Responsibilities:

- parse run JSON
- compute request percentiles
- apply diagnosis rules
- output text or JSON report

The analyzer is intentionally rule-based for clarity and debuggability.

## Data contract

All analysis is derived from one `Run` artifact containing:

- metadata
- request events
- stage events
- queue events
- in-flight snapshots
- runtime snapshots

This keeps the diagnosis pipeline reproducible and file-based.

## Diagnostics boundary

`tailscope` diagnoses based on captured evidence; it does not claim causal certainty.

It answers “most likely suspects with supporting signals,” not “proven root cause.”

## Integration pattern

Recommended integration order:

1. initialize one `Tailscope` collector
2. wrap request handlers with `request(...)` (or macro path)
3. instrument high-impact queue waits
4. instrument critical downstream stages
5. optionally start runtime sampling
6. flush run output and analyze with CLI

This keeps instrumentation incremental and practical for existing services.
