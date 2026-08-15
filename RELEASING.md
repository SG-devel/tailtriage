# Manual release procedure

This is the authoritative procedure for a manual `tailtriage` release. Release tooling is check-only. Crate publication is performed manually by a maintainer.

1. Finalize the intended package version and dated changelog in a separate release-finalization change.
2. Merge that change and select one exact, clean release commit.
3. Record the commit's exact SHA.
4. Run the normal repository completion gate:

   ```bash
   cargo fmt --check
   cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
   cargo test --workspace --all-targets --all-features --locked
   python3 scripts/validate_docs_contracts.py
   ```

5. Run the release validation profile:

   ```bash
   python3 scripts/validate_all.py \
     --profile publish \
     --profile-mode release
   ```

6. Run the release preflight checker (`0.4.0` is an example, not permanent policy):

   ```bash
   VERSION=0.4.0
   python3 scripts/check_release.py --version "$VERSION"
   ```

7. Review the detected packages, publication order, Cargo packaging result, and generated manual commands.
8. Manually publish one package at a time in the generated order.
9. Verify each exact crate version publicly before continuing.
10. Create and push an annotated tag only after every intended package is published. Tag signing is optional unless policy changes later.
11. Create a draft GitHub Release manually.
12. Review and publish the GitHub Release manually.

Repository tooling does not publish crates. Generated publication commands are inert instructions only, and no crates.io credential belongs in repository tooling or GitHub Actions. Tags must not be pushed for an incomplete release. GitHub Release creation and publication remain manual.

## If publication is interrupted

- Verify which exact versions are already public; never republish an existing version.
- Resume from the first unpublished package whose dependencies are public, preserving the generated order.
- Do not tag an incomplete release.
- Treat published packages as immutable.
- Use a corrected patch version when an already published set cannot be completed coherently.
