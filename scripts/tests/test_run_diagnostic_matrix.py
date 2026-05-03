import sys
from pathlib import Path
REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPTS_DIR = REPO_ROOT / "scripts"
sys.path.insert(0, str(SCRIPTS_DIR))

import json
import tempfile
import unittest
import run_diagnostic_matrix as m

class MatrixTests(unittest.TestCase):
    def meta(self, **kw):
        base = {"scenario":"queue","variant":"before","ground_truth":"application_queue_saturation","acceptable_primary":{"application_queue_saturation"},"required_top2":{"application_queue_saturation"},"top1_required":True,"tags":["single_cause"]}
        base.update(kw)
        return base

    def report(self, primary="application_queue_saturation", conf="high", secondary="downstream_stage_dominates"):
        return {"primary_suspect":{"kind":primary,"confidence":conf,"score":95},"secondary_suspects":[{"kind":secondary}],"warnings":[],"request_count":10,"p95_latency_us":100,"p99_latency_us":120}

    def test_extract(self):
        r = m.extract_run_record(self.report(), self.meta(), 1, "dev", Path("a"), Path("b"))
        self.assertTrue(r["top1_ok"])
        self.assertTrue(r["top2_ok"])

    def test_high_conf_wrong(self):
        r = m.extract_run_record(self.report(primary="blocking_pool_pressure"), self.meta(acceptable_primary={"application_queue_saturation"}), 1, "dev", Path("a"), Path("b"))
        self.assertTrue(r["high_confidence_wrong"])

    def test_stability_and_conf_bucket(self):
        recs = [m.extract_run_record(self.report(), self.meta(), i, "dev", Path("a"), Path("b")) for i in [1,2]]
        s = m.summarize_records(recs,2,"dev")
        self.assertEqual(s["per_scenario"]["queue"]["primary_stability"],1.0)
        self.assertIn("high", s["per_scenario"]["queue"]["confidence_bucket_accuracy"])

    def test_iqr_even_odd(self):
        self.assertEqual(m.summarize_latency([1,2,3])["iqr"], 2)
        self.assertEqual(m.summarize_latency([1,2,3,4])["iqr"], 2)

    def test_thresholds(self):
        rec = m.extract_run_record(self.report(primary="blocking_pool_pressure"), self.meta(acceptable_primary={"application_queue_saturation"}),1,"dev",Path("a"),Path("b"))
        s = m.summarize_records([rec],1,"dev")
        fails = m.evaluate_thresholds(s, {"queue": self.meta()}, 0.95, 1.0, 0)
        self.assertTrue(fails)

    def test_mixed_no_top1_enforcement(self):
        rec = m.extract_run_record(self.report(primary="executor_pressure_suspected"), self.meta(scenario="mixed", top1_required=False, tags=["mixed"], acceptable_primary={"application_queue_saturation","executor_pressure_suspected"}),1,"dev",Path("a"),Path("b"))
        s = m.summarize_records([rec],1,"dev")
        fails = m.evaluate_thresholds(s, {"mixed": self.meta(scenario="mixed", top1_required=False, tags=["mixed"])}, 0.95,1.0,0)
        self.assertFalse(any("top1" in f for f in fails))

    def test_jsonl(self):
        with tempfile.TemporaryDirectory() as d:
            p = Path(d)/"o.jsonl"
            m.write_jsonl(p,[{"a":1},{"b":2}])
            lines = p.read_text().strip().splitlines()
            self.assertEqual(len(lines),2)
            json.loads(lines[0])

    def test_missing_optional_latency(self):
        rep = self.report(); del rep["p99_latency_us"]
        rec = m.extract_run_record(rep, self.meta(),1,"dev",Path("a"),Path("b"))
        s = m.summarize_records([rec],1,"dev")
        self.assertIsNone(s["per_scenario"]["queue"]["p99_latency_us"])

if __name__ == '__main__':
    unittest.main()
