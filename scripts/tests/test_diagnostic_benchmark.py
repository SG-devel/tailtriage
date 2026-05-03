import json, tempfile, unittest
from pathlib import Path
import sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import diagnostic_benchmark as db

class T(unittest.TestCase):
    def test_manifest_validation(self):
        with self.assertRaises(FileNotFoundError): db.load_manifest(Path('/tmp/nope'))
    def test_duplicate_ids(self):
        with tempfile.TemporaryDirectory() as d:
            p=Path(d)/'m.json';p.write_text(json.dumps({'cases':[{'id':'a','artifact':'x','artifact_type':'analysis_report','ground_truth':'application_queue_saturation','acceptable_top2':['application_queue_saturation'],'tags':[],'must_include_evidence':[],'allowed_warnings':[],'notes':'n'},{'id':'a','artifact':'x','artifact_type':'analysis_report','ground_truth':'application_queue_saturation','acceptable_top2':['application_queue_saturation'],'tags':[],'must_include_evidence':[],'allowed_warnings':[],'notes':'n'}]}))
            with self.assertRaises(ValueError): db.load_manifest(p)
    def test_metrics(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d)
            rpt={'primary_suspect':{'kind':'application_queue_saturation','confidence':'high','score':90,'evidence':['Queue wait']},'secondary_suspects':[{'kind':'downstream_stage_dominates','evidence':['Stage db']}],'warnings':[]}
            (root/'r.json').write_text(json.dumps(rpt))
            cases=[{'id':'c1','artifact':'r.json','artifact_type':'analysis_report','ground_truth':'application_queue_saturation','acceptable_top2':['application_queue_saturation'],'tags':[],'must_include_evidence':['Stage db'],'allowed_warnings':[],'notes':'x'}]
            m=db.run(cases,root)
            self.assertEqual(m['top1_accuracy'],1.0);self.assertEqual(m['top2_recall'],1.0);self.assertEqual(m['high_confidence_wrong_count'],0)
            self.assertEqual(m['required_evidence_pass_rate'],1.0);self.assertEqual(m['unexpected_warning_count'],0)

if __name__=='__main__': unittest.main()
