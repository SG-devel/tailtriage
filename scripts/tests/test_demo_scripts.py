#!/usr/bin/env python3
"""Lightweight tests for Python demo tooling and argument parsing."""

from __future__ import annotations

import json
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPTS_DIR = REPO_ROOT / "scripts"

sys.path.insert(0, str(SCRIPTS_DIR))

import demo_tool  # noqa: E402
import check_demo_fixture_drift  # noqa: E402
import _demo_runner  # noqa: E402
from demo_tool import has_suspect_kind, parse_args, suspect_score  # noqa: E402


class DemoWrapperTests(unittest.TestCase):
    def test_canonical_live_policy_owns_exactly_nine_scenarios(self) -> None:
        expected = {
            "queue", "blocking", "executor", "downstream", "mixed",
            "cold-start", "db-pool", "shared-lock", "retry-storm",
        }
        self.assertEqual(expected, set(demo_tool.LIVE_SCENARIO_POLICIES))
        self.assertEqual(expected, set(demo_tool.SCENARIOS))

    def test_unknown_live_policy_fails_clearly(self) -> None:
        with self.assertRaisesRegex(ValueError, "unsupported live-demo scenario: typo"):
            demo_tool.validate_scenario(REPO_ROOT, "typo")

    @patch("demo_tool._load_and_evaluate")
    @patch("demo_tool._run_scenario")
    def test_mitigation_reporting_preserves_pass_and_failure(self, run_mock, evaluate_mock) -> None:
        def record(scenario, passed):
            return {"scenario": scenario, "policy_passed": passed,
                "failed_expectations": [] if passed else ["queue_share_decreases"],
                "high_confidence_wrong_after": False, "before_primary_kind": "application_queue_saturation",
                "after_primary_kind": "application_queue_saturation", "p95_delta_us": -100}
        evaluate_mock.side_effect = [record("queue", True), record("db-pool", False), record("queue", True), record("db-pool", False)]
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with self.assertRaisesRegex(SystemExit, "mitigation thresholds failed"):
                demo_tool.run_mitigation_report(root, ["queue", "db-pool"], profile="dev",
                    out=root / "runs.jsonl", summary_path=root / "summary.json", scorecard_path=root / "scorecard.md")
            rows = [json.loads(line) for line in (root / "runs.jsonl").read_text().splitlines()]
            summary = json.loads((root / "summary.json").read_text())
            self.assertEqual([True, False], [r["policy_passed"] for r in rows])
            self.assertEqual((2, 1, 1), (summary["total_scenarios"], summary["passed_scenarios"], summary["failed_scenarios"]))
            self.assertIn("queue_share_decreases", summary["per_scenario"]["db-pool"]["failed_expectations"])
            self.assertIn("| db-pool | no |", (root / "scorecard.md").read_text())
            demo_tool.run_mitigation_report(root, ["queue", "db-pool"], profile="dev",
                out=root / "runs2.jsonl", summary_path=root / "summary2.json", scorecard_path=None,
                no_fail_thresholds=True)
        self.assertEqual(4, run_mock.call_count)

    def _report(self, kind, score=80, p95=1000, queue=700, service=950, confidence="medium", evidence=None, secondary=None):
        return {"primary_suspect": {"kind": kind, "score": score, "confidence": confidence, "evidence": evidence or []},
            "secondary_suspects": secondary or [], "p95_latency_us": p95,
            "p95_queue_share_permille": queue, "p95_service_share_permille": service}

    def test_queue_strong_movement_and_ratio_policy(self):
        before=self._report("application_queue_saturation", p95=1000, queue=700)
        after=self._report("application_queue_saturation", score=70, p95=940, queue=600)
        self.assertTrue(demo_tool.evaluate_live_scenario("queue", before, after, min_p95_improvement_ratio=.05)["policy_passed"])
        self.assertIn("p95_improves", demo_tool.evaluate_live_scenario("queue", before, after, min_p95_improvement_ratio=.10)["failed_expectations"])
        after["p95_queue_share_permille"]=700
        self.assertIn("queue_share_decreases", demo_tool.evaluate_live_scenario("queue", before, after)["failed_expectations"])

    def test_target_score_and_high_confidence_wrong_fail_structurally(self):
        for scenario, target in (("queue", "application_queue_saturation"), ("blocking", "blocking_pool_pressure"), ("downstream", "downstream_stage_dominates")):
            evidence=["Blocking queue depth p95 is 10"] if scenario == "blocking" else []
            before=self._report(target, score=70, p95=1000, evidence=evidence)
            after=self._report("executor_pressure_suspected", score=90, p95=500, queue=500, confidence="high",
                evidence=["Blocking queue depth p95 is 5"] if scenario == "blocking" else [])
            result=demo_tool.evaluate_live_scenario(scenario, before, after)
            self.assertFalse(result["policy_passed"])
            self.assertIn("high_confidence_wrong_after", result["failed_expectations"])
            self.assertIn("targeted_score_nonworsening", result["expected_checks"])

    def test_required_queue_and_blocking_evidence_movement(self):
        db_before=self._report("application_queue_saturation", queue=600)
        db_after=self._report("application_queue_saturation", p95=500, queue=600)
        self.assertIn("queue_share_decreases", demo_tool.evaluate_live_scenario("db-pool", db_before, db_after)["failed_expectations"])
        before=self._report("blocking_pool_pressure", evidence=["Blocking queue depth p95 is 10"])
        after=self._report("blocking_pool_pressure", score=20, p95=500, evidence=["Blocking queue depth p95 is 10"])
        self.assertIn("blocking_depth_decreases", demo_tool.evaluate_live_scenario("blocking", before, after)["failed_expectations"])

    def test_executor_and_extended_semantics_remain(self):
        executor=self._report("executor_pressure_suspected", evidence=["Blocking queue depth p95 is 4"])
        result=demo_tool.evaluate_live_scenario("executor", executor, self._report("executor_pressure_suspected", p95=500))
        self.assertIn("no_blocking_evidence", result["failed_expectations"])
        retry=self._report("downstream_stage_dominates", service=950)
        self.assertTrue(demo_tool.evaluate_live_scenario("retry-storm", retry, self._report("downstream_stage_dominates", score=70, p95=500, service=800))["policy_passed"])

    def test_shared_scenario_metadata_owns_all_demo_paths(self) -> None:
        self.assertEqual(set(demo_tool.SCENARIOS), set(_demo_runner.SCENARIOS))
        self.assertIs(demo_tool.SCENARIO_PATHS, _demo_runner.SCENARIOS)
        self.assertIs(demo_tool.scenario_manifest, _demo_runner.scenario_manifest)
        self.assertIs(demo_tool.scenario_artifact_dir, _demo_runner.scenario_artifact_dir)
        for scenario, service_dir in _demo_runner.SCENARIOS.items():
            demo_dir = REPO_ROOT / "demos" / service_dir
            self.assertEqual(
                _demo_runner.scenario_manifest(REPO_ROOT, scenario),
                demo_dir / "Cargo.toml",
            )
            self.assertEqual(
                _demo_runner.scenario_artifact_dir(REPO_ROOT, scenario),
                demo_dir / "artifacts",
            )

    def test_fixture_refresh_owns_only_canonical_contracts(self) -> None:
        owned = [path.as_posix() for path, _ in check_demo_fixture_drift._scenario_specs()]
        self.assertEqual(len(owned), 19)
        self.assertFalse(any("sample-analysis.json" in path for path in owned))
        comparisons = [path for path in owned if path.endswith("before-after-comparison.json")]
        self.assertEqual(
            comparisons,
            ["demos/downstream_service/fixtures/before-after-comparison.json"],
        )

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
        self.assertTrue("blocking queue depth" in demo_tool._evidence_text(report))

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
            "baseline_targeted",
        ):
            demo_tool.validate_executor(Path("/tmp/tailscope"), profile="release")

    def test_parse_args_accepts_diagnosis_matrix(self) -> None:
        args = parse_args(["diagnosis-matrix", "--scenario", "queue", "--scenario", "executor"])
        self.assertEqual(args.command, "diagnosis-matrix")
        self.assertEqual(args.scenario, ["queue", "executor"])

    def test_parse_args_accepts_validate_tracing_parity(self) -> None:
        args = parse_args(["validate-tracing-parity", "queue", "--profile", "dev"])
        self.assertEqual(args.command, "validate-tracing-parity")
        self.assertEqual(args.scenario, "queue")
        self.assertEqual(args.profile, "dev")

    def test_parse_args_accepts_validate_tracing_parity_retry_storm(self) -> None:
        args = parse_args(["validate-tracing-parity", "retry-storm"])
        self.assertEqual(args.command, "validate-tracing-parity")
        self.assertEqual(args.scenario, "retry-storm")

    def test_parse_args_accepts_validate_tracing_parity_blocking(self) -> None:
        args = parse_args(["validate-tracing-parity", "blocking", "--profile", "dev"])
        self.assertEqual(args.command, "validate-tracing-parity")
        self.assertEqual(args.scenario, "blocking")

    def test_parse_args_accepts_validate_tracing_parity_executor(self) -> None:
        args = parse_args(["validate-tracing-parity", "executor", "--profile", "dev"])
        self.assertEqual(args.command, "validate-tracing-parity")
        self.assertEqual(args.scenario, "executor")

    def test_parse_args_accepts_validate_tracing_parity_all(self) -> None:
        args = parse_args(["validate-tracing-parity", "all", "--profile", "dev"])
        self.assertEqual(args.command, "validate-tracing-parity")
        self.assertEqual(args.scenario, "all")

    def test_parse_args_accepts_validate_tracing_retention_parity(self) -> None:
        args = parse_args(["validate-tracing-retention-parity", "--profile", "release"])
        self.assertEqual(args.command, "validate-tracing-retention-parity")
        self.assertEqual(args.profile, "release")

    @patch("demo_tool._require_equal")
    @patch("demo_tool._load_run")
    @patch("demo_tool.run_and_analyze")
    @patch("demo_tool._tracing_parity_config")
    def test_validate_tracing_retention_parity_uses_tiny_limits_three(
        self,
        parity_config_mock,
        run_and_analyze_mock,
        load_run_mock,
        require_equal_mock,
    ) -> None:
        parity_config_mock.return_value = {
            "demo_manifest": Path("/tmp/demo/Cargo.toml"),
            "artifact_dir": Path("/tmp/demo/artifacts"),
        }
        load_run_mock.side_effect = [{}, {}, {}, {}]
        demo_tool.validate_tracing_retention_parity(Path("/tmp/repo"), profile="release")

        calls = run_and_analyze_mock.call_args_list
        self.assertEqual(len(calls), 4)
        for call in calls:
            extra_args = call.kwargs["extra_demo_args"]
            self.assertIn("--mode", extra_args)
            self.assertIn(extra_args[extra_args.index("--mode") + 1], {"light", "investigation"})
            self.assertEqual(extra_args[extra_args.index("--max-requests") + 1], "3")
            self.assertEqual(extra_args[extra_args.index("--max-stages") + 1], "3")
            self.assertEqual(extra_args[extra_args.index("--max-queues") + 1], "3")
        self.assertTrue(require_equal_mock.called)

    @patch("demo_tool.load_report_json")
    @patch("demo_tool.run_and_analyze")
    @patch("demo_tool._tracing_parity_config")
    def test_validate_tracing_parity_non_runtime_artifact_rejects_runtime_snapshot_fabrication(
        self,
        parity_config_mock,
        _run_and_analyze_mock,
        load_report_json_mock,
    ) -> None:
        parity_config_mock.return_value = {
            "demo_manifest": Path("/tmp/demo/Cargo.toml"),
            "artifact_dir": Path("/tmp/demo/artifacts"),
            "route": "/queue-demo",
            "expected_kind": "application_queue_saturation",
            "queues": {"worker_permit"},
            "stages": {"simulated_work"},
            "require_p95_improvement": True,
        }
        report = {
            "request_count": 1,
            "p95_latency_us": 10,
            "primary_suspect": {"kind": "application_queue_saturation", "score": 10},
            "secondary_suspects": [],
        }
        load_report_json_mock.side_effect = [report] * 8
        fake_run = {
            "requests": [{"route": "/queue-demo"}],
            "stages": [{"stage": "simulated_work"}],
            "queues": [{"queue": "worker_permit", "depth_at_start": 1}],
            "runtime_snapshots": [{"global_queue_depth": 1}],
            "metadata": {
                "mode": "light",
                "effective_core_config": {"capture_limits": {"max_requests": 3, "max_stages": 3, "max_queues": 3}},
                "effective_tokio_sampler_config": None,
            },
            "scenario_label": "queue",
            "truncation": {"dropped_requests": 0, "dropped_stages": 0, "dropped_queues": 0, "limits_hit": False},
        }
        with patch("demo_tool._load_run", side_effect=[fake_run] * 8), patch.object(
            Path, "exists", return_value=True
        ):
            with self.assertRaisesRegex(
                SystemExit,
                r"scenario=queue.*instrumentation=tracing.*artifact=before-light-tracing-run\.json.*field=runtime_snapshots.*expected=\[\].*actual=\[\{'global_queue_depth': 1\}\]",
            ):
                demo_tool.validate_tracing_parity(Path("/tmp/repo"), "queue", profile="release")

    @patch("demo_tool.load_report_json")
    @patch("demo_tool._load_run")
    @patch("demo_tool.run_and_analyze")
    @patch("demo_tool._tracing_parity_config")
    def test_validate_tracing_parity_artifacts_include_mode_and_instrumentation(
        self,
        parity_config_mock,
        run_and_analyze_mock,
        load_run_mock,
        load_report_json_mock,
    ) -> None:
        parity_config_mock.return_value = {
            "demo_manifest": Path("/tmp/demo/Cargo.toml"),
            "artifact_dir": Path("/tmp/demo/artifacts"),
            "route": "/queue-demo",
            "expected_kind": "application_queue_saturation",
            "queues": {"worker_permit"},
            "stages": {"simulated_work"},
            "require_p95_improvement": False,
        }
        report = {
            "request_count": 1,
            "p95_latency_us": 10,
            "primary_suspect": {"kind": "application_queue_saturation", "score": 10},
            "secondary_suspects": [],
        }
        load_report_json_mock.side_effect = [report] * 8
        light_run = {
            "requests": [{"route": "/queue-demo"}],
            "stages": [{"stage": "simulated_work"}],
            "queues": [{"queue": "worker_permit", "depth_at_start": 1}],
            "runtime_snapshots": [],
            "metadata": {
                "mode": "light",
                "effective_core_config": {"capture_limits": {"max_requests": 3, "max_stages": 3, "max_queues": 3}},
                "effective_tokio_sampler_config": None,
            },
            "scenario_label": "queue",
            "truncation": {"dropped_requests": 0, "dropped_stages": 0, "dropped_queues": 0, "limits_hit": False},
        }
        investigation_run = {
            **light_run,
            "metadata": {
                **light_run["metadata"],
                "mode": "investigation",
            },
        }
        load_run_mock.side_effect = ([light_run] * 4) + ([investigation_run] * 4)
        with patch.object(Path, "exists", return_value=True):
            demo_tool.validate_tracing_parity(Path("/tmp/repo"), "queue", profile="release")
        artifact_basenames = {call.args[2].name for call in run_and_analyze_mock.call_args_list}
        self.assertEqual(len(artifact_basenames), 8)
        self.assertIn("before-light-native-run.json", artifact_basenames)
        self.assertIn("before-light-tracing-run.json", artifact_basenames)
        self.assertIn("after-investigation-native-run.json", artifact_basenames)
        self.assertIn("after-investigation-tracing-run.json", artifact_basenames)

    @patch("demo_tool.load_report_json")
    @patch("demo_tool._load_run")
    @patch("demo_tool.run_and_analyze")
    @patch("demo_tool._tracing_parity_config")
    def test_validate_tracing_parity_fails_when_capture_mode_metadata_is_wrong(
        self,
        parity_config_mock,
        run_and_analyze_mock,
        load_run_mock,
        load_report_json_mock,
    ) -> None:
        del run_and_analyze_mock
        parity_config_mock.return_value = {
            "demo_manifest": Path("/tmp/demo/Cargo.toml"),
            "artifact_dir": Path("/tmp/demo/artifacts"),
            "route": "/queue-demo",
            "expected_kind": "application_queue_saturation",
            "queues": {"worker_permit"},
            "stages": {"simulated_work"},
            "require_p95_improvement": False,
        }
        report = {
            "request_count": 1,
            "p95_latency_us": 10,
            "primary_suspect": {"kind": "application_queue_saturation", "score": 10},
            "secondary_suspects": [],
        }
        load_report_json_mock.side_effect = [report] * 8
        light_run = {
            "requests": [{"route": "/queue-demo"}],
            "stages": [{"stage": "simulated_work"}],
            "queues": [{"queue": "worker_permit", "depth_at_start": 1}],
            "runtime_snapshots": [],
            "metadata": {
                "mode": "light",
                "effective_core_config": {"capture_limits": {"max_requests": 3, "max_stages": 3, "max_queues": 3}},
                "effective_tokio_sampler_config": None,
            },
            "scenario_label": "queue",
            "truncation": {"dropped_requests": 0, "dropped_stages": 0, "dropped_queues": 0, "limits_hit": False},
        }
        load_run_mock.side_effect = [light_run] * 8
        with patch.object(Path, "exists", return_value=True):
            with self.assertRaisesRegex(
                SystemExit,
                r"scenario=queue.*capture_mode=investigation.*artifact=before-investigation-native-run\.json.*field=metadata\.mode.*expected='investigation'.*actual='light'",
            ):
                demo_tool.validate_tracing_parity(Path("/tmp/repo"), "queue", profile="release")

    @patch("demo_tool.load_report_json")
    @patch("demo_tool._load_run")
    @patch("demo_tool.run_and_analyze")
    @patch("demo_tool._tracing_parity_config")
    def test_validate_tracing_parity_runtime_sensitive_accepts_manual_disabled_sampler_warning(
        self,
        parity_config_mock,
        _run_and_analyze_mock,
        load_run_mock,
        load_report_json_mock,
    ) -> None:
        parity_config_mock.return_value = {
            "demo_manifest": Path("/tmp/demo/Cargo.toml"),
            "artifact_dir": Path("/tmp/demo/artifacts"),
            "route": "/blocking-demo",
            "expected_kind": "blocking_pool_pressure",
            "queues": {"dispatch_overhead"},
            "stages": {"spawn_blocking_path"},
            "require_p95_improvement": False,
        }
        report = {
            "request_count": 1,
            "p95_latency_us": 10,
            "primary_suspect": {"kind": "blocking_pool_pressure", "score": 10},
            "secondary_suspects": [],
        }
        load_report_json_mock.side_effect = [report] * 8
        light_run = {
            "requests": [{"route": "/blocking-demo"}],
            "stages": [{"stage": "spawn_blocking_path"}],
            "queues": [{"queue": "dispatch_overhead", "depth_at_start": 1}],
            "runtime_snapshots": [{"blocking_queue_depth": 1}],
            "metadata": {
                "mode": "light",
                "lifecycle_warnings": [
                    "tailtriage-tracing session ran with background runtime sampling disabled; runtime snapshots rely on manual record_runtime_snapshot(...) calls"
                ],
                "effective_core_config": {"capture_limits": {"max_requests": 3, "max_stages": 3, "max_queues": 3}},
            },
            "scenario_label": "blocking",
            "truncation": {"dropped_requests": 0, "dropped_stages": 0, "dropped_queues": 0, "limits_hit": False},
        }
        investigation_run = {**light_run, "metadata": {**light_run["metadata"], "mode": "investigation"}}
        load_run_mock.side_effect = ([light_run] * 4) + ([investigation_run] * 4)
        with patch.object(Path, "exists", return_value=True):
            demo_tool.validate_tracing_parity(Path("/tmp/repo"), "blocking", profile="release")

    @patch("demo_tool.load_report_json")
    @patch("demo_tool._load_run")
    @patch("demo_tool.run_and_analyze")
    @patch("demo_tool._tracing_parity_config")
    def test_validate_tracing_parity_runtime_sensitive_rejects_sampler_metadata_without_disabled_warning(
        self,
        parity_config_mock,
        _run_and_analyze_mock,
        load_run_mock,
        load_report_json_mock,
    ) -> None:
        parity_config_mock.return_value = {
            "demo_manifest": Path("/tmp/demo/Cargo.toml"),
            "artifact_dir": Path("/tmp/demo/artifacts"),
            "route": "/blocking-demo",
            "expected_kind": "blocking_pool_pressure",
            "queues": {"dispatch_overhead"},
            "stages": {"spawn_blocking_path"},
            "require_p95_improvement": False,
        }
        report = {
            "request_count": 1,
            "p95_latency_us": 10,
            "primary_suspect": {"kind": "blocking_pool_pressure", "score": 10},
            "secondary_suspects": [],
        }
        load_report_json_mock.side_effect = [report] * 8
        light_run = {
            "requests": [{"route": "/blocking-demo"}],
            "stages": [{"stage": "spawn_blocking_path"}],
            "queues": [{"queue": "dispatch_overhead", "depth_at_start": 1}],
            "runtime_snapshots": [{"blocking_queue_depth": 1}],
            "metadata": {
                "mode": "light",
                "lifecycle_warnings": [],
                "effective_tokio_sampler_config": {"interval_ms": 10},
                "effective_core_config": {"capture_limits": {"max_requests": 3, "max_stages": 3, "max_queues": 3}},
            },
            "scenario_label": "blocking",
            "truncation": {"dropped_requests": 0, "dropped_stages": 0, "dropped_queues": 0, "limits_hit": False},
        }
        investigation_run = {**light_run, "metadata": {**light_run["metadata"], "mode": "investigation"}}
        load_run_mock.side_effect = ([light_run] * 4) + ([investigation_run] * 4)
        with patch.object(Path, "exists", return_value=True):
            with self.assertRaisesRegex(
                SystemExit,
                r"expected disabled-background-sampler lifecycle warning in deterministic runtime-sensitive tracing run before-light-tracing-run\.json",
            ):
                demo_tool.validate_tracing_parity(Path("/tmp/repo"), "blocking", profile="release")

    def test_parity_failure_message_contains_scenario_field_expected_actual(self) -> None:
        with self.assertRaisesRegex(
            SystemExit,
            "scenario=queue.*field=metadata.mode.*expected='light'.*actual='investigation'",
        ):
            demo_tool._require_equal(
                scenario="queue",
                instrumentation="native",
                artifact_path="artifact.json",
                field="metadata.mode",
                expected="light",
                actual="investigation",
            )

    @patch("demo_tool.load_report_json")
    @patch("demo_tool.run_scenario_downstream")
    def test_validate_downstream_uses_downstream_context(
        self,
        _run_scenario_downstream_mock,
        load_report_json_mock,
    ) -> None:
        before_report = {
            "primary_suspect": {"kind": "downstream_stage_dominates", "score": 90},
            "secondary_suspects": [],
            "p95_latency_us": 100_000,
        }
        after_report = {
            "primary_suspect": {"kind": "application_queue_saturation", "score": 95, "confidence": "high"},
            "secondary_suspects": [],
            "p95_latency_us": 20_000,
        }
        load_report_json_mock.side_effect = [before_report, after_report]

        with self.assertRaisesRegex(SystemExit, "high_confidence_wrong_after"):
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
    @patch("demo_tool.validate_scenario")
    def test_main_validate_mixed_dispatches_validate_mixed(
        self,
        validate_scenario_mock,
        _repo_root_mock,
    ) -> None:
        demo_tool.main(["validate", "mixed"])

        validate_scenario_mock.assert_called_once_with(
            Path("/tmp/tailscope"), "mixed", profile="dev"
        )

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
