import json,tempfile,unittest
from pathlib import Path
from unittest import mock
from scripts import diagnostic_benchmark as db

def report(kind='application_queue_saturation',conf='high',secondary=None):
 return {'primary_suspect':{'kind':kind,'confidence':conf,'score':1,'evidence':['queue evidence'],'next_checks':['check queue']},'secondary_suspects':([] if secondary is None else [{'kind':secondary,'confidence':'medium','evidence':[],'next_checks':[]}]),'warnings':[]}
def case(cid='run',typ='run_artifact',eligible=True,**kw):
 c={'id':cid,'artifact':cid+'.json','artifact_type':typ,'validation_class':db.TYPE_CLASS[typ],'accuracy_eligible':eligible,'tags':[],'notes':'test case','expected_primary_kinds':['application_queue_saturation'],'required_visible_suspects':['application_queue_saturation'],'must_include_evidence':['queue'],'must_include_next_checks':['check'],'expected_warnings':[],'allowed_warnings':[]}
 if eligible:c.update(observation_id=cid,ground_truth='application_queue_saturation',exact_primary_kind='application_queue_saturation')
 c.update(kw);return c
def manifest(*cases):return {'schema_version':2,'cases':list(cases)}
class Result:
 def __init__(self,code=0,out='',err=''):self.returncode=code;self.stdout=out;self.stderr=err
