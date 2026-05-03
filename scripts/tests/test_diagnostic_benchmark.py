from __future__ import annotations
import json, tempfile, unittest
from pathlib import Path
import scripts.diagnostic_benchmark as b

def mk_report(kind="application_queue_saturation",conf="high",warnings=None,sec=None,evidence=None):
    return {"warnings":warnings or [],"primary_suspect":{"kind":kind,"confidence":conf,"score":90,"evidence":evidence or ["Queue wait"]},"secondary_suspects":sec or []}

class T(unittest.TestCase):
    def test_missing_field(self):
        with self.assertRaises(ValueError): b.validate_manifest({"cases":[{"id":"x"}]})
    def test_duplicate_ids(self):
        m={"cases":[{"id":"x","artifact":"a.json","artifact_type":"analysis_report","ground_truth":"application_queue_saturation","acceptable_top2":["application_queue_saturation"],"tags":[],"must_include_evidence":[],"allowed_warnings":[],"notes":"n"}]*2}
        with self.assertRaises(ValueError): b.validate_manifest(m)
    def test_unknown_gt(self):
        m={"cases":[{"id":"x","artifact":"a.json","artifact_type":"analysis_report","ground_truth":"bad","acceptable_top2":["bad"],"tags":[],"must_include_evidence":[],"allowed_warnings":[],"notes":"n"}]}
        with self.assertRaises(ValueError): b.validate_manifest(m)
    def test_top2_contains_gt(self):
        m={"cases":[{"id":"x","artifact":"a.json","artifact_type":"analysis_report","ground_truth":"application_queue_saturation","acceptable_top2":["blocking_pool_pressure"],"tags":[],"must_include_evidence":[],"allowed_warnings":[],"notes":"n"}]}
        with self.assertRaises(ValueError): b.validate_manifest(m)
    def test_metrics_and_json_shape(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d)
            (root/"r1.json").write_text(json.dumps(mk_report()))
            (root/"r2.json").write_text(json.dumps(mk_report(kind="blocking_pool_pressure",conf="high",sec=[{"kind":"application_queue_saturation","evidence":["Queue wait"]}],evidence=["Blocking queue"])))
            m={"cases":[
                {"id":"c1","artifact":"r1.json","artifact_type":"analysis_report","ground_truth":"application_queue_saturation","acceptable_top2":["application_queue_saturation"],"tags":[],"must_include_evidence":["Queue wait"],"allowed_warnings":[],"notes":"n"},
                {"id":"c2","artifact":"r2.json","artifact_type":"analysis_report","ground_truth":"application_queue_saturation","acceptable_top2":["application_queue_saturation"],"tags":[],"must_include_evidence":["Queue wait"],"allowed_warnings":[],"notes":"n"},
            ]}
            (root/"m.json").write_text(json.dumps(m))
            out=root/"out.json"
            with self.assertRaises(SystemExit):
                b.run(root/"m.json",0.75,0.9,str(out))
            data=json.loads(out.read_text())
            self.assertIn("confidence_bucket_accuracy",data)
            self.assertEqual(data["high_confidence_wrong_count"],1)
    def test_allowed_warning_substrings(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d)
            (root/"r.json").write_text(json.dumps(mk_report(warnings=["truncated stage events"])))
            m={"cases":[{"id":"c","artifact":"r.json","artifact_type":"analysis_report","ground_truth":"application_queue_saturation","acceptable_top2":["application_queue_saturation"],"tags":[],"must_include_evidence":["Queue wait"],"allowed_warnings":["truncated"],"notes":"n"}]}
            (root/"m.json").write_text(json.dumps(m))
            b.run(root/"m.json",0.0,0.0,None)

if __name__=="__main__": unittest.main()
