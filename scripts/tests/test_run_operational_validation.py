import json
import tempfile
import unittest
from pathlib import Path

from scripts import run_operational_validation as rov


class TestRunOperationalValidation(unittest.TestCase):
    def test_delta_ratio(self):
        self.assertEqual(rov.delta(1, 3), 2)
        self.assertIsNone(rov.delta(None, 3))
        self.assertEqual(rov.ratio_delta(10, 12), 0.2)
        self.assertIsNone(rov.ratio_delta(0, 1))

    def test_bytes_per_request(self):
        self.assertEqual(rov.bytes_per_request(100, 10), 10.0)
        self.assertIsNone(rov.bytes_per_request(100, 0))

    def test_artifact_size(self):
        with tempfile.TemporaryDirectory() as td:
            p = Path(td) / "a.json"
            p.write_text("abc", encoding="utf-8")
            self.assertEqual(rov.artifact_size_bytes(p), 3)

    def test_extract_helpers(self):
        payload = {"truncation": {"dropped_requests": 1, "dropped_stages": 2, "dropped_queues": 3, "dropped_inflight_snapshots": 4, "dropped_runtime_snapshots": 5}}
        drops = rov.extract_drop_counters(payload)
        self.assertEqual(drops["dropped_requests"], 1)
        warnings = rov.extract_limit_warnings({"warnings": ["collector limit reached; report is partial", "foo"]})
        self.assertEqual(len(warnings), 1)

    def test_runtime_eval(self):
        r = {"p95_overhead_ratio": 0.3}
        out = rov.evaluate_runtime_cost(r, {"max_relative_p95_overhead": 0.25})
        self.assertFalse(out["passed"])

    def test_collector_eval(self):
        rec = {"dropped_requests": 10, "dropped_stages": 0, "dropped_queues": 0, "dropped_inflight_snapshots": 0, "dropped_runtime_snapshots": 0, "warnings": [], "limit_hit": False, "diagnosis_downgraded_or_warned": False}
        out = rov.evaluate_collector_limits(rec, require_visibility=True)
        self.assertFalse(out["passed"])

    def test_jsonl_and_scorecard(self):
        with tempfile.TemporaryDirectory() as td:
            out = Path(td) / "out.jsonl"
            rov.write_jsonl(out, [{"a": 1}, {"b": 2}])
            self.assertEqual(len(out.read_text().strip().splitlines()), 2)
            summary = rov.summarize_records([], "dev", ["runtime-cost", "collector-limits"])
            sc = Path(td) / "score.md"
            rov.write_scorecard(sc, summary)
            txt = sc.read_text()
            self.assertIn("Runtime cost", txt)
            self.assertIn("Collector limits", txt)


if __name__ == "__main__":
    unittest.main()
