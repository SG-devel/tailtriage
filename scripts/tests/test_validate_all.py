import json
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace

import scripts.validate_all as va


class ValidateAllTests(unittest.TestCase):
    def args(self, profile="smoke"):
        return SimpleNamespace(profile=profile, out=f"target/validation/{profile}", runs=1, profile_mode="dev", skip_cargo=False, include_cargo=False, no_fail_thresholds=False, python="python3")

    def test_smoke_plan(self):
        plan = va.build_plan(self.args("smoke"))
        names = " ".join(c.name for c in plan)
        self.assertIn("deterministic benchmark", names)
        self.assertIn("docs contract", names)
        self.assertIn("diag matrix smoke", names)
        self.assertIn("mitigation smoke", names)
        self.assertIn("runtime-cost smoke", names)
        self.assertIn("collector-limits smoke", names)

    def test_ci_plan_has_tests(self):
        plan = va.build_plan(self.args("ci"))
        joined = "\n".join(" ".join(c.argv) for c in plan)
        self.assertIn("scripts.tests.test_diagnostic_benchmark", joined)
        self.assertIn("scripts.tests.test_run_diagnostic_matrix", joined)
        self.assertIn("scripts.tests.test_run_mitigation_matrix", joined)
        self.assertIn("scripts.tests.test_run_operational_validation", joined)
        self.assertIn("scripts.tests.test_validate_docs_contracts", joined)

    def test_full_includes_live_tracks(self):
        a = self.args("full"); a.runs = 7
        plan = va.build_plan(a)
        joined = "\n".join(" ".join(c.argv) for c in plan)
        self.assertIn("--domain all", joined)
        self.assertIn("--runs 7", joined)

    def test_publish_default_dir(self):
        p = va.derive_publish_dir()
        self.assertIn("validation/artifacts", str(p))

    def test_skip_and_include_cargo(self):
        a = self.args("ci"); a.include_cargo = True
        self.assertTrue(any(c.track == "cargo" for c in va.build_plan(a)))
        a.skip_cargo = True
        self.assertFalse(any(c.track == "cargo" for c in va.build_plan(a)))

    def test_profile_mode_propagates(self):
        a = self.args("smoke"); a.profile_mode = "release"
        plan = va.build_plan(a)
        joined = "\n".join(" ".join(c.argv) for c in plan)
        self.assertIn("--profile release", joined)

    def test_summary_and_logs(self):
        spec_ok = va.CommandSpec("ok", "docs", ["echo", "ok"])
        spec_bad = va.CommandSpec("bad", "docs", ["false"])
        r1 = va.CommandResult(spec_ok, "s", "e", 0.1, 0, "o", "e")
        r2 = va.CommandResult(spec_bad, "s", "e", 0.1, 1, "o", "e")
        s = va.summarize_results([r1, r2], "ci", "dev", Path("x"), "a", "b")
        self.assertEqual(s["status"], "failed")
        self.assertEqual(len(s["failed_commands"]), 1)
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / "commands.jsonl"
            va.write_commands_jsonl(p, [r1, r2])
            self.assertEqual(len(p.read_text().strip().splitlines()), 2)
            sc = Path(d) / "scorecard.md"
            va.write_scorecard(sc, s)
            self.assertIn("Tailtriage validation scorecard", sc.read_text())

    def test_environment_best_effort(self):
        env = va.collect_environment("dev")
        self.assertIn("schema_version", env)
        self.assertIn("logical_cores", env)


if __name__ == "__main__":
    unittest.main()
