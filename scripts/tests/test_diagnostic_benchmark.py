import json, tempfile, unittest, os
from pathlib import Path
from scripts import diagnostic_benchmark as db

BASE={"id":"x","artifact":"a.json","artifact_type":"analysis_report","ground_truth":"application_queue_saturation","required_top2":["application_queue_saturation"],"acceptable_primary":["application_queue_saturation"],"tags":["t"],"must_include_evidence":[],"must_include_next_checks":[],"expected_warnings":[],"allowed_warnings":[],"top1_required":False,"notes":"n"}

class DiagnosticBenchmarkTests(unittest.TestCase):
    def _write(self, root, rel, payload):
        p=Path(root)/rel; p.parent.mkdir(parents=True, exist_ok=True); p.write_text(json.dumps(payload)); return p

    def test_manifest_validation_errors(self):
        bad=dict(BASE); bad["acceptable_primary"]=[]
        with self.assertRaisesRegex(ValueError,"acceptable_primary"):
            db.validate_manifest({"schema_version":1,"cases":[bad]})
        bad=dict(BASE); bad["expected_warnings"]= ["*"]
        with self.assertRaisesRegex(ValueError,"wildcard"):
            db.validate_manifest({"schema_version":1,"cases":[bad]})

    def test_required_top2_vs_acceptable_primary_semantics(self):
        with tempfile.TemporaryDirectory() as td:
            self._write(td,"a.json",{"primary_suspect":{"kind":"blocking_pool_pressure","confidence":"high","score":1,"evidence":["x"]},"secondary_suspects":[],"warnings":[]})
            case=dict(BASE); case["required_top2"]= ["application_queue_saturation"]; case["acceptable_primary"]= ["application_queue_saturation","blocking_pool_pressure"]
            m=self._write(td,"manifest.json",{"schema_version":1,"cases":[case]})
            metrics,failures=db.run(str(m),0,0,9)
            self.assertEqual(metrics["high_confidence_wrong_count"],0)
            self.assertTrue(failures)

    def test_acceptable_primary_with_secondary_ground_truth_passes(self):
        with tempfile.TemporaryDirectory() as td:
            self._write(td,"a.json",{"primary_suspect":{"kind":"blocking_pool_pressure","confidence":"very_high","score":1,"evidence":["x"]},"secondary_suspects":[{"kind":"application_queue_saturation","evidence":["y"]}],"warnings":[]})
            case=dict(BASE); case["acceptable_primary"]= ["application_queue_saturation","blocking_pool_pressure"]
            m=self._write(td,"manifest.json",{"schema_version":1,"cases":[case]})
            metrics,failures=db.run(str(m),0,0,9)
            self.assertEqual(metrics["high_confidence_wrong_count"],0)
            self.assertFalse(failures)

    def test_unacceptable_high_confidence_counts_wrong_even_if_top2_ok(self):
        with tempfile.TemporaryDirectory() as td:
            self._write(td,"a.json",{"primary_suspect":{"kind":"blocking_pool_pressure","confidence":"high","score":1,"evidence":["x"]},"secondary_suspects":[{"kind":"application_queue_saturation","evidence":["y"]}],"warnings":[]})
            m=self._write(td,"manifest.json",{"schema_version":1,"cases":[dict(BASE)]})
            metrics,_=db.run(str(m),0,0,9)
            self.assertEqual(metrics["high_confidence_wrong_count"],1)

    def test_malformed_secondary_and_warnings(self):
        with tempfile.TemporaryDirectory() as td:
            case=dict(BASE); case["artifact"]="b.json"
            m=self._write(td,"manifest.json",{"schema_version":1,"cases":[case]})
            self._write(td,"b.json",{"primary_suspect":{"kind":"application_queue_saturation","confidence":"low","score":1,"evidence":["x"]},"secondary_suspects":[{"kind":"bad"}],"warnings":[]})
            with self.assertRaisesRegex(ValueError,"secondary_suspects.kind"): db.run(str(m),0,0,0)

    def test_absolute_manifest_path(self):
        with tempfile.TemporaryDirectory() as td:
            self._write(td,"a.json",{"primary_suspect":{"kind":"application_queue_saturation","confidence":"low","score":1,"evidence":["x"]},"secondary_suspects":[],"warnings":[]})
            m=self._write(td,"manifest.json",{"schema_version":1,"cases":[dict(BASE)]})
            old=os.getcwd(); os.chdir('/')
            try: metrics,failures=db.run(str(m.resolve()),0,0,1)
            finally: os.chdir(old)
            self.assertEqual(metrics['total_cases'],1); self.assertFalse(failures)

if __name__=='__main__': unittest.main()
