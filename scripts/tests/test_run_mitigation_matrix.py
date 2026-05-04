import json
import tempfile
import unittest
from pathlib import Path

from scripts import run_mitigation_matrix as rmm


class RunMitigationMatrixTests(unittest.TestCase):
    def report(self, primary="application_queue_saturation", second=None, score=90, p95=1000, p99=1200, q=900, s=100, confidence="high", evidence=None):
        return {
            "primary_suspect": {"kind": primary, "score": score, "confidence": confidence, "evidence": evidence or []},
            "secondary_suspects": second or [{"kind": "downstream_stage_dominates", "score": 40, "evidence": []}],
            "p95_latency_us": p95,
            "p99_latency_us": p99,
            "p95_queue_share_permille": q,
            "p95_service_share_permille": s,
        }

    def test_top2(self):
        self.assertEqual(rmm.top2_kinds(self.report()), ["application_queue_saturation", "downstream_stage_dominates"])

    def test_suspect_score(self):
        rep = self.report()
        self.assertEqual(rmm.suspect_score(rep, "application_queue_saturation"), 90)
        self.assertEqual(rmm.suspect_score(rep, "downstream_stage_dominates"), 40)
        self.assertIsNone(rmm.suspect_score(rep, "missing"))

    def test_extract_blocking_depth(self):
        rep = self.report(primary="blocking_pool_pressure", evidence=["Blocking queue depth p95 is 12"])
        self.assertEqual(rmm.extract_blocking_queue_depth_p95(rep), 12)

    def test_delta_ratio(self):
        self.assertEqual(rmm.delta(5, 1), -4)
        self.assertIsNone(rmm.delta(None, 1))
        self.assertAlmostEqual(rmm.ratio_delta(100, 50), -0.5)
        self.assertIsNone(rmm.ratio_delta(0, 10))

    def test_build_pair_record_required_fields(self):
        rec = rmm.build_pair_record(self.report(), self.report(p95=500), {"targeted_suspect": "application_queue_saturation", "notes": "x"}, before_artifact=Path("b1"), before_analysis=Path("b2"), after_artifact=Path("a1"), after_analysis=Path("a2"), profile="dev", scenario="queue")
        self.assertEqual(rec["schema_version"], 1)
        self.assertEqual(rec["scenario"], "queue")
        self.assertIn("before_top2_kinds", rec)

    def test_movement_checks(self):
        rec = rmm.build_pair_record(self.report(), self.report(p95=800, q=500, s=80), {"targeted_suspect": "application_queue_saturation", "notes": "x"}, before_artifact=Path("b1"), before_analysis=Path("b2"), after_artifact=Path("a1"), after_analysis=Path("a2"), profile="dev", scenario="queue")
        meta = {"targeted_suspect": "application_queue_saturation", "expected_movements": ["p95_decreases", "queue_share_decreases", "service_share_decreases"]}
        out = rmm.evaluate_movements(rec, meta, {"min_p95_improvement_ratio": 0.05})
        self.assertTrue(out["movement_passed"])

    def test_blocking_movement(self):
        b = self.report(primary="blocking_pool_pressure", evidence=["Blocking queue depth p95 is 12"])
        a = self.report(primary="blocking_pool_pressure", evidence=["Blocking queue depth p95 is 2"], p95=700)
        rec = rmm.build_pair_record(b, a, {"targeted_suspect": "blocking_pool_pressure"}, before_artifact=Path("b1"), before_analysis=Path("b2"), after_artifact=Path("a1"), after_analysis=Path("a2"), profile="dev", scenario="blocking")
        out = rmm.evaluate_movements(rec, {"targeted_suspect": "blocking_pool_pressure", "expected_movements": ["blocking_queue_depth_decreases"]}, {"min_p95_improvement_ratio": 0.05})
        self.assertTrue(out["movement_passed"])

    def test_targeted_score_only_not_enough(self):
        before = self.report(score=90)
        after = self.report(score=70)
        rec = rmm.build_pair_record(before, after, {"targeted_suspect": "application_queue_saturation"}, before_artifact=Path("b1"), before_analysis=Path("b2"), after_artifact=Path("a1"), after_analysis=Path("a2"), profile="dev", scenario="queue")
        out = rmm.evaluate_movements(rec, {"targeted_suspect": "application_queue_saturation", "expected_movements": ["targeted_score_decreases"]}, {"min_p95_improvement_ratio": 0.05})
        self.assertFalse(out["movement_passed"])

    def test_high_conf_wrong_fails(self):
        rec = rmm.build_pair_record(self.report(), self.report(primary="executor_pressure_suspected", confidence="high"), {"targeted_suspect": "application_queue_saturation"}, before_artifact=Path("b1"), before_analysis=Path("b2"), after_artifact=Path("a1"), after_analysis=Path("a2"), profile="dev", scenario="queue")
        out = rmm.evaluate_movements(rec, {"targeted_suspect": "application_queue_saturation", "expected_movements": [], "expected_after_top2": ["application_queue_saturation"]}, {"min_p95_improvement_ratio": 0.05})
        self.assertFalse(out["movement_passed"])

    def test_unknown_movement_key_fails_loudly(self):
        rec = rmm.build_pair_record(self.report(), self.report(p95=800), {"targeted_suspect": "application_queue_saturation"}, before_artifact=Path("b1"), before_analysis=Path("b2"), after_artifact=Path("a1"), after_analysis=Path("a2"), profile="dev", scenario="queue")
        meta = {"targeted_suspect": "application_queue_saturation", "expected_movements": ["p95_decreases", "typo_queue_share_decreases"]}
        out = rmm.evaluate_movements(rec, meta, {"min_p95_improvement_ratio": 0.05})
        self.assertTrue(out["expected_movements"]["p95_decreases"])
        self.assertFalse(out["expected_movements"]["typo_queue_share_decreases"])
        self.assertFalse(out["movement_passed"])
        self.assertIn("unknown_movement_key:typo_queue_share_decreases", out["failed_expectations"])

    def test_summary_jsonl_scorecard(self):
        rec = {"scenario": "queue", "movement_passed": True, "failed_expectations": [], "p95_delta_us": -10, "p95_delta_ratio": -0.1, "before_primary_kind": "a", "after_primary_kind": "b", "before_targeted_score": 90, "after_targeted_score": 70, "queue_share_delta_permille": -100, "high_confidence_wrong_after": False, "expected_movements": {"p95_decreases": True}, "targeted_suspect": "application_queue_saturation", "notes": "n"}
        summary = rmm.summarize_records([rec], "dev")
        self.assertEqual(summary["schema_version"], 1)
        with tempfile.TemporaryDirectory() as td:
            p = Path(td) / "out.jsonl"
            rmm.write_jsonl(p, [rec])
            self.assertEqual(len(p.read_text().strip().splitlines()), 1)
            m = Path(td) / "score.md"
            rmm.write_scorecard(m, summary, [rec])
            self.assertIn("| queue |", m.read_text())


if __name__ == "__main__":
    unittest.main()
