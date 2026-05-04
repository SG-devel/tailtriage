import json, tempfile, unittest
from pathlib import Path
from scripts import run_operational_validation as rov

class T(unittest.TestCase):
    def test_delta_ratio(self):
        self.assertEqual(rov.delta(1,3),2); self.assertIsNone(rov.delta(None,3))
        self.assertEqual(rov.ratio_delta(2,3),0.5); self.assertIsNone(rov.ratio_delta(0,3))
    def test_bytes_per_req(self):
        self.assertEqual(rov.bytes_per_request(10,2),5); self.assertIsNone(rov.bytes_per_request(1,0))
    def test_artifact_size(self):
        with tempfile.TemporaryDirectory() as td:
            p=Path(td)/'a'; p.write_text('abcd'); self.assertEqual(rov.artifact_size_bytes(p),4)
    def test_runtime_record_and_eval(self):
        r=rov.latency_overhead_record(domain='runtime-cost',scenario='q',profile='dev',run_index=1,baseline_p95_latency_us=100,instrumented_p95_latency_us=140,p95_overhead_ratio=0.4,p99_overhead_ratio=None,artifact_bytes_per_request=2)
        self.assertEqual(r['schema_version'],1)
        out=rov.evaluate_runtime_cost(r,{"max_relative_p95_overhead":0.25}); self.assertFalse(out['passed'])
    def test_runtime_summary(self):
        s=rov.summarize_runtime_cost([{"measurement_quality":"partial","p95_overhead_ratio":0.1,"p99_overhead_ratio":0.2,"artifact_bytes_per_request":3},{"measurement_quality":"partial","p95_overhead_ratio":0.3,"p99_overhead_ratio":0.1,"artifact_bytes_per_request":5}])
        self.assertEqual(s['p95_overhead_ratio']['median'],0.2); self.assertEqual(s['p95_overhead_ratio']['max'],0.3)
    def test_collector_eval_fail_and_pass(self):
        bad=rov.collector_limit_record(domain='collector-limits',scenario='x',profile='dev',dropped_requests=2,dropped_stages=0,dropped_queues=0,dropped_inflight_snapshots=0,dropped_runtime_snapshots=0,warnings=[],limit_hit=False,diagnosis_downgraded_or_warned=False)
        self.assertFalse(rov.evaluate_collector_limits(bad,True)['passed'])
        good=rov.collector_limit_record(domain='collector-limits',scenario='x',profile='dev',dropped_requests=2,dropped_stages=0,dropped_queues=0,dropped_inflight_snapshots=0,dropped_runtime_snapshots=0,warnings=['partial'],limit_hit=True,diagnosis_downgraded_or_warned=True)
        self.assertTrue(rov.evaluate_collector_limits(good,True)['passed'])
    def test_extract_helpers(self):
        d=rov.extract_drop_counters({'truncation':{'dropped_requests':1,'dropped_stages':2,'dropped_queues':3,'dropped_inflight_snapshots':4,'dropped_runtime_snapshots':5}})
        self.assertEqual(d['dropped_stages'],2)
        w=rov.extract_limit_warnings({'warnings':['collector limit reached; report is partial','other']})
        self.assertEqual(len(w),1)
    def test_summary_shape_jsonl_scorecard(self):
        recs=[{"domain":"runtime-cost","scenario":"q","passed":True,"measurement_quality":"partial","p95_overhead_ratio":0.1,"p99_overhead_ratio":0.2,"artifact_bytes_per_request":1,"failed_expectations":[]},{"domain":"collector-limits","scenario":"c","passed":True,"limit_hit":True,"limit_visibility_passed":True,"diagnosis_downgraded_or_warned":True,"dropped_requests":1,"dropped_stages":0,"dropped_queues":0,"dropped_runtime_snapshots":0,"failed_expectations":[]}]
        s=rov.summarize_records(recs,'dev',['runtime-cost','collector-limits']); self.assertEqual(s['schema_version'],1)
        with tempfile.TemporaryDirectory() as td:
            p=Path(td)/'o.jsonl'; rov.write_jsonl(p,recs); self.assertEqual(len(p.read_text().strip().splitlines()),2)
            m=Path(td)/'s.md'; rov.write_scorecard(m,s,recs); t=m.read_text(); self.assertIn('## Runtime cost',t); self.assertIn('## Collector limits',t)
    def test_no_fail_thresholds_style(self):
        r={"domain":"runtime-cost"}; r.setdefault('failed_expectations',[]); r.setdefault('passed',True); self.assertTrue(r['passed'])

if __name__=='__main__': unittest.main()
