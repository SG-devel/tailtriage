# Collector-limits operational validation domain

This directory is the operational validation domain for collector-limit checks.

- User-facing guidance: `docs/collector-limits.md`
- Runner: `scripts/measure_collector_limits.py`

Generated outputs are written to the selected `--artifact-dir` and are not committed by default.

Collector-limit validation checks bounded/visible drops plus downgrade/warning behavior; it does not claim the collector never drops.


Unified orchestration: `scripts/validate_all.py` invokes collector-limits operational validation in `full` and `publish` profiles while preserving direct domain-runner usage.
