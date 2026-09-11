# Diagnostic validation evidence

This page explains the public evidence behind `tailtriage` diagnosis behavior and how to interpret
it. Validation supports bounded confidence in evidence-ranked suspects and next checks; it does not
turn a suspect into proof of root cause.

## What is validated?

Validation asks whether explicit controlled evidence selects an expected diagnosis family, whether
artifact intake and report contracts remain stable, whether controlled live integrations preserve
their signals, and whether operational measurements expose their limits. Fixture ground truth is
the intent of a controlled scenario—not universal production truth or a calibrated causal
probability.

## Evidence map

| Evidence class | Where and how it runs | What it supports | What it does not establish |
| --- | --- | --- | --- |
| Typed/unit rule tests | Applicable code-changing pull-request CI and `workflow_dispatch`; local Cargo tests | Individual analyzer rules, helpers, and contract behavior | End-to-end production diagnosis by themselves |
| Deterministic committed corpus | Applicable deterministic CI job and local benchmark runner | Bounded analyzer, importer, warning, and Report behavior on committed fixtures | Universal accuracy or production root cause |
| Live controlled demos and parity | Applicable live CI job in release profile; locally through `demo_tool.py` | Capture integration and semantic native/tracing evidence direction on controlled workloads | Byte-identical outputs, universal performance, or production causality |
| Bounded runtime-cost smoke | Applicable operational CI job and `workflow_dispatch` | Regression-oriented cost evidence for one synthetic CI workload | Stable or universal production overhead |
| Bounded collector-limit smoke | Applicable operational CI job and `workflow_dispatch` | Retention, truncation/drop visibility, and onset/resource behavior in the smoke profile | No drops or a safe universal operating range |
| Repeated-run controlled matrix | Manual/local | Stability and top-k behavior across repeated runs on one machine/workload profile | Universal stability |
| Mitigation comparisons | Manual/local | Whether a controlled one-change rerun moves relevant latency and evidence | Formal causal proof |
| Curated real-service validation | Planned; no current execution evidence | A future production-shaped trust check | Current real-service coverage |

Relevant pull requests always run changed-path detection. A Markdown-only pull request can run only
the docs-contract path. A code-affecting pull request can additionally run the Cargo matrix and the
deterministic, live, and operational jobs. `workflow_dispatch` runs all of those paths. `CI
required` only aggregates applicable results; it performs no independent validation. The workflow
has no ordinary push-to-main trigger.

The deterministic benchmark is a real applicable CI gate. CI also smoke-checks the deterministic
scorecard **generator**, but does not normally publish a provenance-rich scorecard as a durable
artifact. Such generated snapshots remain local/manual evidence unless separately archived.

## Interpreting diagnostic metrics

- **Top-1**: the first ranked suspect matches the controlled fixture's expected diagnosis family.
- **Top-2**: that expected family is visible as the primary or first secondary suspect.
- **High-confidence-wrong**: a high-confidence primary does not match the fixture's expected family;
  the benchmark can cap this count.
- **Confidence-bucket summaries**: fixture outcomes grouped by the report's evidence-conditioned
  confidence label. They describe this corpus and are not probability calibration for production.

Report-contract fixtures can validate fields, warnings, evidence, next checks, routes, and temporal
output without running the analyzer. They therefore do not contribute to diagnostic accuracy.

## Inspecting and reproducing evidence

Run the committed deterministic corpus with:

```bash
python3 scripts/diagnostic_benchmark.py \
  --manifest validation/diagnostics/manifest.json \
  --min-top1 0.75 \
  --min-top2 0.90 \
  --max-high-confidence-wrong 0
```

The [diagnostic corpus owner](../validation/diagnostics/README.md) documents manifest fields,
fixture integrity and refresh rules, typed success/failure contracts, generated outputs, and
diagnostics-specific runner mechanics. The [maintainer validation map](dev/VALIDATION.md) documents
GitHub Actions ownership, local orchestration profiles, repeated-run and mitigation procedures,
scorecard provenance, release-readiness boundaries, and planned evidence.

Use [runtime-cost](runtime-cost.md) and [collector-limits](collector-limits.md) for their distinct
measurement questions and reproducibility methods. Use the [demo guide](getting-started-demo.md) for
one worked scenario.

## Interpretation limits

- Suspect score ranks evidence within one report; it is not a probability or an absolute severity
  scale across reports.
- Confidence expresses evidence-conditioned ranking confidence, not causal certainty.
- Missing runtime evidence means executor/blocking-pool evidence was unavailable, not that pressure
  was zero.
- Partial queue/stage durations are observed lower bounds, not proof that the underlying operation
  stopped. Completed distributions exclude them; selected partial evidence can affect warnings,
  evidence quality, and ranking. Tracing intake remains completed-only.
- Truncation makes retained evidence partial. Collector measurement characterizes drop visibility
  and onset; analyzer and deterministic tests separately validate warning and diagnosis downgrade
  behavior.
- Repeated-run and mitigation evidence is machine/workload/profile scoped. Movement can support a
  next check but is not formal causal proof.
- Runtime-cost and collector-limit measurements are synthetic and machine/workload/profile scoped,
  not universal production guarantees.
