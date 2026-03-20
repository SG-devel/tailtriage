# v0.1 release gates and launch sequence

This document is the launch-day source of truth for `tailtriage` v0.1.

Use it as a short decision checklist so release steps are not ad hoc.

## Guiding principle

- **Public repo** = first impression
- **Crates publish** = durable public artifact
- **Announcement** = amplification after artifact quality is verified

## Default launch order

1. Internal polish
2. Repo public
3. Crates publish
4. docs.rs verify
5. Announcement

Do not skip forward: each step is blocked on the gate directly before it.

---

## Gate 1: repo-public readiness

Turn the GitHub repository public only when all items are true:

- README opening is clearly positioned around Tokio tail-latency triage.
- No public-facing docs use local path dependencies as the primary install path.
- At least one minimal public example exists and runs.
- One before/after diagnosis workflow is documented.
- CI is green on the branch that will become public.
- Secrets, Actions logs/artifacts, and repository settings were reviewed for public exposure.

For exact repository operations behind this gate (branch protection, required checks, labels, topics, and verification steps), follow [`docs/github-repo-ops.md`](github-repo-ops.md).

### If the repo becomes public in a noisy/misleading state

Mitigate immediately:

1. Fix highest-visibility issues first (README opening, install path, broken example links).
2. If needed, temporarily pause external promotion until corrections are merged.
3. Add a short changelog note documenting what was corrected and when.
4. Re-run the repo-public gate before moving to crates publish.

---

## Gate 2: crates publish readiness

Publish crates only when all items are true:

- Repo is already public.
- Package names are final enough for permanent first-public versions.
- Crate metadata is complete (`description`, `license`, `repository`, `documentation`).
- Local publish checks are done (`cargo publish --dry-run` per publishable crate).
- README install instructions match package names and intended versions.
- Team is ready for docs.rs to become the public docs surface right after publish.

### If a bad crate version is published

Mitigate immediately:

1. Stop announcing and stop tagging additional release material.
2. Publish a corrected patch/minor version (do not rely on overwrite, which is not possible).
3. Mark the bad version as deprecated/yanked when appropriate.
4. Update README/install snippets and changelog to point users to the corrected version.

---

## Gate 3: announcement readiness

Announce only when all items are true:

- Repo is public.
- Crates are published.
- docs.rs builds succeeded for published crates.
- The "3-minute try-it" path works end-to-end from public instructions.
- One crisp positioning statement and one tiny example are ready to post.

### If announcement links are broken or docs.rs failed

Mitigate immediately:

1. Fix broken link paths in README/docs/social post drafts.
2. If docs.rs failed, fix crate docs/build metadata and republish a corrected version.
3. Re-verify the try-it path from a clean environment.
4. Resume announcement only after gate checks pass again.

---

## Launch-day quick checklist

- [ ] Gate 1 (repo-public readiness) is fully green.
- [ ] Repository visibility switched to public.
- [ ] Gate 2 (crates publish readiness) is fully green.
- [ ] Crates published.
- [ ] docs.rs pages verified.
- [ ] Gate 3 (announcement readiness) is fully green.
- [ ] Announcement posted with working links and tiny example.

Status rule: if any box is unchecked, do not proceed to the next phase.
