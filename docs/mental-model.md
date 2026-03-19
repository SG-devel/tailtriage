# tailscope mental model for newcomers

This guide is for Rust developers who use Tokio but may not be deep performance engineers.

`tailscope` helps answer one practical question:

> Is tail latency mainly driven by queueing, executor pressure, blocking-pool pressure, or a slow downstream stage?

Use this document as a quick interpretation layer on top of `README.md` quickstart and `docs/diagnostics.md`.

## Who this guide is for

- You can read and write async Rust service code.
- You can add small instrumentation wrappers around important awaits.
- You want clear diagnosis direction without claiming root-cause certainty.

## Core concepts in plain language

### Tail latency

"Tail" latency means the slow end of requests (for example p95/p99), not the average.
A service can have good average latency while still having painful tails.

### Queueing

Time spent waiting to be worked on.
Examples: waiting on a semaphore permit, waiting for a worker queue slot, or waiting behind backlog.

In `tailscope`, queueing often shows up as higher `p95_queue_share_permille`.

### Service time / stage latency

Time spent actively doing work once admitted.
Often represented by stage wrappers around downstream awaits (DB, cache, RPC, etc.).

In `tailscope`, service-heavy paths often show up in `p95_service_share_permille` and stage evidence.

### Executor pressure (Tokio scheduler pressure)

A signal that runnable task load is high enough that scheduling delay may contribute to tails.
This is not a proof by itself; it is an evidence-ranked suspect.

### Blocking-pool pressure

A signal that `spawn_blocking` style work is queued or saturated.
If blocking queue depth remains elevated, tail latency can increase even if async code looks normal.

## How to read a diagnosis report

Start with these fields:

1. `primary_suspect.kind`
2. `primary_suspect.evidence[]`
3. `primary_suspect.next_checks[]`
4. `p95_queue_share_permille` and `p95_service_share_permille`

Interpretation pattern:

- Treat `primary_suspect` as "best current lead," not proven root cause.
- Use evidence lines to pick one focused experiment.
- Re-run workload after one change and compare p95-level fields.

## First checks by suspect kind

### `ApplicationQueueSaturation`

Try first:

- verify admission limits and queue policies
- reduce producer burstiness or cap concurrency at ingress
- add queue depth samples at major wait points

### `BlockingPoolPressure`

Try first:

- audit `spawn_blocking` callsites for long-running work
- move synchronous hot-path work off critical request path
- check whether blocking queue depth trends down after mitigation

### `ExecutorPressureSuspected`

Try first:

- look for long-running polls or missing cooperative yields
- reduce overly broad fan-out that creates scheduler churn
- compare runtime snapshots before/after one targeted change

### `DownstreamStageDominates`

Try first:

- inspect the dominant stage dependency (DB/RPC/cache)
- check timeout/retry behavior under load
- isolate with one synthetic test to verify downstream contribution

### `InsufficientEvidence`

Try first:

- instrument 1-3 important queues and stages
- capture runtime snapshots during the same load window
- rerun with equivalent load shape

## Confidence and limits (important)

`tailscope` provides evidence-ranked suspects, not causal proof.

Keep in mind:

- Partial instrumentation is supported, but confidence can be lower.
- Mixed-cause incidents can produce overlapping signals.
- One run is a starting point; confidence improves with targeted reruns.

## Suggested workflow for newcomers

1. Integrate quickly using one request wrapper (or macro) and a few stage/queue wrappers.
2. Run representative load and export one run JSON.
3. Analyze and read primary suspect evidence.
4. Make one mitigation change only.
5. Re-run, compare p95 metrics and suspect evidence, then iterate.

This keeps diagnosis practical and avoids overfitting conclusions to a single run.
