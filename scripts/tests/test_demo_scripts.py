#!/usr/bin/env python3
"""Lightweight tests for Python demo wrappers and argument parsing."""

from __future__ import annotations

import subprocess
import sys
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPTS_DIR = REPO_ROOT / "scripts"

sys.path.insert(0, str(SCRIPTS_DIR))

from demo_tool import has_suspect_kind, parse_args  # noqa: E402
from run_downstream_demo import build_argv  # noqa: E402


class DemoWrapperTests(unittest.TestCase):
    def test_help_runs_for_all_demo_wrappers(self) -> None:
        wrappers = [
            "run_queue_demo.py",
            "run_blocking_demo.py",
            "run_executor_demo.py",
            "run_downstream_demo.py",
        ]

        for wrapper in wrappers:
            with self.subTest(wrapper=wrapper):
                completed = subprocess.run(
                    [sys.executable, str(SCRIPTS_DIR / wrapper), "--help"],
                    cwd=REPO_ROOT,
                    capture_output=True,
                    text=True,
                    check=False,
                )
                self.assertEqual(
                    completed.returncode,
                    0,
                    msg=f"{wrapper} help failed: {completed.stderr}",
                )
                self.assertIn("usage:", completed.stdout)

    def test_downstream_positional_artifact_path_is_mapped_to_flag(self) -> None:
        argv = build_argv(["custom-run.json"])
        args = parse_args(argv)

        self.assertEqual(args.command, "run")
        self.assertEqual(args.scenario, "downstream")
        self.assertEqual(args.artifact_path, "custom-run.json")

    def test_parse_args_accepts_mixed_scenario(self) -> None:
        args = parse_args(["run", "mixed", "baseline"])
        self.assertEqual(args.command, "run")
        self.assertEqual(args.scenario, "mixed")
        self.assertEqual(args.mode, "baseline")

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


if __name__ == "__main__":
    unittest.main()
