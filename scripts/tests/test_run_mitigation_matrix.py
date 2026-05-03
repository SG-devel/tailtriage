import json
import tempfile
import unittest
from pathlib import Path

from scripts import run_mitigation_matrix as rmm


class RunMitigationMatrixTests(unittest.TestCase):
    def report(self, pk="application_queue_saturation", ps=90, sk="downstream_stage_dominates", p95=1000, p99=1200, q=900, s=100, evidence=None, conf="high"):
        return {
            "primary_suspect": {"kind": pk, "score": ps, "confidence": conf, "evidence": evidence or ["blocking queue depth p95 12"]},
            "secondary_suspects": [{"kind": sk, "score": 40, "evidence": []}],
            "p95_latency_us": p95,
            "p99_latency_us": p99,
            "p95_queue_share_permille": q,
            "p95_service_share_permille": s,
        }

    def test_top2_and_score_helpers(self):
        rp = self.report()
        self.assertEqual(rmm.top2_kinds(rp), ["application_queue_saturation", "downstream_stage_dominates"])
        self.assertEqual(rmm.suspect_score(rp, "application_queue_saturation"), 90)
        self.assertEqual(rmm.suspect_score(rp, "downstream_stage_dominates"), 40)
        self.assertIsNone(rmm.suspect_score(rp, "blocking_pool_pressure"))

    def test_blocking_depth_extract(self):
        self.assertEqual(rmm.extract_blocking_queue_depth_p95(self.report()), 12)

    def test_delta_ratio(self):
        self.assertEqual(rmm.delta(10, 7), -3)
        self.assertIsNone(rmm.delta(None, 7))
        self.assertAlmostEqual(rmm.ratio_delta(10, 7), -0.3)
        self.assertIsNone(rmm.ratio_delta(0, 7))

    def test_build_pair_record_shape(self):
        meta = {"name": "queue", "targeted_suspect": "application_queue_saturation", "notes": "n"}
        rec = rmm.build_pair_record(self.report(), self.report(p95=800, q=700), meta, {k: Path(k) for k in ["before_artifact_path", "before_analysis_path", "after_artifact_path", "after_analysis_path"]}, "dev")
        self.assertEqual(rec["schema_version"], 1)
        self.assertIn("before_top2_kinds", rec)

    def test_expectations_and_summary_and_writes(self):
        meta = {"name": "queue", "targeted_suspect": "application_queue_saturation", "expected_movements": ["p95_decreases", "queue_share_decreases", "targeted_score_nonworsening"], "acceptable_after_primary": ["application_queue_saturation", "downstream_stage_dominates"], "notes": "n"}
        rec = rmm.build_pair_record(self.report(ps=90, p95=1000, q=900), self.report(ps=70, p95=800, q=700), meta, {k: Path(k) for k in ["before_artifact_path", "before_analysis_path", "after_artifact_path", "after_analysis_path"]}, "dev")
        rec["after_primary_confidence"] = "medium"
        rmm.evaluate_movements(rec, meta, {"min_p95_improvement_ratio": 0.05, "min_p99_improvement_ratio": 0.0})
        self.assertTrue(rec["movement_passed"])

        bad = dict(rec)
        bad["expected_movements"] = {}
        bad["movement_passed"] = False
        bad["failed_expectations"] = ["high_confidence_wrong_after_mitigation"]
        summary = rmm.summarize_records([rec, bad], "dev", 0)
        self.assertEqual(summary["schema_version"], 1)

        with tempfile.TemporaryDirectory() as td:
            j = Path(td) / "a.jsonl"
            rmm.write_jsonl(j, [rec, bad])
            self.assertEqual(len(j.read_text().strip().splitlines()), 2)
            md = Path(td) / "s.md"
            rmm.write_scorecard(md, summary, [rec, bad])
            self.assertIn("| queue |", md.read_text())

    def test_targeted_score_only_not_sufficient(self):
        meta = {"name": "queue", "targeted_suspect": "application_queue_saturation", "expected_movements": ["targeted_score_decreases"], "acceptable_after_primary": ["application_queue_saturation"], "notes": "n"}
        rec = rmm.build_pair_record(self.report(ps=90), self.report(ps=70), meta, {k: Path(k) for k in ["before_artifact_path", "before_analysis_path", "after_artifact_path", "after_analysis_path"]}, "dev")
        rec["after_primary_confidence"] = "low"
        rmm.evaluate_movements(rec, meta, {"min_p95_improvement_ratio": 0.05, "min_p99_improvement_ratio": 0.0})
        self.assertFalse(rec["movement_passed"])


if __name__ == "__main__":
    unittest.main()
