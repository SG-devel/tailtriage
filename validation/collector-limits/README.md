# Collector-limits operational validation domain

This directory is the operational validation domain for collector-limit checks.

- User-facing guidance: `docs/collector-limits.md`
- Runner: `scripts/run_operational_validation.py` (`--domain collector-limits`)

Generated outputs are written under `target/operational-validation/` and are not committed by default.

Collector-limit validation checks bounded/visible drops plus downgrade/warning behavior; it does not claim the collector never drops.

The unified orchestrator (`scripts/validate_all.py`) invokes collector-limit operational validation in full/publish profiles while keeping this direct domain runner available.
