import json
import tempfile
import unittest
from pathlib import Path

import scripts.validate_all as va


class ValidateAllTests(unittest.TestCase):
    def _args(self, **kwargs):
        base = {
            "profile": "smoke", "out": "target/validation/smoke", "runs": None, "profile_mode": "dev",
            "skip_cargo": False, "include_cargo": False, "no_fail_fast": False, "no_fail_thresholds": False,
            "dry_run": False, "python": "python3",
        }
        base.update(kwargs)
        return type("A", (), base)()

    def test_smoke_plan(self):
        p = va.build_plan(self._args(profile="smoke", no_fail_thresholds=True))
        names = [x.name for x in p]
        self.assertIn("diagnostic benchmark", names)
        self.assertIn("docs contracts", names)
        self.assertIn("diagnostic matrix", names)
        self.assertIn("mitigation matrix", names)
        self.assertIn("runtime-cost smoke", names)
        self.assertIn("collector-limits smoke", names)

    def test_ci_plan(self):
        p = va.build_plan(self._args(profile="ci"))
        n = [x.name for x in p]
        self.assertIn("diagnostic benchmark tests", n)
        self.assertIn("docs contract tests", n)
        self.assertIn("operational tests", n)

    def test_full_runs_propagates(self):
        p = va.build_plan(self._args(profile="full", runs=7))
        dm = next(x for x in p if x.name == "diagnostic matrix")
        self.assertIn("7", dm.argv)

    def test_profile_mode_propagates(self):
        p = va.build_plan(self._args(profile="full", profile_mode="release"))
        dm = next(x for x in p if x.name == "diagnostic matrix")
        self.assertIn("release", dm.argv)

    def test_skip_and_include_cargo(self):
        p = va.build_plan(self._args(profile="full", skip_cargo=True))
        self.assertFalse(any(x.track == "cargo" for x in p))
        p2 = va.build_plan(self._args(profile="ci", include_cargo=True))
        self.assertTrue(any(x.track == "cargo" for x in p2))

    def test_summary_and_scorecard(self):
        s = va.summarize_results([{"name": "x", "track": "docs", "exit_code": 1}], "ci", "dev", Path("target/x"), "a", "b")
        self.assertEqual(s["status"], "failed")
        with tempfile.TemporaryDirectory() as d:
            sp = Path(d) / "summary.json"
            sc = Path(d) / "scorecard.md"
            va.write_summary(sp, s)
            va.write_scorecard(sc, s)
            self.assertIn("schema_version", json.loads(sp.read_text()))
            self.assertIn("Root cause is not proven", sc.read_text())

    def test_commands_jsonl(self):
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / "commands.jsonl"
            va.write_commands_jsonl(p, [{"a": 1}, {"b": 2}])
            self.assertEqual(len(p.read_text().strip().splitlines()), 2)

    def test_env_best_effort(self):
        e = va.collect_environment("dev")
        self.assertIn("schema_version", e)
        self.assertIn("logical_cores", e)


if __name__ == "__main__":
    unittest.main()
