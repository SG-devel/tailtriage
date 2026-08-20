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

    # TT-TEST: support
    def test_local_action_is_accepted(self) -> None:
        self.assertEqual(check_workflow_security.workflow_policy_errors("uses: ./actions/check"), [])

    # TT-TEST: S05 primary
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

    # TT-TEST: S05 primary
    def test_exact_cargo_deny_policy_is_accepted(self) -> None:
        text = f"jobs:\n  check:\n    steps:\n      - uses: taiki-e/install-action@{SHA}\n        with:\n          tool: cargo-deny@0.20.2\n          fallback: none\n"
        self.assertEqual(check_workflow_security.cargo_deny_policy_errors(text), [])

    # TT-TEST: S05 primary
    def test_named_action_step_layout_is_accepted(self) -> None:
        text = f"jobs:\n  check:\n    steps:\n      - name: Install\n        uses: taiki-e/install-action@{SHA}\n        with:\n          tool: cargo-deny@0.20.2\n          fallback: none\n"
        self.assertEqual(check_workflow_security.cargo_deny_policy_errors(text), [])

    # TT-TEST: support
    def test_moving_or_unversioned_cargo_deny_is_rejected(self) -> None:
        for tool in ("cargo-deny", "cargo-deny@latest", "cargo-deny@0.20"):
            with self.subTest(tool=tool):
                self.assertTrue(
                    check_workflow_security.cargo_deny_policy_errors(
                        f"- uses: taiki-e/install-action@{SHA}\n  with:\n    tool: {tool}\n    fallback: none\n"
                    )
                )

    # TT-TEST: support
    def test_cargo_deny_fallback_other_than_none_is_rejected(self) -> None:
        self.assertTrue(
            check_workflow_security.cargo_deny_policy_errors(
                f"- uses: taiki-e/install-action@{SHA}\n  with:\n    tool: cargo-deny@0.20.2\n    fallback: cargo-binstall\n"
            )
        )

    # TT-TEST: S05 primary
    def test_actual_repository_workflows_pass(self) -> None:
        self.assertEqual(check_workflow_security.check_repository(), [])

    # TT-TEST: support
    def test_ci_source_policy_accepts_input_free_read_only_explicit_toolchain(self) -> None:
        text = f"on:\n  workflow_dispatch:\npermissions:\n  contents: read\njobs:\n  check:\n    steps:\n      - uses: dtolnay/rust-toolchain@{SHA}\n        with:\n          toolchain: stable\n"
        self.assertEqual(check_workflow_security.ci_source_policy_errors(text), [])

    # TT-TEST: S05 primary
    def test_ci_source_policy_rejects_dispatch_inputs_permissions_and_implicit_toolchain(self) -> None:
        cases = (
            "on:\n  workflow_dispatch:\n    inputs:\n      data: {}\npermissions:\n  contents: read\n    toolchain: stable\n",
            "on:\n  workflow_dispatch:\npermissions:\n  contents: write\n    toolchain: stable\n",
            "on:\n  workflow_dispatch:\npermissions:\n  contents: read\n",
        )
        for text in cases:
            with self.subTest(text=text):
                self.assertTrue(check_workflow_security.ci_source_policy_errors(text))

    # TT-TEST: S05 primary
    def test_additional_and_job_permissions_are_rejected(self) -> None:
        base = f"on:\n  workflow_dispatch:\npermissions:\n  contents: read\n  actions: write\njobs:\n  check:\n    steps:\n      - uses: dtolnay/rust-toolchain@{SHA}\n        with:\n          toolchain: stable\n"
        self.assertTrue(check_workflow_security.ci_source_policy_errors(base))
        self.assertTrue(check_workflow_security.ci_source_policy_errors(base.replace("  actions: write\n", "").replace("  check:\n", "  check:\n    permissions:\n      contents: read\n")))

    # TT-TEST: S05 primary
    def test_dispatch_inputs_with_nonstandard_indentation_are_rejected(self) -> None:
        text = f"on:\n    workflow_dispatch:\n      inputs:\n        value: {{}}\npermissions:\n  contents: read\n- uses: dtolnay/rust-toolchain@{SHA}\n  with:\n    toolchain: stable\n"
        self.assertTrue(check_workflow_security.ci_source_policy_errors(text))

    # TT-TEST: S05 primary
    def test_step_settings_cannot_be_satisfied_elsewhere(self) -> None:
        rust = f"on:\n  workflow_dispatch:\npermissions:\n  contents: read\n- uses: dtolnay/rust-toolchain@{SHA}\n- name: unrelated\n  with:\n    toolchain: stable\n"
        self.assertTrue(check_workflow_security.ci_source_policy_errors(rust))
        deny = f"- uses: taiki-e/install-action@{SHA}\n- name: unrelated\n  with:\n    tool: cargo-deny@0.20.2\n    fallback: none\n"
        self.assertTrue(check_workflow_security.cargo_deny_policy_errors(deny))

    # TT-TEST: S05 primary
    def test_rust_toolchain_env_value_does_not_satisfy_action_input(self) -> None:
        text = f"on:\n  workflow_dispatch:\npermissions:\n  contents: read\n- uses: dtolnay/rust-toolchain@{SHA}\n  env:\n    toolchain: stable\n"
        self.assertTrue(check_workflow_security.ci_source_policy_errors(text))

    # TT-TEST: S05 primary
    def test_cargo_deny_env_values_do_not_satisfy_action_inputs(self) -> None:
        text = f"- uses: taiki-e/install-action@{SHA}\n  env:\n    tool: cargo-deny@0.20.2\n    fallback: none\n"
        self.assertTrue(check_workflow_security.cargo_deny_policy_errors(text))

    # TT-TEST: S05 primary
    def test_non_event_dispatch_names_do_not_satisfy_requirement(self) -> None:
        for text in (
            f"permissions:\n  contents: read\njobs:\n  workflow_dispatch:\n    steps:\n      - uses: dtolnay/rust-toolchain@{SHA}\n        with:\n          toolchain: stable\n",
            f"on:\n  push:\n    workflow_dispatch:\npermissions:\n  contents: read\njobs:\n  check:\n    steps:\n      - uses: dtolnay/rust-toolchain@{SHA}\n        with:\n          toolchain: stable\n",
        ):
            with self.subTest(text=text): self.assertTrue(check_workflow_security.ci_source_policy_errors(text))

    # TT-TEST: S05 primary
    def test_unrelated_inputs_do_not_make_dispatch_invalid(self) -> None:
        text = f"on:\n  workflow_dispatch:\npermissions:\n  contents: read\njobs:\n  check:\n    strategy:\n      matrix:\n        inputs: [one]\n    steps:\n      - uses: dtolnay/rust-toolchain@{SHA}\n        with:\n          toolchain: stable\n"
        self.assertEqual(check_workflow_security.ci_source_policy_errors(text), [])

    # TT-TEST: S05 primary
    def test_matrix_action_data_does_not_satisfy_rust_setup(self) -> None:
        text = f"on:\n  workflow_dispatch:\npermissions:\n  contents: read\njobs:\n  check:\n    strategy:\n      matrix:\n        include:\n          - uses: dtolnay/rust-toolchain@{SHA}\n            with:\n              toolchain: stable\n    steps:\n      - run: cargo check\n"
        self.assertTrue(check_workflow_security.ci_source_policy_errors(text))

    # TT-TEST: S05 primary
    def test_non_step_action_data_does_not_satisfy_cargo_deny_policy(self) -> None:
        text = f"jobs:\n  check:\n    env:\n      uses: taiki-e/install-action@{SHA}\n      with:\n        tool: cargo-deny@0.20.2\n        fallback: none\n    steps:\n      - run: cargo deny check\n"
        self.assertTrue(check_workflow_security.cargo_deny_policy_errors(text))

    # TT-TEST: support
    def test_repository_scan_checks_yaml_and_yml(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            workflows = Path(tmp_dir)
            (workflows / "one.yaml").write_text("uses: owner/action@main\n", encoding="utf-8")
            ci = workflows / "ci.yml"
            ci.write_text(
                f"on:\n  workflow_dispatch:\npermissions:\n  contents: read\nuses: dtolnay/rust-toolchain@{SHA}\n  with:\n    toolchain: stable\n- uses: taiki-e/install-action@{SHA}\n  with:\n    tool: cargo-deny@0.20.2\n    fallback: none\n",
                encoding="utf-8",
            )
            self.assertTrue(
                check_workflow_security.check_repository(
                    workflows_dir=workflows, ci_workflow=ci
                )
            )


if __name__ == "__main__":
    unittest.main()
