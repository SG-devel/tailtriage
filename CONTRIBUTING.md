# Contributing to tailtriage

Thanks for helping improve `tailtriage`.

Please read and follow the [implementation plan](IMPLEMENTATION_PLAN.md) before proposing or submitting changes that affect behavior, API shape, scope, diagnostics, demos, or public documentation. Contributions should stay within the current product direction and scope.

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

## Contributor workflow

1. Open an issue (or comment on an existing one) before large changes.
2. Keep PRs scoped to one problem.
3. Add or update tests with behavior changes.
4. Use fast local iteration checks when useful. They are optional fast feedback, not proof that a change is complete.
5. Before a change is complete, run the required completion gate.

### Fast local iteration

For quick feedback while editing, you may run a narrower subset such as:

```bash
cargo fmt --check
cargo test --workspace
```

### Required completion gate

Completed work must pass the repository baseline:

```bash
cargo fmt --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-targets --all-features --locked
python3 scripts/validate_docs_contracts.py
```

Hosted CI also runs platform- and release-profile-specific checks such as release-profile Cargo checks, docs, dependency policy, demos, and example smoke tests. Those CI additions do not replace the local completion gate above.

## Scope guardrails

Please keep drive-by PRs within the narrow Tokio tail-latency triage product. Prefer demonstrated usefulness, adoption clarity, coherent tightening, and severe correctness, reliability, or security fixes over adjacent platform expansion. In particular, avoid adding:

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
- [ ] The required completion gate passes, or any environment limitation is documented.
- [ ] Public docs reflect behavior changes.
- [ ] Claims remain evidence-based; suspects are framed as evidence-ranked leads, not causal proof.
- [ ] I have the right to submit this contribution under the MIT License, and I agree that this contribution is licensed under the repository's MIT License.
