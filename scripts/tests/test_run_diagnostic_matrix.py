import json
import tempfile
import unittest
from pathlib import Path

from scripts import run_diagnostic_matrix as rdm


class RunDiagnosticMatrixTests(unittest.TestCase):
    def sample_report(self, primary_kind="application_queue_saturation", confidence="high", secondary_kind="downstream_stage_dominates"):
        return {
            "request_count": 10,
            "p95_latency_us": 100,
            "p99_latency_us": 150,
            "p95_queue_share_permille": 900,
            "p95_service_share_permille": 100,
            "warnings": [],
            "primary_suspect": {"kind": primary_kind, "confidence": confidence, "score": 99},
            "secondary_suspects": [{"kind": secondary_kind, "confidence": "low", "score": 10}],
        }

    def test_extract_run_record_minimal(self):
        rec = rdm.extract_run_record(self.sample_report(), scenario_name="queue", scenario=rdm.SCENARIO_MATRIX["queue"], run_index=1, profile="dev", artifact_path=Path("a"), analysis_path=Path("b"))
        self.assertTrue(rec["top1_ok"])
        self.assertTrue(rec["top2_ok"])

    def test_top1_top2_high_conf_wrong(self):
        rec = rdm.extract_run_record(self.sample_report(primary_kind="blocking_pool_pressure"), scenario_name="queue", scenario=rdm.SCENARIO_MATRIX["queue"], run_index=1, profile="dev", artifact_path=Path("a"), analysis_path=Path("b"))
        self.assertFalse(rec["top1_ok"])
        self.assertFalse(rec["top2_ok"])
        self.assertTrue(rec["high_confidence_wrong"])

    def test_primary_stability_and_confidence_bucket(self):
        rows = []
        for i in range(3):
            rec = rdm.extract_run_record(self.sample_report(), scenario_name="queue", scenario=rdm.SCENARIO_MATRIX["queue"], run_index=i + 1, profile="dev", artifact_path=Path("a"), analysis_path=Path("b"))
            rows.append(rec)
        rows.append(rdm.extract_run_record(self.sample_report(primary_kind="downstream_stage_dominates", confidence="medium"), scenario_name="queue", scenario={**rdm.SCENARIO_MATRIX["queue"], "acceptable_primary": ["application_queue_saturation", "downstream_stage_dominates"]}, run_index=4, profile="dev", artifact_path=Path("a"), analysis_path=Path("b")))
        summary = rdm.summarize_records(rows, runs=4, profile="dev")
        self.assertEqual(summary["per_scenario"]["queue"]["primary_stability"], 0.75)
        self.assertIn("high", summary["per_scenario"]["queue"]["confidence_bucket_accuracy"])

    def test_latency_stats_even_odd_and_missing(self):
        self.assertEqual(rdm.latency_stats([1, 2, 3])["median"], 2)
        self.assertEqual(rdm.latency_stats([1, 2, 3, 4])["iqr"], 2)
        self.assertIsNone(rdm.latency_stats([]))

    def test_summary_shape_and_threshold_failures(self):
        rows = [rdm.extract_run_record(self.sample_report(primary_kind="blocking_pool_pressure"), scenario_name="queue", scenario=rdm.SCENARIO_MATRIX["queue"], run_index=1, profile="dev", artifact_path=Path("a"), analysis_path=Path("b"))]
        summary = rdm.summarize_records(rows, runs=1, profile="dev")
        self.assertIn("per_scenario", summary)
        fails = rdm.evaluate_thresholds(summary, ["queue"], 0.95, 1.0, 0)
        self.assertTrue(any("top1_accuracy" in item for item in fails))
        self.assertTrue(any("high_confidence_wrong_count" in item for item in fails))

    def test_mixed_threshold_top2_only(self):
        rows = [rdm.extract_run_record(self.sample_report(primary_kind="executor_pressure_suspected"), scenario_name="mixed", scenario=rdm.SCENARIO_MATRIX["mixed"], run_index=1, profile="dev", artifact_path=Path("a"), analysis_path=Path("b"))]
        summary = rdm.summarize_records(rows, runs=1, profile="dev")
        fails = rdm.evaluate_thresholds(summary, ["mixed"], 0.95, 1.0, 0)
        self.assertFalse(any("top1_accuracy" in item for item in fails))

    def test_jsonl_write(self):
        with tempfile.TemporaryDirectory() as td:
            out = Path(td) / "x.jsonl"
            rows = [{"a": 1}, {"b": 2}]
            rdm.write_jsonl(out, rows)
            lines = out.read_text(encoding="utf-8").strip().splitlines()
            self.assertEqual(len(lines), 2)
            self.assertEqual(json.loads(lines[0])["a"], 1)


if __name__ == "__main__":
    unittest.main()
