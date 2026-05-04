import json, tempfile, unittest
from pathlib import Path
import scripts.validate_all as va

class ValidateAllTests(unittest.TestCase):
    def ns(self, **k):
        base=dict(profile='smoke',out='target/validation/smoke',runs=1,profile_mode='dev',skip_cargo=False,include_cargo=False,no_fail_fast=False,no_fail_thresholds=False,dry_run=True,python='python3')
        base.update(k)
        return type('N',(),base)()

    def test_smoke_plan(self):
        p=va.build_plan(self.ns(profile='smoke',no_fail_thresholds=True))
        s='\n'.join(' '.join(x.argv) for x in p)
        self.assertIn('diagnostic_benchmark.py',s); self.assertIn('validate_docs_contracts.py',s)
        self.assertIn('run_diagnostic_matrix.py',s); self.assertIn('run_mitigation_matrix.py',s)
        self.assertIn('--domain runtime-cost',s); self.assertIn('--domain collector-limits',s)

    def test_ci_plan_tests(self):
        p=va.build_plan(self.ns(profile='ci'))
        s='\n'.join(' '.join(x.argv) for x in p)
        self.assertIn('test_diagnostic_benchmark',s); self.assertIn('test_validate_docs_contracts',s)

    def test_full_plan_runs_and_profile(self):
        p=va.build_plan(self.ns(profile='full',runs=7,profile_mode='release',skip_cargo=True))
        s='\n'.join(' '.join(x.argv) for x in p)
        self.assertIn('--runs 7',s); self.assertIn('--profile release',s)

    def test_include_skip_cargo(self):
        self.assertFalse(any(c.track=='cargo' for c in va.build_plan(self.ns(profile='ci'))))
        self.assertTrue(any(c.track=='cargo' for c in va.build_plan(self.ns(profile='ci',include_cargo=True))))
        self.assertFalse(any(c.track=='cargo' for c in va.build_plan(self.ns(profile='full',skip_cargo=True))))

    def test_summary_and_scorecard(self):
        results=[{'name':'a','track':'docs','exit_code':0},{'name':'b','track':'diagnostics','exit_code':1}]
        s=va.summarize_results(results,'full','dev',Path('x'),'a','b')
        self.assertEqual(s['commands']['failed'],1)
        self.assertEqual(s['status'],'failed')
        with tempfile.TemporaryDirectory() as d:
            p=Path(d)/'scorecard.md'; va.write_scorecard(p,s)
            t=p.read_text(); self.assertIn('Root cause is not proven',t)

    def test_commands_jsonl(self):
        with tempfile.TemporaryDirectory() as d:
            p=Path(d)/'commands.jsonl'; va.write_commands_jsonl(p,[{'a':1},{'b':2}])
            self.assertEqual(len(p.read_text().strip().splitlines()),2)

    def test_environment_best_effort(self):
        e=va.collect_environment('dev')
        self.assertIn('schema_version',e)
        self.assertEqual(e['build_profile'],'dev')

if __name__=='__main__':
    unittest.main()
