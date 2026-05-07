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
from demo_tool import has_suspect_kind, parse_args, suspect_score  # noqa: E402


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

    def test_parse_args_accepts_downstream_mode(self) -> None:
        args = parse_args(["run", "downstream", "after"])
        self.assertEqual(args.command, "run")
        self.assertEqual(args.scenario, "downstream")
        self.assertEqual(args.mode, "after")

    def test_parse_args_accepts_retry_storm_scenario(self) -> None:
        args = parse_args(["validate", "retry-storm"])
        self.assertEqual(args.command, "validate")
        self.assertEqual(args.scenario, "retry-storm")

    def test_parse_args_accepts_release_shortcut(self) -> None:
        args = parse_args(["validate", "queue", "--release"])
        self.assertEqual(args.profile, "release")

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

    def test_suspect_score_reads_secondary_kind_score(self) -> None:
        report = {
            "primary_suspect": {"kind": "application_queue_saturation", "score": 90},
            "secondary_suspects": [{"kind": "executor_pressure_suspected", "score": 70}],
        }
        self.assertEqual(suspect_score(report, "executor_pressure_suspected"), 70)
        self.assertIsNone(suspect_score(report, "blocking_pool_pressure"))

    def test_contains_blocking_depth_evidence_checks_secondary_suspects(self) -> None:
        report = {
            "primary_suspect": {"kind": "application_queue_saturation", "evidence": []},
            "secondary_suspects": [
                {
                    "kind": "executor_pressure_suspected",
                    "evidence": ["Blocking queue depth p95 is 12 due to contention."],
                }
            ],
        }
        self.assertTrue(demo_tool._contains_blocking_depth_evidence(report))

    @patch("demo_tool.load_report_json")
    @patch("demo_tool.run_scenario_executor")
    def test_validate_executor_requires_executor_primary(
        self,
        _run_scenario_executor_mock,
        load_report_json_mock,
    ) -> None:
        before_report = {
            "primary_suspect": {"kind": "application_queue_saturation", "score": 83, "evidence": []},
            "secondary_suspects": [{"kind": "downstream_stage_dominates", "score": 70, "evidence": []}],
            "p95_latency_us": 31_000,
        }
        after_report = {
            "primary_suspect": {"kind": "application_queue_saturation", "score": 50, "evidence": []},
            "secondary_suspects": [],
            "p95_latency_us": 900,
        }
        load_report_json_mock.side_effect = [before_report, after_report]

        with self.assertRaisesRegex(
            SystemExit,
            "expected executor demo baseline primary suspect",
        ):
            demo_tool.validate_executor(Path("/tmp/tailscope"), profile="release")

    def test_parse_args_accepts_diagnosis_matrix(self) -> None:
        args = parse_args(["diagnosis-matrix", "--scenario", "queue", "--scenario", "executor"])
        self.assertEqual(args.command, "diagnosis-matrix")
        self.assertEqual(args.scenario, ["queue", "executor"])

    def test_queue_score_increase_allowed_with_material_p95_drop_and_nonworsening_queue_evidence(self) -> None:
        before = {
            "primary_suspect": {"kind": "application_queue_saturation", "score": 95},
            "p95_latency_us": 1_000_000,
            "p95_queue_share_permille": 980,
        }
        after = {
            "primary_suspect": {"kind": "application_queue_saturation", "score": 100},
            "p95_latency_us": 40_000,
            "p95_queue_share_permille": 975,
        }
        demo_tool._validate_nonworsening_score_or_explainable_saturation(
            before=before,
            after=after,
            expected_primary_kinds={"application_queue_saturation"},
            scenario="queue",
        )

    def test_queue_score_increase_rejected_when_p95_worsens(self) -> None:
        before = {
            "primary_suspect": {"kind": "application_queue_saturation", "score": 90},
            "p95_latency_us": 40_000,
            "p95_queue_share_permille": 900,
        }
        after = {
            "primary_suspect": {"kind": "application_queue_saturation", "score": 95},
            "p95_latency_us": 45_000,
            "p95_queue_share_permille": 890,
        }
        with self.assertRaisesRegex(SystemExit, "does not materially improve"):
            demo_tool._validate_nonworsening_score_or_explainable_saturation(
                before=before,
                after=after,
                expected_primary_kinds={"application_queue_saturation"},
                scenario="queue",
            )

    def test_queue_score_increase_rejected_when_queue_evidence_worsens(self) -> None:
        before = {
            "primary_suspect": {"kind": "application_queue_saturation", "score": 90},
            "p95_latency_us": 100_000,
            "p95_queue_share_permille": 900,
        }
        after = {
            "primary_suspect": {"kind": "application_queue_saturation", "score": 95},
            "p95_latency_us": 50_000,
            "p95_queue_share_permille": 950,
        }
        with self.assertRaisesRegex(SystemExit, "non-worsening queue evidence"):
            demo_tool._validate_nonworsening_score_or_explainable_saturation(
                before=before,
                after=after,
                expected_primary_kinds={"application_queue_saturation"},
                scenario="queue",
            )

    def test_queue_score_increase_allows_primary_shift_when_queue_share_drops_materially(self) -> None:
        before = {
            "primary_suspect": {"kind": "application_queue_saturation", "score": 95},
            "p95_latency_us": 1_000_000,
            "p95_queue_share_permille": 980,
        }
        after = {
            "primary_suspect": {"kind": "downstream_stage_dominates", "score": 100},
            "p95_latency_us": 40_000,
            "p95_queue_share_permille": 0,
        }
        demo_tool._validate_nonworsening_score_or_explainable_saturation(
            before=before,
            after=after,
            expected_primary_kinds={"application_queue_saturation"},
            scenario="queue",
        )

    def test_downstream_score_increase_rejected_when_kind_shifts(self) -> None:
        before = {
            "primary_suspect": {"kind": "downstream_stage_dominates", "score": 80},
            "p95_latency_us": 100_000,
        }
        after = {
            "primary_suspect": {"kind": "application_queue_saturation", "score": 85},
            "p95_latency_us": 40_000,
        }
        with self.assertRaisesRegex(SystemExit, "expected mitigated downstream primary suspect"):
            demo_tool._validate_nonworsening_score_or_expected_primary(
                before=before,
                after=after,
                expected_primary_kinds={"downstream_stage_dominates"},
                scenario="downstream",
            )

    @patch("demo_tool.load_report_json")
    @patch("demo_tool.run_scenario_downstream")
    def test_validate_downstream_uses_downstream_wording(
        self,
        _run_scenario_downstream_mock,
        load_report_json_mock,
    ) -> None:
        before_report = {
            "primary_suspect": {"kind": "downstream_stage_dominates", "score": 80},
            "p95_latency_us": 100_000,
        }
        after_report = {
            "primary_suspect": {"kind": "application_queue_saturation", "score": 90},
            "p95_latency_us": 40_000,
            "p95_queue_share_permille": 0,
        }
        load_report_json_mock.side_effect = [before_report, after_report]

        with self.assertRaisesRegex(SystemExit, "mitigated downstream primary suspect"):
            demo_tool.validate_downstream(Path("/tmp/tailscope"), profile="dev")