class Tests(unittest.TestCase):
 def write(self,root,cases,reports=None):
  root=Path(root)
  for c,r in zip(cases,reports or [report()]*len(cases)):(root/c['artifact']).write_text(json.dumps(r))
  p=root/'manifest.json';p.write_text(json.dumps(manifest(*cases)));return p
 def runmock(self,p,outputs):
  with mock.patch.object(db,'_invoke',side_effect=outputs) as inv:return db.run(p,0,0,99),inv
 def test_report_only_manifest_fails_with_zero_analyzer_executions(self):
  c=case('r','analysis_report',False)
  with tempfile.TemporaryDirectory() as td:
   p=self.write(td,[c]);(m,f),inv=self.runmock(p,[])
  self.assertEqual(m['report_contract']['passed_count'],1);self.assertEqual(m['analyzer_execution']['case_count'],0);self.assertIsNone(m['analyzer_accuracy']['top1_accuracy']);self.assertIn('diagnostic corpus contains zero analyzer-executed cases',f);inv.assert_not_called()
 def test_analyzer_execution_without_accuracy_observations_fails_distinctly(self):
  c=case(eligible=False)
  with tempfile.TemporaryDirectory() as td:
   p=self.write(td,[c]);(m,f),inv=self.runmock(p,[Result(out=json.dumps(report()))])
  self.assertEqual(m['analyzer_execution']['case_count'],1);self.assertIn('diagnostic corpus contains zero accuracy-eligible analyzer observations',f);self.assertNotIn('diagnostic corpus contains zero analyzer-executed cases',f);inv.assert_called_once()
 def test_report_contract_results_do_not_change_analyzer_accuracy(self):
  a=case();r=case('r','analysis_report',False)
  vals=[]
  for rr in [report(),report('blocking_pool_pressure','low','downstream_stage_dominates')]:
   with tempfile.TemporaryDirectory() as td:
    p=self.write(td,[a,r],[report(),rr]);(m,_),_=self.runmock(p,[Result(out=json.dumps(report()))]);vals.append(m['analyzer_accuracy'])
  self.assertEqual(vals[0],vals[1])
 def test_run_artifact_executes_cli_analyzer(self):
  c=case()
  with tempfile.TemporaryDirectory() as td:
   p=self.write(td,[c]);(m,_),inv=self.runmock(p,[Result(out=json.dumps(report()))])
  cmd=inv.call_args.args[0];self.assertEqual(cmd[:7],['cargo','run','--quiet','-p','tailtriage-cli','--','analyze']);self.assertNotIn('--allow-ambiguous-artifact',cmd);self.assertEqual(m['analyzer_execution']['run_artifact_count'],1);self.assertEqual(m['report_contract']['case_count'],0)
 def test_tracing_jsonl_executes_import_then_analyzer(self):
  c=case('trace','tracing_span_jsonl');
  def invoke(cmd):
   if 'import' in cmd:Path(cmd[-1]).write_text('{}');return Result()
   return Result(out=json.dumps(report()))
  with tempfile.TemporaryDirectory() as td:
   p=self.write(td,[c]);
   with mock.patch.object(db,'_invoke',side_effect=invoke) as inv:m,f=db.run(p,0,0,99)
  self.assertFalse(f);self.assertIn('import',inv.call_args_list[0].args[0]);self.assertIn('tracing-spans-jsonl',inv.call_args_list[0].args[0]);self.assertIn('analyze',inv.call_args_list[1].args[0]);self.assertEqual(m['analyzer_execution']['tracing_jsonl_count'],1)
 def test_equivalent_encodings_share_one_accuracy_observation(self):
  a=case('a');b=case('b',observation_id='a')
  with tempfile.TemporaryDirectory() as td:
   p=self.write(td,[a,b]);(m,f),_=self.runmock(p,[Result(out=json.dumps(report())),Result(out=json.dumps(report()))])
  self.assertFalse(f);self.assertEqual(m['analyzer_execution']['case_count'],2);self.assertEqual(m['analyzer_accuracy']['encoding_count'],2);self.assertEqual(m['analyzer_accuracy']['observation_count'],1)
 def test_equivalent_encoding_diagnosis_disagreement_fails_observation(self):
  a=case('a');b=case('b',observation_id='a')
  with tempfile.TemporaryDirectory() as td:
   p=self.write(td,[a,b]);(m,f),_=self.runmock(p,[Result(out=json.dumps(report())),Result(out=json.dumps(report(secondary='blocking_pool_pressure')))])
  self.assertTrue(f);x=m['failed_analyzer_cases'][-1];self.assertEqual(x['observation_id'],'a');self.assertEqual(x['member_case_ids'],['a','b']);self.assertEqual(m['analyzer_accuracy']['observation_count'],0)
 def test_equivalent_encoding_confidence_disagreement_fails_observation(self):
  a=case('a');b=case('b',observation_id='a')
  with tempfile.TemporaryDirectory() as td:
   p=self.write(td,[a,b]);(m,_),_=self.runmock(p,[Result(out=json.dumps(report(conf='high'))),Result(out=json.dumps(report(conf='medium')))])
  self.assertIn('confidence disagreement',m['failed_analyzer_cases'][-1]['error'])
 def test_observation_labels_must_match(self):
  changes={'ground_truth':'blocking_pool_pressure','expected_primary_kinds':['blocking_pool_pressure'],'required_visible_suspects':['blocking_pool_pressure'],'exact_primary_kind':'blocking_pool_pressure'}
  for key,value in changes.items():
   with self.subTest(key=key):
    a=case('a');b=case('b',observation_id='a');b[key]=value
    with self.assertRaisesRegex(ValueError,'observation labels disagree|exact_primary_kind'):db.validate_manifest(manifest(a,b))
 def test_class_type_and_eligibility_matrix(self):
  bad=[case('x','analysis_report',False,validation_class='analyzer_execution'),case('x','synthetic_analysis_report',False,validation_class='analyzer_execution'),case(validation_class='report_contract'),case('x','tracing_span_jsonl',True,validation_class='report_contract'),case('x','analysis_report',False,accuracy_eligible=True),case('x','analysis_report',False,ground_truth='insufficient_evidence'),case('x','analysis_report',False,observation_id='x'),case(observation_id=None),case(ground_truth=None),case(eligible=False,ground_truth='insufficient_evidence'),case(eligible=False,execution_expectation='failure',failure_stage='analyze',expected_error_substrings=[],forbidden_error_substrings=[],stdout_expectation='empty',accuracy_eligible=True)]
  for c in bad:
   with self.subTest(c=c):
    with self.assertRaises(ValueError):db.validate_manifest(manifest(c))
 def test_version_1_fields_are_rejected(self):
  for key in db.OLD:
   with self.assertRaisesRegex(ValueError,'version-1'):db.validate_manifest(manifest(case(**{key:[]})))
 def test_expected_execution_failure_is_checked_not_skipped(self):
  base=case(eligible=False,execution_expectation='failure',failure_stage='analyze',expected_error_substrings=['needed'],forbidden_error_substrings=['secret'],stdout_expectation='empty')
  scenarios=[(Result(1,'','needed'),True),(Result(0,json.dumps(report()),''),False),(Result(1,'','missing'),False),(Result(1,'','needed secret'),False),(Result(1,'output','needed'),False)]
  for result,passed in scenarios:
   with tempfile.TemporaryDirectory() as td:
    p=self.write(td,[base]);(m,_),_=self.runmock(p,[result]);self.assertEqual(m['analyzer_execution']['cases'][0]['passed'],passed)
  trace=case('t','tracing_span_jsonl',False,execution_expectation='failure',failure_stage='analyze',expected_error_substrings=[],forbidden_error_substrings=[],stdout_expectation='ignore')
  with tempfile.TemporaryDirectory() as td:
   p=self.write(td,[trace]);(m,_),_=self.runmock(p,[Result(1,'','bad')]);self.assertFalse(m['analyzer_execution']['cases'][0]['passed'])
 def test_artifact_policy_is_typed(self):
  db.validate_manifest(manifest(case()));db.validate_manifest(manifest(case(artifact_policy='allow_ambiguous')))
  self.assertIn('--allow-ambiguous-artifact',db._command('analyze',case(artifact_policy='allow_ambiguous'),Path('x')))
  for c in [case('t','tracing_span_jsonl',True,artifact_policy='strict'),case('r','analysis_report',False,artifact_policy='strict'),case(artifact_policy='other'),case(command=['evil'])]:
   with self.assertRaises(ValueError):db.validate_manifest(manifest(c))
 def test_committed_manifest_invariants(self):
  p=Path(__file__).parents[2]/'validation/diagnostics/manifest.json';m=json.loads(p.read_text());db.validate_manifest(m)
  self.assertEqual(m['schema_version'],2);self.assertTrue(any(c['validation_class']=='analyzer_execution' for c in m['cases']));self.assertTrue(any(c['validation_class']=='report_contract' for c in m['cases']));self.assertTrue(any(c['accuracy_eligible'] for c in m['cases']))
if __name__=='__main__':unittest.main()
