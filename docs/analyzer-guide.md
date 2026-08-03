# Analyzer guide: from report to next check

Use this guide when you already have a text or JSON analyzer report. The analyzer ranks plausible bottleneck families from one captured run and proposes targeted next checks. It is a triage tool: a suspect is an investigation lead, not proof of root cause.

Keep these limits in mind:

- A `score` is neither a probability nor a severity value to compare across runs.
- `confidence` expresses confidence in the ranking under the retained evidence, not causal certainty.
- Analyzer tuning cannot repair missing instrumentation, partial observations, or dropped evidence.

## Read the report in this order

1. Start with the global `primary_suspect`. It is the main full-run lead.
2. Read that suspect's `evidence` and `next_checks`.
3. Check its `confidence` and candidate-specific `confidence_notes`.
4. Check report-wide `warnings` and `evidence_quality` for capture and interpretation limits.
5. Inspect `secondary_suspects` when raw scores are close, warnings call the ranking ambiguous, or the primary does not explain the incident.
6. Use `route_breakdowns` and `temporal_segments` only to add context to the global result.
7. Choose one check or mitigation, capture a comparable rerun, and compare the evidence.

## Choose the first investigation

| Suspect kind | Practical interpretation | Verify first | Sensible next investigation | Do not conclude |
| --- | --- | --- | --- | --- |
| `application_queue_saturation` | Requests spent a material part of their observed time waiting in an application queue. | Queue-time share, queue depth, sample coverage, and whether durations are complete or lower bounds. | Inspect admission limits and producer bursts at the named queue; then compare queue wait after one controlled capacity or concurrency change. | That the queue implementation is proven faulty, or that adding workers must fix it. |
| `blocking_pool_pressure` | Retained runtime samples show pressure in Tokio's blocking-work queue. | Blocking queue depth, its persistence across samples, and runtime-field coverage. | Audit `spawn_blocking` and other synchronous hot-path work for long CPU or I/O operations. | That a blocking-looking stage name alone proves blocking-pool exhaustion. |
| `executor_pressure_suspected` | Runnable async work appears to be waiting for Tokio workers. | Runnable queue evidence, worker-count/local-depth coverage, and the suspect's `confidence_notes`. | Check for long polls without yielding and uneven task fanout; use stage timings to narrow the overloaded work. | That high task counts alone prove executor starvation, or that worker normalization is exact when inputs are incomplete. |
| `downstream_stage_dominates` | One instrumented stage contributes a large share of retained request or tail latency. | The named stage's request coverage, tail contribution, retries, and complete-versus-partial durations. | Inspect the dependency behind that stage and correlate its timings and retry behavior with the same window. | That the downstream service is proven to be the root cause rather than a caller, retry, or attribution effect. |
| `insufficient_evidence` | The run cannot support a stronger bottleneck-family ranking. | Which request, queue, stage, runtime, or in-flight signals are missing, sparse, or truncated. | Improve the capture around critical awaits and enable relevant runtime sampling, then rerun. | That the service had no bottleneck. |

## Interpret ranking and evidence limits

`score` ranks evidence strength **within the current report**. Final `confidence` can be reduced when the evidence supporting a candidate is limited. The visible order uses final confidence first, then unchanged raw score, then a stable suspect-kind tie order. Ambiguity, however, is determined by close raw scores. Therefore, a lower-score candidate can appear first when it retains stronger final confidence, and secondary suspects remain useful alternatives rather than discarded explanations.

Use the limitation fields at the right scope:

- `confidence_notes` explain why one particular suspect's confidence was limited, such as ambiguity, partial evidence, or incomplete executor metrics.
- `warnings` give report-wide cautions, including truncation, close-score ambiguity, sparse coverage, or approximate attribution.
- `evidence_quality` gives structured coverage, completeness, drop counters, overall quality, and limitations for the retained capture.

Truncation or weak evidence should usually lead to a better capture before a strong operational conclusion. Partial queue and stage durations are observed lower bounds: tailtriage observed the helper only from first poll until Drop. Drop does not prove that the underlying operation completed, failed, or stopped.

### Executor worker evidence

Complete, consistent worker evidence lets the analyzer interpret runnable queue pressure per Tokio worker. Historical artifacts without worker counts, or captures with ambiguous worker evidence, use a conservative compatibility path. Missing local queue depth can make normalized runnable pressure a lower bound. Read an executor suspect's `confidence_notes` before treating executor pressure as a strong lead.

## Use route and temporal context carefully

The global `primary_suspect` remains the main full-run lead. `route_breakdowns` use request-attributed request, queue, and stage evidence; they intentionally exclude global runtime and in-flight evidence. `temporal_segments` are conservative hints about changes within the run. Runtime and in-flight attribution filtered by timestamps can be sparse or approximate, especially when segment windows overlap.

Use these sections to decide where to inspect next, not as independent proof of a route-specific or phase-specific root cause.

## Worked interpretation

Suppose `downstream_stage_dominates` is primary with high confidence, while a higher-score `application_queue_saturation` suspect is secondary with medium confidence and a note that its partial queue durations are lower bounds. Start with the downstream stage because it retained stronger final confidence. Keep queueing as a live alternative, inspect report warnings for capture gaps, and run one downstream check. This ordering does not prove that the downstream is causal; a better queue capture may change the next report.

## Compare one rerun

1. Choose one `next_checks` item or one mitigation.
2. Keep workload and capture settings as comparable as practical.
3. Change one meaningful variable.
4. Capture and analyze again.
5. Compare latency, supporting evidence, confidence, warnings, `evidence_quality`, and suspect movement.
6. Do not compare scores across unrelated workloads as universal severity values.

A useful movement after a controlled change strengthens or weakens a lead; it still does not turn the report into causal proof.

## Go deeper

- For complete report behavior and field interpretation, see the [detailed diagnostics guide](diagnostics.md).
- For the design reasons, tradeoffs, proof owners, and revision criteria behind major analyzer policies, see the [analyzer rationale catalog](analyzer-rationale.md).
- For capture choice, truncation, limits, and weak-signal troubleshooting, see the [operations guide](operations.md).
- For analyzer options, run `tailtriage analyze --help-analyzer-options` and see [`examples/analyzer-config.toml`](../examples/analyzer-config.toml).
- For saved-artifact loading and output behavior, see the [`tailtriage-cli` README](../tailtriage-cli/README.md).
- For typed in-process analysis and rendering, see the [`tailtriage-analyzer` README](../tailtriage-analyzer/README.md).
