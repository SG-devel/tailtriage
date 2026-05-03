import json
import os
import tempfile
import unittest
from pathlib import Path

from scripts import diagnostic_benchmark as db

BASE_CASE = {
    "id": "case-1", "artifact": "a.json", "artifact_type": "analysis_report",
    "ground_truth": "application_queue_saturation", "required_top2": ["application_queue_saturation"],
    "acceptable_primary": ["application_queue_saturation"], "tags": ["t"],
    "must_include_evidence": [], "must_include_next_checks": [], "expected_warnings": [],
    "allowed_warnings": [], "top1_required": False, "notes": "note",
}

def report(primary_kind="application_queue_saturation", secondary=None, warnings=None, confidence="high"):
    return {"primary_suspect": {"kind": primary_kind, "confidence": confidence, "score": 1.0, "evidence": ["Queue wait"]}, "secondary_suspects": secondary or [], "warnings": warnings or []}

class DiagnosticBenchmarkTests(unittest.TestCase):
    def _write(self, root, rel, payload):
        p = Path(root) / rel; p.parent.mkdir(parents=True, exist_ok=True); p.write_text(json.dumps(payload), encoding="utf-8"); return p
    def _manifest(self, case):
        return {"schema_version": 1, "cases": [case]}

    def test_manifest_and_report_validation_samples(self):
        with self.assertRaises(ValueError): db.validate_manifest({"cases": [dict(BASE_CASE)]})
        bad=dict(BASE_CASE); bad["required_top2"]=[]
        with self.assertRaisesRegex(ValueError,"required_top2"): db.validate_manifest(self._manifest(bad))
        with self.assertRaisesRegex(ValueError,"kind"): db.extract({"primary_suspect":{"kind":"bad","confidence":"high","evidence":[]},"secondary_suspects":[],"warnings":[]})
        with self.assertRaisesRegex(ValueError,"secondary_suspects.confidence"): db.extract({"primary_suspect":{"kind":"application_queue_saturation","confidence":"high","evidence":[]},"secondary_suspects":[{"confidence":"bad"}],"warnings":[]})

    def test_semantics_and_counters(self):
        with tempfile.TemporaryDirectory() as td:
            case=dict(BASE_CASE); case["required_top2"]=["application_queue_saturation"]; case["acceptable_primary"]=["application_queue_saturation","blocking_pool_pressure"]
            self._write(td,"a.json",report("blocking_pool_pressure",[{"kind":"application_queue_saturation","evidence":["x"]}],[]))
            metrics, failures=db.run(str(self._write(td,"manifest.json",self._manifest(case))),0,0,0)
            self.assertEqual(metrics["high_confidence_wrong_count"],0); self.assertFalse(failures)

            case2=dict(case); case2["id"]="c2"; case2["acceptable_primary"]=["application_queue_saturation"]
            self._write(td,"b.json",report("blocking_pool_pressure",[{"kind":"application_queue_saturation"}],[]))
            m={"schema_version":1,"cases":[dict(case2,artifact='b.json')]}
            metrics, failures=db.run(str(self._write(td,"manifest2.json",m)),0,0,0)
            self.assertEqual(metrics["high_confidence_wrong_count"],1); self.assertTrue(failures)

    def test_warning_evidence_threshold_and_json_shape(self):
        with tempfile.TemporaryDirectory() as td:
            c=dict(BASE_CASE); c["must_include_evidence"]=["secondary-evidence"]; c["expected_warnings"]=["need signal"]; c["allowed_warnings"]=["extra ok"]; c["top1_required"]=True
            sec=[{"kind":"blocking_pool_pressure","evidence":["secondary-evidence"],"confidence":"low","score":0.1}]
            self._write(td,"a.json",report("application_queue_saturation",sec,["need signal", "extra ok"]))
            metrics, failures = db.run(str(self._write(td,"m.json",self._manifest(c))),0.99,0.99,0)
            for k in ["total_cases","top1_accuracy","top2_recall","high_confidence_wrong_count","required_evidence_pass_rate","unexpected_warning_count","missing_expected_warning_count","next_check_required_cases","next_check_pass_rate","next_check_presence_rate","failed_cases"]:
                self.assertIn(k, metrics)
            self.assertFalse(failures)
            c2=dict(c); c2["expected_warnings"]=["missing"]
            metrics, failures=db.run(str(self._write(td,'m2.json',self._manifest(c2))),0,0,10)
            self.assertEqual(metrics["missing_expected_warning_count"],1); self.assertTrue(failures)

    def test_invalid_shapes_and_path(self):
        with tempfile.TemporaryDirectory() as td:
            self._write(td,"a.json",report())
            m=self._write(td,"manifest.json",self._manifest(dict(BASE_CASE)))
            cwd=os.getcwd(); os.chdir('/')
            try: metrics,_=db.run(str(m.resolve()),0,0,1)
            finally: os.chdir(cwd)
            self.assertEqual(metrics["total_cases"],1)
            bad=report(); bad["primary_suspect"]["score"]="x"; self._write(td,"bad.json",bad)
            with self.assertRaisesRegex(ValueError,"score"): db.run(str(self._write(td,"mbad.json",self._manifest(dict(BASE_CASE,artifact='bad.json')))),0,0,1)

if __name__ == '__main__':
    unittest.main()
