#!/usr/bin/env python3
"""Lightweight tests for Python demo tooling and argument parsing."""

from __future__ import annotations

import subprocess
import sys
import unittest
from unittest.mock import patch
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPTS_DIR = REPO_ROOT / "scripts"

sys.path.insert(0, str(SCRIPTS_DIR))

import demo_tool  # noqa: E402
from demo_tool import has_suspect_kind, parse_args  # noqa: E402


class DemoWrapperTests(unittest.TestCase):
    def test_demo_tool_help_runs(self) -> None:
        completed = subprocess.run(
            [sys.executable, str(SCRIPTS_DIR / "demo_tool.py"), "--help"],
            cwd=REPO_ROOT,
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(
            completed.returncode,
            0,
            msg=f"demo_tool.py help failed: {completed.stderr}",
        )
        self.assertIn("usage:", completed.stdout)

    def test_parse_args_accepts_mixed_scenario(self) -> None:
        args = parse_args(["run", "mixed", "baseline"])
        self.assertEqual(args.command, "run")
        self.assertEqual(args.scenario, "mixed")
        self.assertEqual(args.mode, "baseline")

    def test_parse_args_accepts_cold_start_scenario(self) -> None:
        args = parse_args(["validate", "cold-start"])
        self.assertEqual(args.command, "validate")
        self.assertEqual(args.scenario, "cold-start")


    def test_parse_args_accepts_db_pool_scenario(self) -> None:
        args = parse_args(["run", "db-pool", "mitigated"])
        self.assertEqual(args.command, "run")
        self.assertEqual(args.scenario, "db-pool")
        self.assertEqual(args.mode, "mitigated")

    def test_parse_args_accepts_downstream_artifact_flag(self) -> None:
        args = parse_args(["run", "downstream", "--artifact-path", "custom-run.json"])
        self.assertEqual(args.command, "run")
        self.assertEqual(args.scenario, "downstream")
        self.assertEqual(args.artifact_path, "custom-run.json")

    def test_parse_args_accepts_retry_storm_scenario(self) -> None:
        args = parse_args(["validate", "retry-storm"])
        self.assertEqual(args.command, "validate")
        self.assertEqual(args.scenario, "retry-storm")

    def test_has_suspect_kind_handles_missing_primary(self) -> None:
        report = {
            "secondary_suspects": [{"kind": "downstream_stage_dominates"}],
        }

        self.assertTrue(has_suspect_kind(report, {"downstream_stage_dominates"}))
        self.assertFalse(has_suspect_kind(report, {"application_queue_saturation"}))

    def test_has_suspect_kind_checks_primary_and_secondary(self) -> None:
        report = {
            "primary_suspect": {"kind": "application_queue_saturation"},
            "secondary_suspects": [{"kind": "downstream_stage_dominates"}],
        }

        self.assertTrue(has_suspect_kind(report, {"application_queue_saturation"}))
        self.assertTrue(has_suspect_kind(report, {"downstream_stage_dominates"}))
        self.assertFalse(has_suspect_kind(report, {"blocking_pool_pressure"}))


class DemoMainRoutingTests(unittest.TestCase):
    @patch("demo_tool.repo_root", return_value=Path("/tmp/tailscope"))
    @patch("demo_tool.run_scenario_queue")
    def test_main_run_queue_baseline_dispatches_queue_scenario(
        self,
        run_scenario_queue_mock,
        _repo_root_mock,
    ) -> None:
        demo_tool.main(["run", "queue", "baseline"])

        run_scenario_queue_mock.assert_called_once_with(Path("/tmp/tailscope"), "baseline")

    @patch("demo_tool.repo_root", return_value=Path("/tmp/tailscope"))
    @patch("demo_tool.validate_mixed")
    def test_main_validate_mixed_dispatches_validate_mixed(
        self,
        validate_mixed_mock,
        _repo_root_mock,
    ) -> None:
        demo_tool.main(["validate", "mixed"])

        validate_mixed_mock.assert_called_once_with(Path("/tmp/tailscope"))

    @patch("demo_tool.repo_root", return_value=Path("/tmp/tailscope"))
    @patch("demo_tool.run_scenario_downstream")
    def test_main_run_downstream_rejects_non_default_mode(
        self,
        run_scenario_downstream_mock,
        _repo_root_mock,
    ) -> None:
        with self.assertRaisesRegex(
            SystemExit,
            "downstream scenario does not accept mode; use --artifact-path if needed",
        ):
            demo_tool.main(["run", "downstream", "baseline"])

        run_scenario_downstream_mock.assert_not_called()


if __name__ == "__main__":
    unittest.main()
