# Production operations guide

`tailtriage` supports a bounded operational loop:

```text
identify a slow window -> arm a bounded capture -> collect representative traffic
-> stop/finalize -> inspect limitations -> analyze -> choose one next check
-> change one thing -> rerun under comparable conditions
```

The output is a set of evidence-ranked suspects and next checks. Suspects are leads, not proof of
root cause, and comparability or mitigation movement does not establish formal causality.

## 1. Choose the capture model

Use `Tailtriage` for one explicit bounded run whose lifetime the application controls. Its simple
lifecycle is `build -> capture -> shutdown`.

Use `TailtriageController` for repeated bounded windows in a long-lived service. `disable()` closes
the current generation but is reversible; a later `enable()` can create another generation.
`shutdown()` is terminal: its first call permanently prevents future generations and makes new
requests inert. Exact builder, TOML, and reload behavior belongs to the
[controller README](../tailtriage-controller/README.md).

Whichever model you choose, specify a time, traffic, or incident window before enabling capture.
Avoid an unreviewed, open-ended production capture.

## 2. Arm a representative window

Choose a window that includes the slow behavior and enough representative traffic to distinguish
it from startup noise. Record the service version, workload shape, capture mode, analyzer config,
and relevant environment facts so a rerun can be compared honestly.

Start with narrow queue and stage instrumentation around waits that can explain request latency.
Native request-context capture is the simplest choice for a new integration. Tracing intake is
useful when reliable request correlation already exists; its exact span and replay contracts are
owned by the [tracing crate](../tailtriage-tracing/README.md).

## 3. Select capture density

- **`light`** is the conservative default first production choice. Use it to validate signal
  quality with bounded retention and lower capture density.
- **`investigation`** is a denser, still-bounded choice for an active investigation when light-mode
  evidence is insufficient. It is not an always-on telemetry mode.

Capture modes supply defaults; explicit limit overrides may change them. Consult the relevant item
Rustdoc when exact defaults or setter behavior matters rather than copying a defaults table into an
operations plan.

## 4. Decide whether runtime sampling is needed

Runtime sampling is optional. `CaptureMode` and `Tailtriage::builder` do not start it. Starting a
sampler requires an active Tokio runtime, and retained runtime snapshots consume bounded capture
capacity.

Begin without sampling when queue and stage evidence can answer the operational question. Add it
when you need evidence to separate application queueing from executor or blocking-pool pressure.
Missing runtime evidence means that evidence is unavailable; it does **not** mean runtime pressure
was zero. Tracing-only evidence likewise does not fabricate runtime snapshots.

## 5. Confirm effective retention and resource bounds

Inspect the resolved effective configuration recorded for the run instead of assuming requested
values or mode defaults survived configuration resolution unchanged. Bound the window and each
retained evidence family for the expected request rate.

Core request, stage, queue, in-flight, and runtime-snapshot capacities legally permit zero. A zero
capacity can mean that no evidence from that family is retained; it is not a universal validation
error. In particular, do not generalize the CLI requirement for an analyzable persisted request
artifact into a core `CaptureLimits` rule. Exact admission and cardinality behavior belongs to
`CaptureLimits` Rustdoc.

When a limit is reached, inspect `truncation.limits_hit` and the dropped-category counters. They
identify partial retention; they do not imply that uncaptured activity did not occur. Reduce the
window or capture density, or raise only the relevant limit after checking resource impact. The
[collector-limits evidence page](collector-limits.md) explains how to characterize onset and
resource trends.

## 6. Capture, then stop and finalize safely

Stop only after the selected traffic has completed or after you have deliberately accepted partial
lifecycle evidence. Prefer explicit request completion with the application's known outcome.

Dropping an admitted unfinished completion token while capture is open records one `cancelled`
request and resolves that tailtriage lifecycle. It does not prove the underlying external operation
stopped. Consequently, a strict unfinished-work error points instead to work or tokens still alive,
deliberately forgotten/leaked tokens, or cleanup/finalization ordering.

### Direct strict finalization recovery

A direct strict unfinished-request failure is retryable. It occurs before finalization and before
any sink attempt, so admitted work can resolve before `shutdown()` is retried.

