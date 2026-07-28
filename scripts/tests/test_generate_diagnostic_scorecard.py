import json,tempfile,unittest
from pathlib import Path
from unittest import mock
from scripts.generate_diagnostic_scorecard import *
def env():return {'generated_at_utc':'t','snapshot_label':'s','git':{'sha':'a','tag':None,'describe':'a'},'tailtriage':{'workspace_package_version':'1','packages':{}},'github_actions':{},'software':{},'hardware':{},'inputs':{'manifest_sha256':'m','referenced_artifacts_sha256':'a','thresholds':{}}}
def metrics(zero=False):return {'schema_version':2,'manifest_case_count':2,'analyzer_execution':{'case_count':1,'success_count':1,'expected_failure_count':0,'unexpected_failure_count':0,'run_artifact_count':1,'tracing_jsonl_count':0},'analyzer_accuracy':{'observation_count':0 if zero else 1,'encoding_count':0 if zero else 1,'top1_accuracy':None if zero else 1.0,'top2_recall':None if zero else 1.0,'high_confidence_wrong_count':0,'per_ground_truth_counts':{},'confusion_matrix':{},'confidence_bucket_accuracy':{}},'report_contract':{'case_count':1,'passed_count':1,'failed_count':0,'analysis_report_count':1,'synthetic_report_count':0},'validated_paths':{},'failed_analyzer_cases':[],'failed_report_contract_cases':[]}
class Tests(unittest.TestCase):
 def test_scorecard_json_is_separated(self):
  m=metrics();self.assertEqual(m['schema_version'],2);self.assertNotIn('total_cases',m);self.assertEqual({'analyzer_execution','analyzer_accuracy','report_contract'} <= m.keys(),True)
 def test_scorecard_markdown_is_separated(self):
  text=render_scorecard(metrics(),env())
  for h in ['## Analyzer execution','## Analyzer accuracy','## Report-contract validation','## Validated input paths','## Failed analyzer cases','## Failed report-contract cases','## Non-claims']:self.assertIn(h,text)
  self.assertIn('Unique accuracy observations: 1',text);self.assertIn('Executed artifact encodings: 1',text);self.assertIn('do not contribute to analyzer accuracy',text)
 def test_unavailable_accuracy_renders_na(self):
  text=render_scorecard(metrics(True),env());self.assertIn('Top-1 accuracy: n/a',text);self.assertIn('Top-2 recall: n/a',text)
 def test_environment_schema_two(self):
  repo=Path(__file__).parents[2];e=collect_environment(repo,repo/'validation/diagnostics/manifest.json','x',{});self.assertEqual(e['schema_version'],2)
 def test_hashes(self):self.assertEqual(sha256_bytes(b'a'),sha256_bytes(b'a'))
 def test_generate_writes_separated_json(self):
  with tempfile.TemporaryDirectory() as td:
   root=Path(td);(root/'Cargo.toml').write_text('[workspace]\nmembers=[]\n[workspace.package]\nversion="1"\n');p=root/'validation';p.mkdir();(p/'manifest.json').write_text('{"cases":[]}')
   with mock.patch('scripts.generate_diagnostic_scorecard.run_diagnostic_benchmark',return_value=(metrics(),[])):
    generate_scorecard(root,'validation/manifest.json','out',.75,.9,0,'x')
   got=json.loads((root/'out/benchmark-summary.json').read_text());self.assertEqual(got['schema_version'],2);self.assertIn('analyzer_accuracy',got)
if __name__=='__main__':unittest.main()
