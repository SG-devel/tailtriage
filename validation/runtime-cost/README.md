# Runtime-cost operational validation domain

This directory is the operational validation domain for runtime-cost checks.

- User-facing guidance: `docs/runtime-cost.md`
- Runner: `scripts/run_operational_validation.py` (`--domain runtime-cost`)

Generated outputs are written under `target/operational-validation/` and are not committed by default.

Runtime-cost numbers are machine/workload/profile scoped for local triage validation and are not universal production guarantees.


Unified orchestration: `scripts/validate_all.py` invokes runtime-cost operational validation in `full` and `publish` profiles while preserving direct domain-runner usage.

This domain now includes native-vs-tracing runtime/instrumentation overhead comparisons using `python3 scripts/measure_runtime_cost.py`. Results remain directional and machine/workload/profile scoped. CI wiring is intentionally deferred; future CI should use broad catastrophic-regression checks only.
