# Runtime-cost operational validation domain

This directory is the operational validation domain for runtime-cost checks.

- User-facing guidance: `docs/runtime-cost.md`
- Runner: `scripts/run_operational_validation.py` (`--domain runtime-cost`)

Generated outputs are written under `target/operational-validation/` and are not committed by default.

Runtime-cost numbers are machine/workload/profile scoped for local triage validation and are not universal production guarantees.


> `scripts/validate_all.py` orchestrates runtime-cost operational validation in full/publish profiles. Direct runtime-cost commands remain the source for domain-specific validation details.
