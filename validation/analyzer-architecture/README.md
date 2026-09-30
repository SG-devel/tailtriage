# Analyzer architecture suites

This directory owns deterministic schema-v2 `Run` definitions for analyzer architecture work. The
manifest format is `tailtriage.analyzer-architecture-suite.v1` and records every input hash.

The **visible sentinels** are inspectable architecture and regression cases. They expose semantic
projections, but are not coefficient-training truth and do not freeze incidental scores. Run them
with `scripts/analyzer_architecture_evidence.py run-sentinels` and verify them with
`verify-sentinels`; generated evidence stays below ignored `target/` and is never committed.

The **locked challenges** are a calibration holdout. Their reviewable inputs and hashes were frozen
by Prompt 13, but analyzer outputs must remain unseen until Prompt 34. `check-locked-challenges`
validates definitions and exact-byte non-duplication without preparing or running a binary. Future
execution additionally requires `run-locked-challenges --acknowledge-locked-output-inspection`.
After the first output inspection, any definition change requires an explicit audit disposition.
Future formula-development and calibration matrices must not reuse locked input bytes or hashes.

No generated sentinel or challenge Report belongs in this directory. The diagnostic manifest and
goldens retain diagnostic ownership; numeric108 retains independent numeric-sensitivity ownership.
These suites are manual/local validation surfaces, not normal hosted-CI requirements. Their
suspects remain triage leads, not proof of root cause.
