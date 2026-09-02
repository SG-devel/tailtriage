import json
import tempfile
import unittest
from pathlib import Path

from scripts import run_diagnostic_matrix as rdm


class RunDiagnosticMatrixTests(unittest.TestCase):
    def sample_report(self, primary_kind="application_queue_pressure", confidence="high", p95=100, p99=200, second=None):
        return {
            "primary_suspect": {"kind": primary_kind, "confidence": confidence, "score": 90},
            "secondary_suspects": second or [{"kind": "downstream_stage_dominance"}],
            "warnings": [],
            "request_count": 10,
            "p95_latency_us": p95,
            "p99_latency_us": p99,
            "p95_queue_share_permille": 900,
            "p95_service_share_permille": 100,
        }

    def metadata(self, **updates):
        base = {
            "name": "queue",
            "variant": "before",
            "ground_truth": "application_queue_pressure",
            "required_top2": ["application_queue_pressure"],
            "acceptable_primary": ["application_queue_pressure"],
            "top1_required": True,
        }
        base.update(updates)
        return base

    # TT-TEST: D02 secondary
    def test_extract_run_record(self):
        rec = rdm.build_record(report=self.sample_report(), metadata=self.metadata(), run_index=1, profile="dev", artifact_path=Path("a.json"), analysis_path=Path("b.json"))
        self.assertTrue(rec["top1_ok"])
        self.assertTrue(rec["top2_ok"])
        self.assertFalse(rec["high_confidence_wrong"])

    # TT-TEST: D02 secondary
    def test_high_confidence_wrong(self):
        rec = rdm.build_record(report=self.sample_report(primary_kind="blocking_pool_pressure"), metadata=self.metadata(), run_index=1, profile="dev", artifact_path=Path("a"), analysis_path=Path("b"))
        self.assertTrue(rec["high_confidence_wrong"])

    # TT-TEST: D01 primary
    def test_primary_stability_and_confidence_bucket(self):
        records = [
            rdm.build_record(report=self.sample_report(), metadata=self.metadata(), run_index=1, profile="dev", artifact_path=Path("a"), analysis_path=Path("b")),
            rdm.build_record(report=self.sample_report(primary_kind="application_queue_pressure", confidence="medium"), metadata=self.metadata(), run_index=2, profile="dev", artifact_path=Path("a"), analysis_path=Path("b")),
            rdm.build_record(report=self.sample_report(primary_kind="blocking_pool_pressure"), metadata=self.metadata(acceptable_primary=["application_queue_pressure", "blocking_pool_pressure"]), run_index=3, profile="dev", artifact_path=Path("a"), analysis_path=Path("b")),
        ]
        summary = rdm.summarize(records, runs=3, profile="dev")
        per = summary["per_scenario"]["queue"]
        self.assertAlmostEqual(per["primary_stability"], 2 / 3)
        self.assertIn("high", per["confidence_bucket_accuracy"])

    # TT-TEST: support
    def test_latency_stats_odd_even_and_missing(self):
        self.assertEqual(rdm.latency_stats([1, 2, 3])["median"], 2)
        self.assertEqual(rdm.latency_stats([1, 2, 3, 4])["median"], 2)
        self.assertIsNone(rdm.latency_stats([]))

    # TT-TEST: D01 primary
    def test_threshold_failures(self):
        rec = rdm.build_record(report=self.sample_report(primary_kind="blocking_pool_pressure"), metadata=self.metadata(), run_index=1, profile="dev", artifact_path=Path("a"), analysis_path=Path("b"))
        summary = rdm.summarize([rec], runs=1, profile="dev")
        failures = rdm.evaluate_thresholds(summary, {"queue": self.metadata()}, min_top1=0.95, min_top2=1.0, max_high_confidence_wrong=0)
        self.assertTrue(any("top1_accuracy" in f for f in failures))
        self.assertTrue(any("high_confidence_wrong_count" in f for f in failures))

    # TT-TEST: D02 secondary
    def test_mixed_without_top1_requirement(self):
        rec = rdm.build_record(report=self.sample_report(primary_kind="executor_pressure", second=[{"kind": "application_queue_pressure"}]), metadata=self.metadata(name="mixed", top1_required=False, acceptable_primary=["application_queue_pressure", "executor_pressure"]), run_index=1, profile="dev", artifact_path=Path("a"), analysis_path=Path("b"))
        summary = rdm.summarize([rec], runs=1, profile="dev")
        failures = rdm.evaluate_thresholds(summary, {"mixed": self.metadata(name="mixed", top1_required=False, acceptable_primary=["application_queue_pressure", "executor_pressure"])}, min_top1=0.95, min_top2=1.0, max_high_confidence_wrong=0)
        self.assertFalse(any("top1_accuracy" in f for f in failures))

    # TT-TEST: support
    def test_jsonl_write(self):
        records = [{"a": 1}, {"b": 2}]
        with tempfile.TemporaryDirectory() as td:
            path = Path(td) / "out.jsonl"
            rdm.write_jsonl(path, records)
            lines = path.read_text(encoding="utf-8").strip().splitlines()
            self.assertEqual(len(lines), 2)
            self.assertEqual(json.loads(lines[0])["a"], 1)

    # TT-TEST: support
    def test_missing_optional_p99_in_summary(self):
        report = self.sample_report()
        report.pop("p99_latency_us")
        rec = rdm.build_record(report=report, metadata=self.metadata(), run_index=1, profile="dev", artifact_path=Path("a"), analysis_path=Path("b"))
        summary = rdm.summarize([rec], runs=1, profile="dev")
        self.assertIsNone(summary["per_scenario"]["queue"]["p99_latency_us"])


if __name__ == "__main__":
    unittest.main()
