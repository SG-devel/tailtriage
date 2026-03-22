# Documentation map

Use this page as the single entry point for project documentation.

## User docs (start here)

- **Try from this repo (source/workspace quickstart):** [user-guide.md#path-a--try-from-this-repo-sourceworkspace](user-guide.md#path-a--try-from-this-repo-sourceworkspace)
- **Adopt in your app (crates.io quickstart):** [user-guide.md#path-b--adopt-in-your-app-cratesio](user-guide.md#path-b--adopt-in-your-app-cratesio)

### First-run and triage workflow

- **How triage analysis works:** [diagnostics.md](diagnostics.md)
- **Demos and fixture workflow:** [getting-started-demo.md](getting-started-demo.md)
- **Framework-based adoption starter (axum):** [../tailtriage-tokio/examples/axum_minimal.rs](../tailtriage-tokio/examples/axum_minimal.rs)
- **Realistic mini-service integration example (adoption confidence):** [../tailtriage-tokio/examples/mini_service_integration.rs](../tailtriage-tokio/examples/mini_service_integration.rs)
- **Before/after proof workflow (secondary):** [../demos/retry_storm_service/fixtures/before-after-comparison.json](../demos/retry_storm_service/fixtures/before-after-comparison.json)
- **Runtime cost measurement:** [runtime-cost.md](runtime-cost.md)

### Product and architecture references

- **Architecture and crate responsibilities:** [architecture.md](architecture.md)
- **MVP product contract (Tokio tail-latency triage):** [../SPEC.md](../SPEC.md)
- **Release/polish plan:** [../IMPLEMENTATION_PLAN.md](../IMPLEMENTATION_PLAN.md)
- **Project changelog:** [changelog.md](changelog.md)

## Maintainer / launch / operational docs

- **v0.1 release decision gates and launch order:** [release-gates-v0.1.md](release-gates-v0.1.md)
- **Public visibility readiness checklist:** [public-readiness-checklist.md](public-readiness-checklist.md)
- **Launch checklist issue template (v0.1):** [launch-checklist-issue-v0.1.md](launch-checklist-issue-v0.1.md)
- **GitHub repository operations:** [github-repo-ops.md](github-repo-ops.md)

### Historical audits and snapshots

- **MVP audit (2026-03-20):** [mvp-audit-2026-03-20.md](mvp-audit-2026-03-20.md)
- **MVP audit (2026-03-19):** [mvp-audit-2026-03-19.md](mvp-audit-2026-03-19.md)

## Documentation conventions

- `demos/*/artifacts/` = generated, untracked outputs.
- `demos/*/fixtures/` = committed reference snapshots used for deterministic validation.
- Suspects are evidence-ranked leads, not causal proof.
- Prefer triage language for product/category descriptions.
