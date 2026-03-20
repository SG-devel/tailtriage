# Contributing to tailtriage

Thanks for helping improve `tailtriage`.

## What this project is

`tailtriage` is a Rust toolkit for **Tokio tail-latency triage**.

The project focuses on producing **evidence-ranked suspects** and **next checks** from one run artifact. Suspects are leads, not proof of root cause.

## Fast contributor workflow

1. Open an issue (or comment on an existing one) before large changes.
2. Keep PRs scoped to one problem.
3. Add or update tests with behavior changes.
4. Run local checks before pushing:

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Scope guardrails

Please do not expand MVP scope in drive-by PRs. In particular, avoid adding:

- observability backends/exporters
- distributed tracing backends
- non-Tokio runtime support
- GUI/web UI
- ML/statistical auto-diagnosis systems

## Docs updates expected

If behavior or user workflows change, update:

- `README.md`
- `SPEC.md`
- `IMPLEMENTATION_PLAN.md` (if roadmap/milestone expectations shift)

## Pull request checklist

- [ ] Change is scoped and explained.
- [ ] Tests updated/added where needed.
- [ ] `cargo fmt`, `cargo clippy`, and `cargo test` pass.
- [ ] Public docs reflect behavior changes.
- [ ] Claims remain evidence-based and within MVP scope.
