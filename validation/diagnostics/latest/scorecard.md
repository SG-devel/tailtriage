# Diagnostic validation scorecard (deterministic + adversarial)

| Area | Status | Notes |
|---|---|---|
| queue saturation | Initial deterministic + adversarial coverage | includes no-stage-events and low-request humility checks. |
| downstream dominance | Initial deterministic + adversarial coverage | includes no-queue-events and weak-blocking-vs-strong-downstream checks. |
| db/pool wait | Partially validated | covered via db-pool scenario labels; broaden with non-demo corpora later. |
| blocking-pool pressure | Initial deterministic + adversarial coverage | includes blocking-correlated-stage and partial-runtime-field checks. |
| executor pressure | Initial deterministic + adversarial coverage | includes no-runtime-snapshots ambiguity checks. |
| mixed bottlenecks | Initial deterministic adversarial coverage | explicit top-2 checks for mixed and misleading-signal fixtures. |
| insufficient evidence | Initial deterministic adversarial coverage | low-request-count, noise-only, and high-latency-missing-instrumentation cases enforce low-confidence fallback. |
| truncation handling | Initial deterministic adversarial coverage | truncated-artifact adversarial case enforces warning + confidence ceiling. |
| missing instrumentation warnings | Initial deterministic adversarial coverage | queue/stage/runtime missing and optional-runtime-field warnings are explicitly checked. |
| runtime overhead | Measured separately | see `docs/runtime-cost.md`. |
| collector limits | Measured separately | see `docs/collector-limits.md`. |
| repeated-run diagnostic matrix | Manual/local repeated-run validation available | publishable repeated-run outputs are generated locally (JSONL/summary/scorecard) and not committed by default; results are machine/workload scoped. |
| mitigation validation | Manual/local mitigation matrix available | baseline/mitigated controlled demos compare latency and evidence movement; generated outputs are not committed by default |
| real service validation | Planned | add curated real-service anonymized artifacts. |

Deterministic synthetic adversarial cases validate benchmark/report contract behavior and humility checks; they are not real-service validation and do not provide root-cause proof.
