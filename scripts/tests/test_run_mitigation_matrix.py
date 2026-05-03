import json
import tempfile
import unittest
from pathlib import Path

from scripts import run_mitigation_matrix as rmm


class RunMitigationMatrixTests(unittest.TestCase):
    def sample_report(self, primary="application_queue_saturation", secondary=None, p95=1000, p99=1200):
        return {
            "primary_suspect": {"kind": primary, "score": 90, "confidence": "high", "evidence": ["Blocking queue depth p95 is 12"]},
            "secondary_suspects": secondary or [{"kind": "downstream_stage_dominates", "score": 60}],
            "p95_latency_us": p95,
            "p99_latency_us": p99,
            "p95_queue_share_permille": 800,
            "p95_service_share_permille": 200,
        }

    def test_top2(self):
        self.assertEqual(rmm.top2_kinds(self.sample_report()), ["application_queue_saturation", "downstream_stage_dominates"])

    def test_suspect_score_lookup(self):
        rep = self.sample_report()
        self.assertEqual(rmm.suspect_score(rep, "application_queue_saturation"), 90)
        self.assertEqual(rmm.suspect_score(rep, "downstream_stage_dominates"), 60)
        self.assertIsNone(rmm.suspect_score(rep, "blocking_pool_pressure"))

    def test_blocking_depth_extract(self):
        self.assertEqual(rmm.extract_blocking_queue_depth_p95(self.sample_report()), 12)

    def test_delta_ratio(self):
        self.assertEqual(rmm.delta(10, 7), -3)
        self.assertIsNone(rmm.delta(None, 7))
        self.assertAlmostEqual(rmm.ratio_delta(10, 5), -0.5)
        self.assertIsNone(rmm.ratio_delta(0, 5))

    def test_build_pair_record_required(self):
        before = self.sample_report(p95=1000)
        after = self.sample_report(p95=700)
        rec = rmm.build_pair_record(before, after, rmm.SCENARIOS["queue"], scenario="queue", profile="dev", before_artifact_path=Path("a"), before_analysis_path=Path("b"), after_artifact_path=Path("c"), after_analysis_path=Path("d"))
        self.assertEqual(rec["scenario"], "queue")
        self.assertIn("p95_delta_ratio", rec)

    def test_evaluate_checks(self):
        before = self.sample_report(p95=1000)
        after = self.sample_report(p95=800)
        rec = rmm.build_pair_record(before, after, rmm.SCENARIOS["queue"], scenario="queue", profile="dev", before_artifact_path=Path("a"), before_analysis_path=Path("b"), after_artifact_path=Path("c"), after_analysis_path=Path("d"))
        rec["after_p95_queue_share_permille"] = 600
        rec["queue_share_delta_permille"] = -200
        out = rmm.evaluate_movements(rec, rmm.SCENARIOS["queue"], {"min_p95_improvement_ratio": 0.05, "min_p99_improvement_ratio": 0.0})
        self.assertTrue(out["expected_movements"]["p95_decreases"])

    def test_targeted_score_alone_not_sufficient(self):
        rec = {"targeted_score_delta": -1, "before_primary_kind": "a", "targeted_suspect": "a", "after_primary_kind": "a", "after_top2_kinds": ["a"], "p95_delta_ratio": None, "p99_delta_ratio": None, "queue_share_delta_permille": None, "service_share_delta_permille": None, "blocking_queue_depth_delta": None}
        meta = {"expected_movements": ["targeted_score_decreases"]}
        out = rmm.evaluate_movements(rec, meta, {"min_p95_improvement_ratio": 0.05, "min_p99_improvement_ratio": 0.0})
        self.assertIn("targeted_score_decreases_requires_additional_concrete_movement", out["failed_expectations"])

    def test_summary_jsonl_scorecard(self):
        rec = {"scenario": "queue", "movement_passed": True, "failed_expectations": [], "p95_delta_us": -1, "p95_delta_ratio": -0.1, "before_primary_kind": "a", "after_primary_kind": "b", "before_targeted_score": 10, "after_targeted_score": 9, "queue_share_delta_permille": -10, "high_confidence_wrong": False, "targeted_suspect": "a", "expected_movements": {"p95_decreases": True}, "notes": "n"}
        summary = rmm.summarize_records([rec], "dev", 0)
        self.assertEqual(summary["total_scenarios"], 1)
        with tempfile.TemporaryDirectory() as td:
            p = Path(td) / "x.jsonl"
            rmm.write_jsonl(p, [rec])
            self.assertEqual(len(p.read_text().strip().splitlines()), 1)
            md = Path(td) / "x.md"
            rmm.write_scorecard(md, summary, [rec])
            self.assertIn("| queue |", md.read_text())


if __name__ == "__main__":
    unittest.main()
