# Runtime-cost operational validation domain

This directory is the operational validation domain for runtime-cost checks.

- User-facing guidance: `docs/runtime-cost.md`
- Runner: `scripts/run_operational_validation.py` (`--domain runtime-cost`)

Generated outputs are written under `target/operational-validation/` and are not committed by default.

Runtime-cost numbers are machine/workload/profile scoped for local triage validation and are not universal production guarantees.
Tracing comparisons in this domain measure tailtriage semantic tracing spans (`tt.*`) and optional Tokio-session runtime sampling; they do not add OTel/OTLP behavior.


Unified orchestration: `scripts/validate_all.py` invokes runtime-cost operational validation in `full` and `publish` profiles while preserving direct domain-runner usage.

CI additionally runs one bounded runtime-cost smoke on Ubuntu extended release using one warmup round plus the measured-round count shown in `.github/workflows/ci.yml`, then runs `scripts/validate_runtime_cost_summary.py`. This CI path validates output shape and broad catastrophic sanity checks only; outputs are validated in-place and are not uploaded by default. Full runtime-cost measurement remains local/developer-run and machine/workload/profile scoped.
