#!/usr/bin/env python3
from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from scripts import run_mitigation_matrix as rmm


class RunMitigationMatrixTests(unittest.TestCase):
    def report(self, primary="application_queue_saturation", second=None, score=90):
        return {
            "primary_suspect": {"kind": primary, "score": score, "confidence": "high", "evidence": ["blocking queue p95 12"]},
            "secondary_suspects": second or [{"kind": "downstream_stage_dominates", "score": 40, "evidence": []}],
            "p95_latency_us": 100,
            "p99_latency_us": 120,
            "p95_queue_share_permille": 900,
            "p95_service_share_permille": 100,
        }

    def test_top2(self):
        self.assertEqual(rmm.top2_kinds(self.report()), ["application_queue_saturation", "downstream_stage_dominates"])

    def test_suspect_score(self):
        rep = self.report()
        self.assertEqual(rmm.suspect_score(rep, "application_queue_saturation"), 90)
        self.assertEqual(rmm.suspect_score(rep, "downstream_stage_dominates"), 40)
        self.assertIsNone(rmm.suspect_score(rep, "blocking_pool_pressure"))

    def test_blocking_extract(self):
        self.assertEqual(rmm.extract_blocking_queue_depth_p95(self.report()), 12)

    def test_delta_ratio(self):
        self.assertEqual(rmm.delta(10, 6), -4)
        self.assertIsNone(rmm.delta(None, 1))
        self.assertAlmostEqual(rmm.ratio_delta(10, 5), -0.5)
        self.assertIsNone(rmm.ratio_delta(0, 5))

    def test_build_record_shape(self):
        rec = rmm.build_pair_record(self.report(), self.report(primary="downstream_stage_dominates"), {"name": "queue", "targeted_suspect": "application_queue_saturation", "notes": "n"}, profile="dev", before_artifact_path=Path("a"), before_analysis_path=Path("b"), after_artifact_path=Path("c"), after_analysis_path=Path("d"))
        self.assertIn("scenario", rec)
        self.assertIn("p95_delta_ratio", rec)

    def test_evaluate_movements(self):
        rec = {"p95_delta_ratio": -0.2, "queue_share_delta_permille": -1, "service_share_delta_permille": -1, "blocking_queue_depth_delta": -1, "targeted_score_delta": -1, "after_top2_kinds": ["application_queue_saturation"], "targeted_suspect": "application_queue_saturation", "before_primary_kind": "application_queue_saturation", "after_primary_kind": "downstream_stage_dominates", "p99_delta_us": 0}
        meta = {"expected_movements": ["p95_decreases", "queue_share_decreases", "service_share_decreases", "blocking_queue_depth_decreases", "targeted_score_decreases", "top2_retains_target_or_expected_successor", "primary_changes_from_targeted"], "expected_successor": "downstream_stage_dominates"}
        out = rmm.evaluate_movements(rec, meta, {"min_p95_improvement_ratio": 0.05})
        self.assertTrue(all(out.values()))

    def test_summary_jsonl_scorecard(self):
        records = [{"scenario": "queue", "movement_passed": True, "failed_expectations": [], "p95_delta_us": -1, "p95_delta_ratio": -0.1, "before_primary_kind": "a", "after_primary_kind": "b", "before_targeted_score": 1, "after_targeted_score": 0, "queue_share_delta_permille": -1, "high_confidence_wrong_after": False}]
        summary = rmm.summarize_records(records, "dev")
        self.assertEqual(summary["total_scenarios"], 1)
        with tempfile.TemporaryDirectory() as td:
            out = Path(td) / "o.jsonl"
            rmm.write_jsonl(out, records)
            self.assertEqual(len(out.read_text().splitlines()), 1)
            sc = Path(td) / "s.md"
            rmm.write_scorecard(sc, summary)
            self.assertIn("| queue |", sc.read_text())


if __name__ == "__main__":
    unittest.main()
