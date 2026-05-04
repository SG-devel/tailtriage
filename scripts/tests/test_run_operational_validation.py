import json, tempfile, unittest
from pathlib import Path
from scripts import run_operational_validation as rov

class T(unittest.TestCase):
    def test_delta_ratio(self):
        self.assertEqual(rov.delta(1,3),2); self.assertIsNone(rov.delta(None,1)); self.assertAlmostEqual(rov.ratio_delta(10,12),0.2); self.assertIsNone(rov.ratio_delta(0,1))
    def test_bytes_per_request(self):
        self.assertEqual(rov.bytes_per_request(100,10),10); self.assertIsNone(rov.bytes_per_request(1,0))
    def test_artifact_size(self):
        with tempfile.TemporaryDirectory() as td:
            p=Path(td)/'a'; p.write_text('abcd'); self.assertEqual(rov.artifact_size_bytes(p),4)
    def test_runtime_record_and_eval(self):
        r=rov.latency_overhead_record(schema_version=1,domain='runtime-cost',scenario='queue',profile='dev',baseline_p50_latency_us=None,p95_overhead_ratio=0.3)
        out=rov.evaluate_runtime_cost(r,{"max_relative_p95_overhead":0.25}); self.assertFalse(out['passed'])
    def test_runtime_summary(self):
        s=rov.summarize_runtime_cost([{"p95_overhead_ratio":0.1,"p99_overhead_ratio":0.2,"artifact_bytes_per_request":5,"measurement_quality":"partial"}])
        self.assertEqual(s['records'],1)
    def test_collector_eval_visibility(self):
        r={"dropped_requests":1,"dropped_stages":0,"dropped_queues":0,"dropped_inflight_snapshots":0,"dropped_runtime_snapshots":0,"warnings":[],"limit_hit":False,"diagnosis_downgraded_or_warned":False}
        self.assertFalse(rov.evaluate_collector_limits(r.copy(),True)['passed'])
        r2=r.copy(); r2['warnings']=['collector limit reached']; r2['diagnosis_downgraded_or_warned']=True
        self.assertTrue(rov.evaluate_collector_limits(r2,True)['passed'])
    def test_extractors(self):
        obj={"truncation":{"dropped_requests":2,"dropped_stages":3,"dropped_queues":4,"dropped_inflight_snapshots":5,"dropped_runtime_snapshots":6}}
        self.assertEqual(rov.extract_drop_counters(obj)['dropped_queues'],4)
        self.assertTrue(rov.extract_limit_warnings({"warnings":["collector limit reached; report is partial"]}))
    def test_summary_shape_jsonl_scorecard(self):
        recs=[{"domain":"runtime-cost","passed":True,"measurement_quality":"partial","p95_overhead_ratio":0.1,"p99_overhead_ratio":0.1,"artifact_bytes_per_request":1},{"domain":"collector-limits","passed":True,"limit_hit":True,"limit_visibility_passed":True,"diagnosis_downgraded_or_warned":True,"dropped_requests":1,"dropped_stages":0,"dropped_queues":0,"dropped_runtime_snapshots":0}]
        s=rov.summarize_records(recs,'dev'); self.assertEqual(s['schema_version'],1)
        with tempfile.TemporaryDirectory() as td:
            p=Path(td)/'o.jsonl'; rov.write_jsonl(p,recs); self.assertEqual(len(p.read_text().strip().splitlines()),2)
            md=Path(td)/'s.md'; rov.write_scorecard(md,s); t=md.read_text(); self.assertIn('## Runtime cost',t); self.assertIn('## Collector limits',t)

if __name__=='__main__':unittest.main()
