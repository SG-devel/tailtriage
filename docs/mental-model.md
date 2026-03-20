# Mental model

`tailscope` gives a ranked answer to: *what most likely drives tail latency in this run?*

## Four bottleneck families

1. **Application queueing**: work waits before execution.
2. **Blocking-pool pressure**: `spawn_blocking` backlog inflates tails.
3. **Executor pressure**: scheduler contention delays runnable work.
4. **Downstream stage latency**: a dependency dominates request time.

## How to read results

- Treat `primary_suspect` as the best lead, not proof.
- Use `evidence[]` to choose one targeted experiment.
- Re-run and compare p95 shares plus suspect evidence.

## Confidence boundaries

- Partial instrumentation can still be useful.
- Mixed-cause incidents can produce overlapping signals.
- Confidence improves through iterative, controlled reruns.

For field-level details, see [diagnostics.md](diagnostics.md).