class DemoMainRoutingTests(unittest.TestCase):
    @patch("demo_tool.repo_root", return_value=Path("/tmp/tailscope"))
    @patch("demo_tool.run_scenario_queue")
    def test_main_run_queue_baseline_dispatches_queue_scenario(
        self,
        run_scenario_queue_mock,
        _repo_root_mock,
    ) -> None:
        demo_tool.main(["run", "queue", "baseline"])

        run_scenario_queue_mock.assert_called_once_with(
            Path("/tmp/tailscope"),
            "baseline",
            profile="dev",
        )

    @patch("demo_tool.repo_root", return_value=Path("/tmp/tailscope"))
    @patch("demo_tool.validate_mixed")
    def test_main_validate_mixed_dispatches_validate_mixed(
        self,
        validate_mixed_mock,
        _repo_root_mock,
    ) -> None:
        demo_tool.main(["validate", "mixed"])

        validate_mixed_mock.assert_called_once_with(Path("/tmp/tailscope"), profile="dev")

    @patch("demo_tool.repo_root", return_value=Path("/tmp/tailscope"))
    @patch("demo_tool.run_scenario_downstream")
    def test_main_run_downstream_baseline_dispatches_downstream_scenario(
        self,
        run_scenario_downstream_mock,
        _repo_root_mock,
    ) -> None:
        demo_tool.main(["run", "downstream", "baseline"])
        run_scenario_downstream_mock.assert_called_once_with(
            Path("/tmp/tailscope"),
            "baseline",
            profile="dev",
        )


if __name__ == "__main__":
    unittest.main()
