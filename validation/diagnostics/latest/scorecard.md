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
| raw run-artifact analyzer path | Initial deterministic adversarial coverage | selected no-queue/no-stage/no-runtime/low-request/truncation cases execute Run -> `analyze_run()` via CLI on committed raw fixtures. |
| runtime overhead | Manual/local operational validation available | canonical operational domain lives under `validation/runtime-cost/`; machine/workload scoped; generated outputs under `target/operational-validation/` are not committed by default. |
| collector limits | Manual/local operational validation available | canonical operational domain lives under `validation/collector-limits/`; validates visible bounded drops and warning/downgrade behavior. |
| repeated-run diagnostic matrix | Manual/local repeated-run validation available | publishable repeated-run outputs are generated locally (JSONL/summary/scorecard) and not committed by default; results are machine/workload scoped. |
| mitigation validation | Manual/local mitigation matrix available | baseline/mitigated controlled demos compare latency and evidence movement; generated outputs are not committed by default. |
| real service validation | Planned | add curated real-service anonymized artifacts. |

Deterministic synthetic adversarial cases validate benchmark/report contract behavior and humility checks. Deterministic raw run-artifact adversarial cases validate analyzer-path behavior on committed fixtures. Neither track is real-service validation or root-cause proof.

Normal CI runs the deterministic benchmark against the committed diagnostics manifest and fixtures as a required validation gate. Normal CI still does not publish durable scorecards.

## Generated metrics snapshot

Latest committed scorecard does not embed benchmark numbers directly. Generate fresh metrics with `python3 scripts/diagnostic_benchmark.py --manifest validation/diagnostics/manifest.json --output target/diagnostic-benchmark.json` and report them alongside machine/workload context when publishing.

## Versioned/manual scorecards

The committed scorecard is a stable repository note, not an automatically updated latest-run file.

For a versioned or manually requested snapshot, run the `validation-snapshot.yml` workflow. It uploads a generated scorecard plus `benchmark-summary.json` and `environment.json` as a GitHub Actions artifact. Snapshot artifacts include the `tailtriage` package version, git metadata, runner metadata, software/hardware metadata, manifest hash, referenced-artifact hash, thresholds, and deterministic benchmark metrics.

Normal CI does not publish durable diagnostic scorecards and does not auto-overwrite this committed file.
