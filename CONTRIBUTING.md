# Contributing to tailtriage

Thanks for helping improve `tailtriage`.

## What this project is

`tailtriage` is a Rust toolkit for **Tokio tail-latency triage**.

The project focuses on producing **evidence-ranked suspects** and **next checks** from one run artifact. Suspects are leads, not proof of root cause.

## Community and security policies

- Please follow the project [Code of Conduct](CODE_OF_CONDUCT.md).
- Commits must be signed (`--signoff` option).
- Pull requests are merged using **Squash and merge** to keep project history clean and readable.
- For security vulnerabilities, follow the private reporting instructions in [SECURITY.md](SECURITY.md) and avoid opening public issues before a fix is available.

## License for contributions

By submitting a contribution to this repository, you agree that your contribution is licensed under the repository's MIT License.

You must have the right to submit the code, documentation, tests, examples, fixtures, and any other material you contribute.

Do not submit material that you cannot license under MIT.

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

If behavior or user workflows change, update the relevant public docs. Common files to check:

- README.md
- docs/README.md
- docs/user-guide.md
- docs/diagnostics.md
- demos/README.md

## Pull request checklist

- [ ] Change is scoped and explained.
- [ ] Tests updated/added where needed.
- [ ] `cargo fmt`, `cargo clippy`, and `cargo test` pass.
- [ ] Public docs reflect behavior changes.
- [ ] Claims remain evidence-based and within MVP scope.
- [ ] I have the right to submit this contribution under the MIT License, and I agree that this contribution is licensed under the repository's MIT License.
