# Worked demo: queue pressure

The flagship queue demo is one focused exercise in the capture -> analyze -> next check -> rerun
workflow. It simulates requests waiting for a limited semaphore (`worker_permit`); its baseline uses
tighter capacity and slower work than its mitigated variant.

This controlled scenario is useful for learning how application-level queueing appears in a report.
For other diagnosis shapes, choose a scenario from the [demo index](../demos/README.md).

## Run and inspect the queue scenario

Generate the current baseline and mitigated artifacts:

```bash
python3 scripts/demo_tool.py run queue
```

The command prints the concrete generated artifact paths. Analyze the Run JSON paths it reports,
or use the supported validation command to run and check the scenario contract directly:

```bash
python3 scripts/demo_tool.py validate queue
```

Validation checks the controlled queue scenario without requiring an exact score or latency value.
The bounded expected primary diagnosis family is `application_queue_pressure`, with queue evidence
on the primary suspect.

## What to inspect

In the report, inspect:

1. `primary_suspect.kind`;
2. `p95_queue_share_permille`;
3. queue-depth and queue-share evidence attached to the suspect;
4. warnings, truncation, and `evidence_quality` before trusting the ranking;
5. p95 and evidence movement between baseline and mitigated runs.

A suspect score ranks candidates inside one report. It is not an absolute severity scale across
runs, so a useful mitigation can leave a score unchanged even while latency and queue evidence
move. Compare the underlying evidence and distributions rather than requiring a score decrease.

## What this exercise establishes

The checked result establishes the repository's expected diagnosis family for this deterministic,
simplified queue workload. It demonstrates a practical next check: change queue capacity/work and
rerun under comparable conditions.

It does not prove that every production queue topology behaves the same way, that score movement is
causal, or that a matching production suspect is root-cause proof. Generated files under demo
`artifacts/` directories are untracked; committed fixtures are deterministic references.
