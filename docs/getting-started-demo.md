# Getting started with demos

Use this guide to run the MVP demos and compare generated outputs against tracked fixtures.

## Demo artifact policy

`tailscope` demo outputs are intentionally split into two classes:

- **Generated outputs (untracked):** `demos/*/artifacts/`.
  - These are local run results produced by scripts.
  - Regenerate any time; do not treat as repository source-of-truth.
- **Reference fixtures (tracked):** `demos/*/fixtures/`.
  - These are committed snapshots used for deterministic validation and documentation.
  - Use these when asserting expected behavior in docs/tests.

## Queue service demo (`demos/queue_service`)

**Scenario:** heavy application queueing before mitigation, reduced queue pressure after mitigation.

**Run:**

```bash
python3 scripts/demo_tool.py run queue
python3 scripts/demo_tool.py validate queue
```

**Key generated artifacts (`artifacts/`):**

- `before-run.json`, `before-analysis.json`
- `after-run.json`, `after-analysis.json`
- `before-after-comparison.json`

**Reference fixtures (`fixtures/`):**

- `before-analysis.json`
- `after-analysis.json`

**Interpretation focus:**

- `primary_suspect.kind`
- `p95_queue_share_permille`
- `p95_service_share_permille`
- `primary_suspect.evidence[]`

## Blocking service demo (`demos/blocking_service`)

**Scenario:** blocking-pool contention before mitigation, lower blocking pressure after mitigation.

**Run:**

```bash
python3 scripts/demo_tool.py run blocking
python3 scripts/demo_tool.py validate blocking
```

**Key generated artifacts (`artifacts/`):**

- `before-run.json`, `before-analysis.json`
- `after-run.json`, `after-analysis.json`
- `before-after-comparison.json`

**Reference fixtures (`fixtures/`):**

- `before-analysis.json`
- `after-analysis.json`

**Interpretation focus:**

- `primary_suspect.kind`
- `p95_queue_share_permille`
- `p95_service_share_permille`
- `primary_suspect.evidence[]`

## Downstream service demo (`demos/downstream_service`)

**Scenario:** latency dominated by a downstream stage.

**Run:**

```bash
python3 scripts/demo_tool.py run downstream
python3 scripts/demo_tool.py validate downstream
```

**Key generated artifacts (`artifacts/`):**

- `downstream-run.json`
- `downstream-analysis.json`

**Reference fixtures (`fixtures/`):**

- `sample-analysis.json`

**Interpretation focus:**

- `primary_suspect.kind`
- `p95_service_share_permille`
- `p95_queue_share_permille`
- `primary_suspect.evidence[]`

## If your local output differs

- Re-run on an otherwise idle machine.
- Confirm demo script success before comparing fields.
- Compare against `fixtures/` first, then inspect local `artifacts/` deltas.
