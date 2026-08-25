from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from scripts import check_workflow_security


SHA = "0123456789abcdef0123456789abcdef01234567"


def ci_source(*, rust_with: str = "        with:\n          toolchain: stable\n", extra: str = "") -> str:
    return f"""name: CI
on:
  pull_request:
  workflow_dispatch:
permissions:
  contents: read
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: dtolnay/rust-toolchain@{SHA}
{rust_with}{extra}"""


def installer_source(with_values: str = "          tool: cargo-deny@0.20.2\n          fallback: none\n") -> str:
    return f"""jobs:
  test:
    steps:
      - uses: taiki-e/install-action@{SHA}
        with:
{with_values}"""


class WorkflowSecurityTests(unittest.TestCase):
    # TT-TEST: support
    def test_full_sha_is_accepted(self) -> None:
        self.assertEqual(check_workflow_security.workflow_policy_errors(f"uses: owner/action@{SHA}"), [])

    # TT-TEST: support
    def test_trailing_comment_is_accepted(self) -> None:
        self.assertEqual(
            check_workflow_security.workflow_policy_errors(f"- uses: owner/action@{SHA} # v1"), []
        )

    # TT-TEST: support
    def test_local_action_is_accepted(self) -> None:
        self.assertEqual(check_workflow_security.workflow_policy_errors("uses: ./actions/check"), [])

    # TT-TEST: support
    def test_mutable_version_tag_is_rejected(self) -> None:
        self.assertTrue(check_workflow_security.workflow_policy_errors("uses: actions/checkout@v4"))

    # TT-TEST: support
    def test_mutable_branch_or_channel_is_rejected(self) -> None:
        for ref in ("main", "stable", "latest"):
            with self.subTest(ref=ref):
                self.assertTrue(
                    check_workflow_security.workflow_policy_errors(f"uses: owner/action@{ref}")
                )

    # TT-TEST: support
    def test_short_or_malformed_sha_is_rejected(self) -> None:
        for ref in (SHA[:-1], "g" * 40):
            with self.subTest(ref=ref):
                self.assertTrue(
                    check_workflow_security.workflow_policy_errors(f"uses: owner/action@{ref}")
                )

    # TT-TEST: support
    def test_exact_cargo_deny_policy_is_accepted(self) -> None:
        self.assertEqual(check_workflow_security.cargo_deny_policy_errors(installer_source()), [])

    # TT-TEST: support
    def test_moving_or_unversioned_cargo_deny_is_rejected(self) -> None:
        for tool in ("cargo-deny", "cargo-deny@latest", "cargo-deny@0.20"):
            with self.subTest(tool=tool):
                self.assertTrue(
                    check_workflow_security.cargo_deny_policy_errors(
                        installer_source(f"          tool: {tool}\n          fallback: none\n")
                    )
                )

    # TT-TEST: support
    def test_cargo_deny_fallback_other_than_none_is_rejected(self) -> None:
        self.assertTrue(
            check_workflow_security.cargo_deny_policy_errors(
                installer_source(
                    "          tool: cargo-deny@0.20.2\n          fallback: cargo-binstall\n"
                )
            )
        )

    # TT-TEST: S05 primary
    def test_actual_repository_workflows_pass(self) -> None:
        self.assertEqual(check_workflow_security.check_repository(), [])

    # TT-TEST: support
    def test_repository_scan_checks_yaml_and_yml(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            workflows = Path(tmp_dir)
            (workflows / "extra.yaml").write_text(f"uses: owner/action@{SHA}\n", encoding="utf-8")
            ci = workflows / "ci.yml"
            ci.write_text(
                ci_source(
                    extra=f"      - uses: taiki-e/install-action@{SHA}\n        with:\n          tool: cargo-deny@0.20.2\n          fallback: none\n"
                ),
                encoding="utf-8",
            )
            errors = check_workflow_security.check_repository(
                workflows_dir=workflows, ci_workflow=ci
            )
            self.assertTrue(any("workflow inventory must be exactly" in error for error in errors))

    # TT-TEST: support
    def test_ci_source_policy_accepts_input_free_read_only_explicit_toolchain(self) -> None:
        self.assertEqual(check_workflow_security.ci_source_policy_errors(ci_source()), [])

    # TT-TEST: support
    def test_ci_source_policy_requires_top_level_input_free_dispatch(self) -> None:
        valid = ci_source()
        invalid = (
            valid.replace("  workflow_dispatch:\n", ""),
            valid.replace("  workflow_dispatch:\n", "    workflow_dispatch:\n"),
            valid.replace("  workflow_dispatch:\n", "  workflow_dispatch:\n    inputs:\n"),
        )
        for source in invalid:
            with self.subTest(source=source):
                self.assertTrue(check_workflow_security.ci_source_policy_errors(source))
        unrelated_inputs = valid.replace("    runs-on:", "    strategy:\n      inputs:\n        fake: true\n    runs-on:")
        self.assertEqual(check_workflow_security.ci_source_policy_errors(unrelated_inputs), [])

    # TT-TEST: support
    def test_ci_source_policy_rejects_permission_escalation(self) -> None:
        valid = ci_source()
        invalid = (
            valid.replace("  contents: read\n", "  contents: read\n  issues: read\n"),
            valid.replace("  contents: read\n", "  contents: write\n"),
            valid.replace("    runs-on:", "    permissions:\n      contents: read\n    runs-on:"),
        )
        for source in invalid:
            with self.subTest(source=source):
                self.assertTrue(check_workflow_security.ci_source_policy_errors(source))

    # TT-TEST: support
    def test_ci_source_policy_requires_stable_toolchain_on_the_action_step(self) -> None:
        unrelated = (
            "        env:\n          toolchain: stable\n",
            "        with:\n          toolchain: nightly\n      - name: unrelated\n        with:\n          toolchain: stable\n",
            "        with:\n          toolchain: nightly\n      strategy:\n        matrix:\n          toolchain: stable\n",
        )
        for rust_with in ("", "        with:\n          toolchain: nightly\n", *unrelated):
            with self.subTest(rust_with=rust_with):
                self.assertTrue(
                    check_workflow_security.ci_source_policy_errors(ci_source(rust_with=rust_with))
                )

    # TT-TEST: support
    def test_cargo_deny_policy_requires_inputs_on_the_installer_step(self) -> None:
        self.assertEqual(check_workflow_security.cargo_deny_policy_errors(installer_source()), [])
        invalid = (
            "          fallback: none\n",
            "          tool: cargo-deny@latest\n          fallback: none\n",
            "          tool: cargo-deny@0.20.2\n",
            "          tool: cargo-deny@0.20.2\n          fallback: cargo-binstall\n",
            "          wrong: value\n      - name: unrelated\n        with:\n          tool: cargo-deny@0.20.2\n          fallback: none\n",
            "          wrong: value\n        env:\n          tool: cargo-deny@0.20.2\n          fallback: none\n",
            "          wrong: value\n      strategy:\n        matrix:\n          tool: cargo-deny@0.20.2\n          fallback: none\n",
        )
        for with_values in invalid:
            with self.subTest(with_values=with_values):
                self.assertTrue(
                    check_workflow_security.cargo_deny_policy_errors(installer_source(with_values))
                )

    # TT-TEST: support
    def test_named_action_step_layout_is_accepted(self) -> None:
        source = f"""on:
  workflow_dispatch:
permissions:
  contents: read
jobs:
  test:
    steps:
      - name: Rust
        uses: dtolnay/rust-toolchain@{SHA}
        with:
          toolchain: stable
      - name: Deny
        uses: taiki-e/install-action@{SHA}
        with:
          tool: cargo-deny@0.20.2
          fallback: none
"""
        self.assertEqual(check_workflow_security.ci_source_policy_errors(source), [])
        self.assertEqual(check_workflow_security.cargo_deny_policy_errors(source), [])


if __name__ == "__main__":
    unittest.main()
