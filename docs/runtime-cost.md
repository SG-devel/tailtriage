# Runtime cost measurement

This document defines the reproducible runtime-cost measurement path for `tailscope` MVP modes.

## What is measured

For each mode (`baseline`, `light`, `investigation`) the harness reports:

- throughput (requests/second)
- end-to-end latency p50
- end-to-end latency p95
- end-to-end latency p99

The summary also computes relative overhead for `light` and `investigation` vs `baseline`.

## Harness

- Binary: `demos/runtime_cost`
- Canonical script: `python3 scripts/measure_runtime_cost.py`

The harness runs a queueing workload with bounded concurrency and fixed simulated work per request.

Mode behavior:

- `baseline`: no `tailscope` instrumentation
- `light`: request + queue + stage + inflight instrumentation
- `investigation`: same as light, plus an additional stage marker and dense Tokio runtime sampling

## Reproduce

From repo root:

```bash
python3 scripts/measure_runtime_cost.py
```

Optional tuning via environment variables:

- `REQUESTS` (default `1200`)
- `CONCURRENCY` (default `48`)
- `WORK_MS` (default `3`)
- `ITERATIONS` (default `5`)

Artifacts written to `demos/runtime_cost/artifacts/`:

- `runtime-cost-raw.jsonl`
- `runtime-cost-summary.json`
- per-mode run JSON files for instrumented modes

## Regenerating current numbers

This document intentionally does **not** pin “latest local run” numeric claims, because runtime-cost values are machine- and workload-dependent.

To generate fresh summary values on your machine, run:

```bash
python3 scripts/measure_runtime_cost.py
```

Then inspect:

- `demos/runtime_cost/artifacts/runtime-cost-summary.json`

If you need to compare runs over time, archive selected summaries under a tracked fixtures path (for example, `demos/runtime_cost/fixtures/`) and cite those committed files in docs/PRs.

## Artifact policy

- `demos/runtime_cost/artifacts/` is **generated at runtime** and intentionally untracked.
- `demos/*/fixtures/` is the **tracked snapshot** area for reproducible, reviewable fixtures.

Use `artifacts/` for local/regenerated outputs and `fixtures/` for intentionally versioned reference snapshots.

## Notes and limits

- Results are workload- and machine-specific.
- Runtime-cost claims should cite either:
  - fresh output generated via `python3 scripts/measure_runtime_cost.py`, or
  - a committed fixture snapshot under `demos/*/fixtures/`.
