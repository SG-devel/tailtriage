import json, tempfile, unittest
from pathlib import Path
import scripts.run_operational_validation as op

class OperationalValidationTests(unittest.TestCase):
    def test_delta_ratio(self):
        self.assertEqual(op.delta(1,3),2)
        self.assertIsNone(op.delta(None,3))
        self.assertAlmostEqual(op.ratio_delta(10,12),0.2)
        self.assertIsNone(op.ratio_delta(0,1))
    def test_bytes_per_request(self):
        self.assertEqual(op.bytes_per_request(100,10),10)
        self.assertIsNone(op.bytes_per_request(100,0))
    def test_artifact_size_helper(self):
        with tempfile.TemporaryDirectory() as d:
            p=Path(d)/"x"; p.write_text("abcd",encoding="utf-8")
            self.assertEqual(op.artifact_size_bytes(p),4)
    def test_runtime_record_eval(self):
        r={"p95_overhead_ratio":0.3}
        out=op.evaluate_runtime_cost(r,0.25,False)
        self.assertFalse(out["passed"])
    def test_runtime_summary(self):
        s=op.summarize_runtime_cost([{"p95_overhead_ratio":0.1,"p99_overhead_ratio":0.2,"artifact_bytes_per_request":5,"measurement_quality":"partial"},{"p95_overhead_ratio":0.3,"p99_overhead_ratio":0.4,"artifact_bytes_per_request":7,"measurement_quality":"partial"}])
        self.assertEqual(s["p95_overhead_ratio"]["max"],0.3)
    def test_collector_visibility_fail(self):
        rec={"dropped_requests":1,"dropped_stages":0,"dropped_queues":0,"dropped_inflight_snapshots":0,"dropped_runtime_snapshots":0,"limit_visibility_passed":False,"diagnosis_downgraded_or_warned":False}
        out=op.evaluate_collector_limits(rec,True,False)
        self.assertFalse(out["passed"])
    def test_collector_visibility_pass(self):
        rec={"dropped_requests":1,"dropped_stages":0,"dropped_queues":0,"dropped_inflight_snapshots":0,"dropped_runtime_snapshots":0,"limit_visibility_passed":True,"diagnosis_downgraded_or_warned":True}
        out=op.evaluate_collector_limits(rec,True,False)
        self.assertTrue(out["passed"])
    def test_extractors(self):
        d=op.extract_drop_counters({"truncation":{"dropped_requests":2}})
        self.assertEqual(d["dropped_requests"],2)
        w=op.extract_limit_warnings({"warnings":["collector limit reached; partial"]})
        self.assertTrue(w)
    def test_summary_shape_and_jsonl_and_scorecard(self):
        records=[{"domain":"runtime-cost","passed":True,"p95_overhead_ratio":0.1,"p99_overhead_ratio":0.1,"artifact_bytes_per_request":1.0,"measurement_quality":"partial","failed_expectations":[]},{"domain":"collector-limits","passed":True,"limit_hit":True,"limit_visibility_passed":True,"diagnosis_downgraded_or_warned":True,"dropped_requests":1,"dropped_stages":0,"dropped_queues":0,"dropped_runtime_snapshots":0,"failed_expectations":[]}]
        s=op.summarize_records(records,"dev",["runtime-cost","collector-limits"])
        self.assertIn("schema_version",s)
        with tempfile.TemporaryDirectory() as d:
            j=Path(d)/"a.jsonl"; op.write_jsonl(j,records)
            self.assertEqual(len(j.read_text().strip().splitlines()),2)
            sc=Path(d)/"s.md"; op.write_scorecard(sc,s)
            t=sc.read_text()
            self.assertIn("## Runtime cost",t)
            self.assertIn("## Collector limits",t)

if __name__=="__main__":
    unittest.main()
