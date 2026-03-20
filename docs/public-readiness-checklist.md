# Public readiness checklist

Use this checklist before switching repository visibility from private to public.

## 1) Repository metadata and settings

- [ ] Repository description clearly says: "Rust toolkit for Tokio tail-latency triage".
- [ ] Topics are intentional (for example: `rust`, `tokio`, `performance`, `latency`, `diagnostics`).
- [ ] Default branch is correct and protected.
- [ ] Branch protection requires passing CI and blocks force-pushes/deletions.
- [ ] Required status checks reference current workflow names (`CI`, `Python Demo Checks`).
- [ ] Tag and release policy is intentional (no accidental internal tags).

## 2) CI log and artifact hygiene

- [ ] Review recent GitHub Actions logs for secrets, internal hostnames, and private paths.
- [ ] Confirm Actions artifacts do not contain sensitive internals.
- [ ] Confirm workflow output is reproducible and intentionally public-facing.

## 3) Public docs and front page quality

- [ ] README opening clearly states category and target user.
- [ ] README quickstart avoids internal-only path assumptions.
- [ ] README links to at least one concrete before/after workflow artifact.
- [ ] Docs index (`docs/README.md`) points first-time users to an obvious start path.
- [ ] Language keeps suspects as evidence-ranked leads, not proof of root cause.

## 4) Community and contribution surface

- [ ] `LICENSE` is present and intentional.
- [ ] `CONTRIBUTING.md` is present and aligned with MVP scope.
- [ ] Issue templates and PR template are present and intentional.
- [ ] Labels are curated for public use (bug, enhancement, docs, triage, good first issue).

## 5) Launch readiness checks

Run locally before the visibility flip:

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Definition of ready:

- main branch green
- no obvious internal-only assumptions in public docs
- no known sensitive exposure in Actions logs/artifacts
- first-time visitor can understand product category and workflow from repository front page
