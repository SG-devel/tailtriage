from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from scripts import check_workflow_security


SHA = "0123456789abcdef0123456789abcdef01234567"


class WorkflowSecurityTests(unittest.TestCase):
    # TT-TEST: S05 primary
    def test_full_sha_is_accepted(self) -> None:
        self.assertEqual(check_workflow_security.workflow_policy_errors(f"uses: owner/action@{SHA}"), [])

    # TT-TEST: support
    def test_trailing_comment_is_accepted(self) -> None:
        self.assertEqual(
            check_workflow_security.workflow_policy_errors(f"- uses: owner/action@{SHA} # v1"), []
        )

    # TT-TEST: S05 secondary
    def test_local_action_is_accepted(self) -> None:
        self.assertEqual(check_workflow_security.workflow_policy_errors("uses: ./actions/check"), [])

    # TT-TEST: S05 primary
    def test_mutable_version_tag_is_rejected(self) -> None:
        self.assertTrue(check_workflow_security.workflow_policy_errors("uses: actions/checkout@v4"))

    # TT-TEST: S05 secondary
    def test_mutable_branch_or_channel_is_rejected(self) -> None:
        for ref in ("main", "stable", "latest"):
            with self.subTest(ref=ref):
                self.assertTrue(
                    check_workflow_security.workflow_policy_errors(f"uses: owner/action@{ref}")
                )

    # TT-TEST: S05 secondary
    def test_short_or_malformed_sha_is_rejected(self) -> None:
        for ref in (SHA[:-1], "g" * 40):
            with self.subTest(ref=ref):
                self.assertTrue(
                    check_workflow_security.workflow_policy_errors(f"uses: owner/action@{ref}")
                )

    # TT-TEST: S05 primary
    def test_exact_cargo_deny_policy_is_accepted(self) -> None:
        text = "tool: cargo-deny@0.20.2\nfallback: none\n"
        self.assertEqual(check_workflow_security.cargo_deny_policy_errors(text), [])

    # TT-TEST: S05 primary
    def test_moving_or_unversioned_cargo_deny_is_rejected(self) -> None:
        for tool in ("cargo-deny", "cargo-deny@latest", "cargo-deny@0.20"):
            with self.subTest(tool=tool):
                self.assertTrue(
                    check_workflow_security.cargo_deny_policy_errors(
                        f"tool: {tool}\nfallback: none\n"
                    )
                )

    # TT-TEST: S05 primary
    def test_cargo_deny_fallback_other_than_none_is_rejected(self) -> None:
        self.assertTrue(
            check_workflow_security.cargo_deny_policy_errors(
                "tool: cargo-deny@0.20.2\nfallback: cargo-binstall\n"
            )
        )

    # TT-TEST: S05 primary
    def test_actual_repository_workflows_pass(self) -> None:
        self.assertEqual(check_workflow_security.check_repository(), [])

    # TT-TEST: support
    def test_repository_scan_checks_yaml_and_yml(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            workflows = Path(tmp_dir)
            (workflows / "one.yaml").write_text("uses: owner/action@main\n", encoding="utf-8")
            ci = workflows / "ci.yml"
            ci.write_text(
                f"uses: owner/action@{SHA}\ntool: cargo-deny@0.20.2\nfallback: none\n",
                encoding="utf-8",
            )
            self.assertTrue(
                check_workflow_security.check_repository(
                    workflows_dir=workflows, ci_workflow=ci
                )
            )


if __name__ == "__main__":
    unittest.main()
