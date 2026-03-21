# Documentation map

Use this page as the single entry point for project documentation.

## Start here

- **Canonical first run (capture -> analyze -> interpret):** [user-guide.md](user-guide.md)
- **How triage analysis works:** [diagnostics.md](diagnostics.md)
- **Before/after proof workflow (secondary):** [../demos/retry_storm_service/fixtures/before-after-comparison.json](../demos/retry_storm_service/fixtures/before-after-comparison.json)

## Core references

- **Architecture and crate responsibilities:** [architecture.md](architecture.md)
- **MVP product contract (Tokio tail-latency triage):** [../SPEC.md](../SPEC.md)
- **Release/polish plan:** [../IMPLEMENTATION_PLAN.md](../IMPLEMENTATION_PLAN.md)
- **v0.1 release decision gates and launch order:** [release-gates-v0.1.md](release-gates-v0.1.md)
- **Public visibility readiness checklist:** [public-readiness-checklist.md](public-readiness-checklist.md)

## Reproducibility and operations

- **Demos and fixture workflow:** [getting-started-demo.md](getting-started-demo.md)
- **Runtime cost measurement:** [runtime-cost.md](runtime-cost.md)
- **Project changelog:** [changelog.md](changelog.md)

## Historical snapshot

- **MVP audit (2026-03-20):** [mvp-audit-2026-03-20.md](mvp-audit-2026-03-20.md)
- **MVP audit (2026-03-19):** [mvp-audit-2026-03-19.md](mvp-audit-2026-03-19.md)

## Documentation conventions

- `demos/*/artifacts/` = generated, untracked outputs.
- `demos/*/fixtures/` = committed reference snapshots used for deterministic validation.
- Suspects are evidence-ranked leads, not causal proof.
- Prefer triage language for product/category descriptions.
