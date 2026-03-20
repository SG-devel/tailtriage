# GitHub repository operations runbook

Use this runbook to convert public-readiness checklist items into exact, repeatable GitHub operations.

Scope: repository metadata, branch protection, labels, and post-change verification for `tailtriage`.

## 1) Branch protection baseline (required)

Apply these settings to the default branch (`main`) in:
**GitHub → Settings → Branches → Add branch protection rule**.

### Rule target

- **Branch name pattern:** `main`

### Required settings

- [x] **Require a pull request before merging**
  - [x] Require approvals: `1` (or higher if your team policy requires it)
  - [x] Dismiss stale approvals when new commits are pushed
- [x] **Require status checks to pass before merging**
  - [x] Require branches to be up to date before merging
  - [x] Required status checks:
    - `CI`
    - `Python Demo Checks`
- [x] **Block force pushes**
- [x] **Block branch deletion**

### Optional but recommended settings

- [ ] Restrict who can push to matching branches (enable for larger teams)
- [ ] Require conversation resolution before merging

## 2) Canonical labels (required)

Create or normalize labels in:
**GitHub → Issues → Labels**.

Use this canonical set for MVP triage workflows:

| Name | Color | Description |
|---|---|---|
| `bug` | `d73a4a` | Confirmed defect that breaks expected behavior. |
| `enhancement` | `a2eeef` | Improvement to existing behavior or developer UX. |
| `docs` | `0075ca` | Documentation-only change (README, specs, guides, runbooks). |
| `triage` | `fbca04` | Needs evidence review, suspect ranking, or next-check definition. |
| `good first issue` | `7057ff` | Small, well-scoped task suitable for first-time contributors. |

Notes:
- Keep names exact (case and spacing), especially `triage` and `good first issue`.
- Merge or rename near-duplicates (for example, `documentation` → `docs`).

## 3) Repository metadata baseline (required)

Update in:
**GitHub → Settings → General** (description) and the repository home page editor (topics).

### Canonical description text

Set repository description to exactly:

> Rust toolkit for Tokio tail-latency triage

### Canonical topics

Set topics to:

- `rust`
- `tokio`
- `performance`
- `latency`
- `diagnostics`

If you add extra topics, keep them narrowly relevant to Tokio async-service triage.

## 4) Pre-public owner checklist

Complete this checklist before switching repository visibility from private to public:

1. Apply the branch protection rule in Section 1 to `main`.
2. Confirm required status checks are exactly `CI` and `Python Demo Checks`.
3. Normalize labels using Section 2.
4. Set description and topics using Section 3.
5. Verify README opening clearly positions `tailtriage` as a Tokio tail-latency triage toolkit.
6. Review recent Actions logs and artifacts for secrets, private hostnames, and internal-only paths.
7. Confirm `LICENSE` and `CONTRIBUTING.md` are present and intentional.
8. Run local quality gates:

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## 5) Post-change verification steps

Run these checks after changing branch rules, labels, topics, or description.

### A. Branch protection verification

1. Open **Settings → Branches** and confirm the `main` rule is active.
2. Open a test PR from a branch behind `main`:
   - Verify merge is blocked until required checks pass and branch is updated.
3. Confirm both checks appear in required checks:
   - `CI`
   - `Python Demo Checks`
4. Confirm force-push and delete controls are disabled for `main`.

### B. Label verification

1. Open **Issues → Labels**.
2. Confirm all canonical labels exist with exact names and expected colors.
3. Create a draft issue and apply `triage` plus `good first issue` to ensure labels are available in UI flows.

### C. Metadata verification

1. Open repository home page and confirm description text exactly matches the canonical text.
2. Confirm all canonical topics are visible and spelled correctly.
3. Open repository in logged-out/incognito mode to verify public-facing metadata clarity.

### D. Recordkeeping

1. Add a short note in the next PR description or changelog entry:
   - what changed
   - when it changed
   - who verified it
2. If a mismatch is found, fix immediately and re-run this verification section.
