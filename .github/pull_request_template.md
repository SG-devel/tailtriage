## Summary

-

## Why this change

-

## Validation

Required completion gate:

- [ ] `cargo fmt --check`
- [ ] `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
- [ ] `cargo test --workspace --all-targets --all-features --locked`
- [ ] `python3 scripts/validate_docs_contracts.py`

## Scope check

- [ ] This PR preserves the narrow Tokio tail-latency triage product scope and does not add adjacent platform scope or a competing product story.
- [ ] If behavior changed, docs were updated.
- [ ] Suspects are still described as evidence-ranked leads, not causal proof.

## Contribution license check

- [ ] I have the right to submit this contribution under the MIT License.
- [ ] I agree that this contribution is licensed under the repository's MIT License.
