import json, tempfile, unittest
from pathlib import Path
from scripts import diagnostic_benchmark as b

class T(unittest.TestCase):
    def _write(self,d,p,o):
        q=Path(d)/p; q.parent.mkdir(parents=True,exist_ok=True); q.write_text(json.dumps(o)); return q

    def test_manifest_validation_missing(self):
        with tempfile.TemporaryDirectory() as d:
            p=self._write(d,'m.json',[{"id":"x"}])
            with self.assertRaises(SystemExit): b.load_manifest(p)

    def test_manifest_validation_duplicates_unknown(self):
        base={"id":"a","artifact":"a.json","artifact_type":"analysis_report","ground_truth":"application_queue_saturation","acceptable_top2":["application_queue_saturation"],"tags":[],"must_include_evidence":[],"allowed_warnings":[],"notes":"n"}
        with tempfile.TemporaryDirectory() as d:
            p=self._write(d,'m.json',[base,{**base}])
            with self.assertRaises(SystemExit): b.load_manifest(p)
            p=self._write(d,'m2.json',[{**base,"id":"b","ground_truth":"nope","acceptable_top2":["nope"]}])
            with self.assertRaises(SystemExit): b.load_manifest(p)

    def test_json_output_shape_stable(self):
        import subprocess
        out='target/diagnostic-benchmark-test.json'
        subprocess.run(['python3','scripts/diagnostic_benchmark.py','--manifest','validation/diagnostics/manifest.json','--output',out],check=True)
        payload=json.loads(Path(out).read_text())
        for k in ["total_cases","top1_accuracy","top2_recall","required_evidence_pass_rate","unexpected_warning_count","per_ground_truth","confidence_bucket_accuracy","confusion_matrix","failed_cases"]:
            self.assertIn(k,payload)

if __name__=='__main__':
    unittest.main()