A serialization or persistence failure after an eligible sink attempt is different: that shutdown
result is terminal. Later shutdown calls replay the stored failure and do not attempt a second sink
write. Do not collapse this sink contract into unfinished-work recovery.

### Controller generation recovery

For an unfinished strict controller generation, already-admitted requests may drain and the
generation can finalize after the drain. Retry `disable()` or `shutdown()` as appropriate to obtain
the authoritative stored result. A terminal controller shutdown remains terminal even while this
generation-finalization result is being resolved. Controller
`GenerationFinalizationError` and core `ShutdownError` are distinct contracts.

## 7. Handle and inspect the artifact

Run JSON is the complete persisted triage artifact. Completed-span tracing JSONL is a narrower
tracing-source interchange/replay surface, not a complete Run archive. Use Run JSON for an
operational handoff when runtime, in-flight, lifecycle, and truncation context matters.

Run artifacts may contain service, route, stage, queue, environment, and other operational labels
or identifiers. Review and redact them before sharing outside the intended trust boundary;
`tailtriage` does not automatically sanitize artifacts. Apply file-size and input-resource limits
appropriate to the receiving environment. Exhaustive artifact input policy belongs to the
[CLI README](../tailtriage-cli/README.md).

Before interpreting a diagnosis, check:

1. truncation and dropped counters;
2. lifecycle and validation warnings;
3. `evidence_quality` and signal availability;
4. whether runtime evidence was intentionally collected;
5. partial queue or stage events.

Completed queue and stage distributions exclude partial observations. A partial duration is the
observed lower bound from first poll until helper Drop, not proof that the underlying operation
completed, failed, was cancelled, or stopped. Partial evidence can affect warnings, evidence
quality, and ranking when selected. Tracing intake is completed-only.

## 8. Analyze and choose one next check

Use the [analyzer guide](analyzer-guide.md) for a practical report-to-next-check workflow and
[diagnostics](diagnostics.md) for exact scoring, ordering, fallback, and field mechanics.

Operationally:

- suspect scores rank candidates inside one report; they are neither probabilities nor an absolute
  severity scale across runs;
- confidence is conditioned on available evidence, not causal certainty;
- warnings and evidence quality bound how strongly to read the ranking;
- `insufficient_evidence` is analyzer abstention, not proof that no bottleneck exists.

Choose one next check that discriminates among plausible suspects. Examples include reducing one
queue's contention, isolating one slow stage, or adding bounded runtime sampling. Change one thing
rather than tuning multiple capture and service variables together.

## 9. Troubleshoot weak or failed captures

- **Ambiguous or insufficient report:** verify request boundaries and correlation, then add the
  smallest missing queue, stage, or runtime signal. Analyzer tuning cannot repair absent evidence.
- **Unexpectedly large artifact:** shorten the window, use light mode, lower the relevant effective
  limit, or narrow labels. Preserve enough capacity for the evidence family under investigation.
- **Too many runtime snapshots:** lengthen the sampling interval, shorten the window, or lower the
  runtime-snapshot limit. Do not reinterpret absence after that change as zero pressure.
- **Truncation:** treat retained evidence as partial and rerun with a better window/limit balance.
- **Strict finalization failure:** find still-live or forgotten tokens and correct cleanup ordering;
  ordinary open-capture Drop already resolves its lifecycle as cancellation.
- **Tracing mismatch:** verify stable correlation and use Run JSON when complete run context is
  required. Native capture is usually simpler for a new integration.

## 10. Rerun under comparable conditions

Preserve the workload shape, capture mode, effective limits, sampling choice, and analyzer config
unless one of them is the deliberate change. Compare latency distributions, evidence availability,
warnings, suspect ordering, and the evidence behind the selected suspect—not raw score alone.

Movement after a mitigation supports or weakens the chosen next-check hypothesis within this
controlled comparison. It is not formal causal proof. Runtime-cost and collector-limit results are
also machine/workload/profile scoped; use the dedicated [runtime-cost](runtime-cost.md) and
[collector-limits](collector-limits.md) pages rather than treating repository measurements as
universal production guarantees.
