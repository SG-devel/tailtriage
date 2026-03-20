# Demo guide: what each demo does and what triage it exercises

The demos are intentionally small synthetic services for Tokio tail-latency triage.

Each one answers a specific triage question by producing an artifact that should drive the analyzer toward evidence-ranked suspects. Suspects are leads, not proof.

## Shared ideas across all demos

All service demos follow the same pattern:

1. Parse output path and optional mode (`baseline`/`before`, `mitigated`/`after`).
2. Create artifact output directory.
3. Initialize one `Tailtriage` collector.
4. Generate a deterministic request burst.
5. Wrap requests with `tailtriage.request(...)`.
6. Instrument admission queue and/or stages.
7. Flush to JSON and run CLI analysis.

Shared helper code for this setup lives in `demos/demo_support`:

- `DemoMode` and mode parsing
- common CLI argument parsing
- artifact directory creation
- collector initialization

This keeps demo binaries focused on the triage scenario rather than boilerplate.

## Demo catalog

### `queue_service`

**What happens**

- Requests compete for a limited semaphore (`worker_permit`).
- Baseline has tighter capacity and slower work; mitigated raises capacity and shortens work.

**Triage being exercised**

- Primary suspect should emphasize application queueing.
- Evidence should reference queue depth and queue share.
- Mitigation should reduce queue-dominant evidence and suspect score.

### `blocking_service`

**What happens**

- Requests dispatch to `spawn_blocking` workloads.
- Baseline constrains `max_blocking_threads` and uses longer blocking work.
- Runtime snapshots include a synthetic `blocking_queue_depth` signal.

**Triage being exercised**

- Primary suspect should emphasize blocking-pool pressure.
- Evidence should point at elevated blocking queue depth alongside request timing impact.
- Mitigation should reduce blocking pressure evidence.

### `executor_pressure_service`

**What happens**

- Each request fans out many hot subtasks and does repeated CPU turns with frequent scheduling.
- Baseline uses fewer worker threads and heavier fanout.
- Runtime snapshots capture global/local runnable depth signals.

**Triage being exercised**

- Primary suspect should emphasize executor pressure/runnable backlog.
- Evidence should reference runtime queue depth and scheduling saturation patterns.
- Mitigation should reduce runnable backlog evidence and score.

### `downstream_service`

**What happens**

- Request flow has a tiny local precheck and a consistently slower downstream stage.
- No intentional queue bottleneck is introduced.

**Triage being exercised**

- Primary suspect should emphasize downstream stage dominance.
- Evidence should highlight stage service share (`downstream_call`) rather than admission queueing.

### `mixed_contention_service`

**What happens**

- Requests first queue for worker admission and then call a downstream stage with periodic slow outliers.
- Baseline keeps both contention sources visible.
- Mitigated mode reduces admission queueing while keeping downstream behavior comparable.

**Triage being exercised**

- Ranked suspects should keep both queueing and downstream leads visible.
- Primary suspect can vary by machine, but evidence should justify the rank.
- Mitigation should shift score/rank toward the remaining bottleneck.

### `cold_start_burst_service`

**What happens**

- Early requests pay extra warmup delay in `cold_start_stage` while a burst competes for admission.
- Baseline has larger cold-start cohort and tighter capacity.
- Mitigated mode reduces warmup cohort and increases admission capacity.

**Triage being exercised**

- Analyzer should surface queueing and/or downstream-stage warmup leads with explicit evidence.
- Mitigation should lower p95 and reduce primary suspect score.

## Typical local workflow

```bash
python3 scripts/demo_tool.py run queue
python3 scripts/demo_tool.py validate queue

python3 scripts/demo_tool.py run blocking
python3 scripts/demo_tool.py validate blocking
```

Repeat for the remaining demos (`executor`, `downstream`, `mixed`, `cold-start`).
