# Runtime-cost operational validation domain

This directory is the operational validation domain for runtime-cost checks.

- User-facing guidance: `docs/runtime-cost.md`
- Runner: `scripts/measure_runtime_cost.py`

Generated outputs are written to the selected `--artifact-dir` and are not committed by default.

Runtime-cost numbers are machine/workload/profile scoped for local triage validation and are not universal production guarantees.
Tracing comparisons in this domain measure tailtriage semantic tracing spans (`tt.*`) and optional Tokio-session runtime sampling; they do not add OTel/OTLP behavior.


Unified orchestration: `scripts/validate_all.py` invokes runtime-cost operational validation in `full` and `publish` profiles while preserving direct domain-runner usage.

Operational validation runs one bounded runtime-cost smoke using one warmup round plus the configured measured-round count; the producer applies its sanity and tracing/native parity checks directly. Runtime-cost CI is a bounded smoke gate with broad tracing/native sanity thresholds. It is not a rigorous benchmark suite and should not be interpreted as stable performance characterization. Outputs are validated in-place and are not uploaded by default. Full runtime-cost measurement remains local/developer-run and machine/workload/profile scoped.
