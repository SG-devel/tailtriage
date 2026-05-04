import json
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from scripts import validate_all as va


class ValidateAllTests(unittest.TestCase):
    def args(self, **kw):
        base = {
            "profile": "smoke", "python": "python3", "runs": 1, "profile_mode": "dev", "skip_cargo": False,
            "include_cargo": False, "no_fail_fast": False, "no_fail_thresholds": False,
        }
        base.update(kw)
        return type("Args", (), base)()

    def test_smoke_plan_contains_expected_tracks(self):
        plan = va.build_plan(self.args(profile="smoke"), Path("target/validation/smoke"))
        names = [p.name for p in plan]
        self.assertIn("diagnostic_benchmark", names)
        self.assertIn("validate_docs_contracts", names)
        self.assertIn("diagnostic_matrix_smoke", names)
        self.assertIn("mitigation_smoke", names)
        self.assertIn("runtime_cost_smoke", names)
        self.assertIn("collector_limits_smoke", names)

    def test_ci_plan_contains_tests(self):
        plan = va.build_plan(self.args(profile="ci"), Path("target/validation/ci"))
        names = [p.name for p in plan]
        self.assertIn("test_diagnostic_benchmark", names)
        self.assertIn("test_validate_docs_contracts", names)
        self.assertIn("test_run_diagnostic_matrix", names)

    def test_full_plan_has_runs_and_operational(self):
        plan = va.build_plan(self.args(profile="full", runs=7), Path("x"))
        cmd = next(p for p in plan if p.name == "diagnostic_matrix_full").argv
        self.assertIn("7", cmd)
        self.assertTrue(any(p.name == "operational_all" for p in plan))

    def test_include_skip_cargo_flags(self):
        self.assertFalse(any(p.track == "cargo" for p in va.build_plan(self.args(profile="ci"), Path("x"))))
        self.assertTrue(any(p.track == "cargo" for p in va.build_plan(self.args(profile="ci", include_cargo=True), Path("x"))))
        self.assertFalse(any(p.track == "cargo" for p in va.build_plan(self.args(profile="full", skip_cargo=True), Path("x"))))

    def test_profile_mode_propagates(self):
        plan = va.build_plan(self.args(profile="smoke", profile_mode="release"), Path("x"))
        self.assertTrue(any("release" in p.argv for p in plan if "run_" in p.argv[1]))

    def test_summary_and_scorecard_and_jsonl(self):
        r1 = va.CommandResult("a", "docs", ["x"], "s", "f", 1.0, 0, "o", "e")
        r2 = va.CommandResult("b", "cargo", ["x"], "s", "f", 1.0, 1, "o", "e")
        s = va.summarize_results([r1, r2], "ci", "dev", Path("x"), "s", "f")
        self.assertEqual(s["status"], "failed")
        with tempfile.TemporaryDirectory() as td:
            td = Path(td)
            va.write_commands_jsonl(td / "c.jsonl", [r1, r2])
            self.assertEqual(len((td / "c.jsonl").read_text().strip().splitlines()), 2)
            va.write_scorecard(td / "scorecard.md", s)
            self.assertIn("Deterministic diagnostics", (td / "scorecard.md").read_text())

    def test_environment_best_effort(self):
        with mock.patch("scripts.validate_all.safe_cmd", return_value="unknown"):
            e = va.collect_environment("dev")
            self.assertIn("git_sha", e)

    def test_dry_run_plan_builds(self):
        plan = va.build_plan(self.args(profile="publish", runs=3), Path("validation/artifacts/x"))
        self.assertTrue(len(plan) > 0)


if __name__ == "__main__":
    unittest.main()
