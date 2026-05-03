from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path
import sys

REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPTS_DIR = REPO_ROOT / "scripts"
sys.path.insert(0, str(SCRIPTS_DIR))

import run_diagnostic_matrix as rdm


class MatrixTests(unittest.TestCase):
    def scenario(self, **kwargs):
        base = rdm.ScenarioDef("queue", "demos/queue_service/Cargo.toml", "before", "application_queue_saturation", ("application_queue_saturation",), ("application_queue_saturation",), True)
        return rdm.ScenarioDef(**{**base.__dict__, **kwargs})

    def report(self, **kwargs):
        rep = {
            "primary_suspect": {"kind": "application_queue_saturation", "confidence": "high", "score": 95},
            "secondary_suspects": [{"kind": "downstream_stage_dominates"}],
            "warnings": [],
            "request_count": 10,
            "p95_latency_us": 100,
            "p99_latency_us": 120,
            "p95_queue_share_permille": 900,
            "p95_service_share_permille": 100,
        }
        rep.update(kwargs)
        return rep

    def test_extract_record(self):
        rec = rdm.extract_run_record(self.report(), self.scenario(), 1, "dev", Path("a"), Path("b"))
        self.assertTrue(rec["top1_ok"])
        self.assertTrue(rec["top2_ok"])

    def test_high_conf_wrong(self):
        rep = self.report(primary_suspect={"kind": "blocking_pool_pressure", "confidence": "high", "score": 90})
        rec = rdm.extract_run_record(rep, self.scenario(), 1, "dev", Path("a"), Path("b"))
        self.assertTrue(rec["high_confidence_wrong"])

    def test_primary_stability_and_bucket(self):
        s = self.scenario()
        recs = [rdm.extract_run_record(self.report(), s, i, "dev", Path("a"), Path("b")) for i in range(1, 4)]
        summary = rdm.summarize_records(recs, 3, "dev")
        self.assertEqual(summary["per_scenario"]["queue"]["primary_stability"], 1.0)
        self.assertEqual(summary["per_scenario"]["queue"]["confidence_bucket_accuracy"]["high"]["accuracy"], 1.0)

    def test_iqr_stats_even_odd(self):
        self.assertEqual(rdm.iqr_stats([1, 2, 3])["iqr"], 2)
        self.assertEqual(rdm.iqr_stats([1, 2, 3, 4])["iqr"], 2)

    def test_threshold_failures(self):
        s = self.scenario()
        bad = rdm.extract_run_record(self.report(primary_suspect={"kind": "blocking_pool_pressure", "confidence": "high"}), s, 1, "dev", Path("a"), Path("b"))
        summary = rdm.summarize_records([bad], 1, "dev")
        fails = rdm.evaluate_thresholds(summary, [s], 0.95, 1.0, 0)
        self.assertTrue(any("top1_accuracy" in f for f in fails))
        self.assertTrue(any("high_confidence_wrong" in f for f in fails))

    def test_mixed_top2_without_top1(self):
        mixed = self.scenario(name="mixed", top1_required=False, required_top2=("application_queue_saturation", "executor_pressure_suspected"))
        rep = self.report(primary_suspect={"kind": "executor_pressure_suspected", "confidence": "medium"}, secondary_suspects=[{"kind": "application_queue_saturation"}])
        rec = rdm.extract_run_record(rep, mixed, 1, "dev", Path("a"), Path("b"))
        summary = rdm.summarize_records([rec], 1, "dev")
        fails = rdm.evaluate_thresholds(summary, [mixed], 0.95, 1.0, 0)
        self.assertFalse(any("top1_accuracy" in f for f in fails))

    def test_jsonl_write(self):
        with tempfile.TemporaryDirectory() as td:
            p = Path(td) / "x.jsonl"
            rdm.write_jsonl(p, [{"a": 1}, {"a": 2}])
            lines = p.read_text().strip().splitlines()
            self.assertEqual(len(lines), 2)
            self.assertEqual(json.loads(lines[0])["a"], 1)

    def test_missing_optional_latency(self):
        rep = self.report()
        del rep["p99_latency_us"]
        s = self.scenario()
        rec = rdm.extract_run_record(rep, s, 1, "dev", Path("a"), Path("b"))
        summary = rdm.summarize_records([rec], 1, "dev")
        self.assertIsNone(summary["per_scenario"]["queue"]["p99_latency_us"])


if __name__ == "__main__":
    unittest.main()
