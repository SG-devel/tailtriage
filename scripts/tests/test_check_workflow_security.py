from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from scripts import check_workflow_security


SHA = "0123456789abcdef0123456789abcdef01234567"


# TT-INVARIANT: S05 primary
class WorkflowSecurityTests(unittest.TestCase):
    def test_full_sha_is_accepted(self) -> None:
        self.assertEqual(check_workflow_security.workflow_policy_errors(f"uses: owner/action@{SHA}"), [])

    def test_trailing_comment_is_accepted(self) -> None:
        self.assertEqual(
            check_workflow_security.workflow_policy_errors(f"- uses: owner/action@{SHA} # v1"), []
        )

    def test_local_action_is_accepted(self) -> None:
        self.assertEqual(check_workflow_security.workflow_policy_errors("uses: ./actions/check"), [])

    def test_mutable_version_tag_is_rejected(self) -> None:
        self.assertTrue(check_workflow_security.workflow_policy_errors("uses: actions/checkout@v4"))

    def test_mutable_branch_or_channel_is_rejected(self) -> None:
        for ref in ("main", "stable", "latest"):
            with self.subTest(ref=ref):
                self.assertTrue(
                    check_workflow_security.workflow_policy_errors(f"uses: owner/action@{ref}")
                )

    def test_short_or_malformed_sha_is_rejected(self) -> None:
        for ref in (SHA[:-1], "g" * 40):
            with self.subTest(ref=ref):
                self.assertTrue(
                    check_workflow_security.workflow_policy_errors(f"uses: owner/action@{ref}")
                )

    def test_exact_cargo_deny_policy_is_accepted(self) -> None:
        text = "tool: cargo-deny@0.20.2\nfallback: none\n"
        self.assertEqual(check_workflow_security.cargo_deny_policy_errors(text), [])

    def test_moving_or_unversioned_cargo_deny_is_rejected(self) -> None:
        for tool in ("cargo-deny", "cargo-deny@latest", "cargo-deny@0.20"):
            with self.subTest(tool=tool):
                self.assertTrue(
                    check_workflow_security.cargo_deny_policy_errors(
                        f"tool: {tool}\nfallback: none\n"
                    )
                )

    def test_cargo_deny_fallback_other_than_none_is_rejected(self) -> None:
        self.assertTrue(
            check_workflow_security.cargo_deny_policy_errors(
                "tool: cargo-deny@0.20.2\nfallback: cargo-binstall\n"
            )
        )

    def test_actual_repository_workflows_pass(self) -> None:
        self.assertEqual(check_workflow_security.check_repository(), [])

    def test_ci_source_policy_accepts_input_free_read_only_explicit_toolchain(self) -> None:
        text = "on:\n  workflow_dispatch:\npermissions:\n  contents: read\n    toolchain: stable\n"
        self.assertEqual(check_workflow_security.ci_source_policy_errors(text), [])

    def test_ci_source_policy_rejects_dispatch_inputs_permissions_and_implicit_toolchain(self) -> None:
        cases = (
            "on:\n  workflow_dispatch:\n    inputs:\n      data: {}\npermissions:\n  contents: read\n    toolchain: stable\n",
            "on:\n  workflow_dispatch:\npermissions:\n  contents: write\n    toolchain: stable\n",
            "on:\n  workflow_dispatch:\npermissions:\n  contents: read\n",
        )
        for text in cases:
            with self.subTest(text=text):
                self.assertTrue(check_workflow_security.ci_source_policy_errors(text))

    def test_repository_scan_checks_yaml_and_yml(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            workflows = Path(tmp_dir)
            (workflows / "one.yaml").write_text("uses: owner/action@main\n", encoding="utf-8")
            ci = workflows / "ci.yml"
            ci.write_text(
                f"on:\n  workflow_dispatch:\npermissions:\n  contents: read\nuses: owner/action@{SHA}\n    toolchain: stable\ntool: cargo-deny@0.20.2\nfallback: none\n",
                encoding="utf-8",
            )
            self.assertTrue(
                check_workflow_security.check_repository(
                    workflows_dir=workflows, ci_workflow=ci
                )
            )


if __name__ == "__main__":
    unittest.main()
