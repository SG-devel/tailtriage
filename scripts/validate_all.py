#!/usr/bin/env python3
from __future__ import annotations
import argparse, json, os, platform, subprocess, sys
from dataclasses import dataclass, asdict
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

@dataclass
class CommandSpec:
    name:str
    track:str
    argv:list[str]


def utc_now()->str:
    return datetime.now(timezone.utc).replace(microsecond=0).isoformat().replace('+00:00','Z')

def default_out_dir(profile:str)->Path:
    return Path('target')/'validation'/profile

def derive_publish_dir()->Path:
    sha='unknown'
    try:
        sha=subprocess.run(['git','rev-parse','--short','HEAD'],check=True,capture_output=True,text=True).stdout.strip()
    except Exception:
        pass
    return Path('validation')/'artifacts'/f"{datetime.now(timezone.utc).strftime('%Y%m%d')}-git-{sha}"

def validate_args(args:argparse.Namespace)->None:
    if args.runs<=0: raise SystemExit('--runs must be > 0')

def build_plan(args:argparse.Namespace)->list[CommandSpec]:
    py=args.python
    out=Path(args.out)
    plan=[CommandSpec('diagnostic benchmark','diagnostics',[py,'scripts/diagnostic_benchmark.py','--manifest','validation/diagnostics/manifest.json','--output',str(out/'diagnostics'/'benchmark-summary.json')]),
          CommandSpec('docs contract','docs',[py,'scripts/validate_docs_contracts.py'])]
    if args.profile=='smoke':
        plan += [
            CommandSpec('diagnostic matrix smoke','diagnostic_matrix',[py,'scripts/run_diagnostic_matrix.py','--runs','1','--scenario','queue','--out',str(out/'diagnostic-matrix'/'runs.jsonl'),'--summary',str(out/'diagnostic-matrix'/'summary.json'),'--scorecard',str(out/'diagnostic-matrix'/'scorecard.md'),'--profile',args.profile_mode,'--no-fail-thresholds' if args.no_fail_thresholds else '']),
            CommandSpec('mitigation smoke','mitigation',[py,'scripts/run_mitigation_matrix.py','--scenario','queue','--out',str(out/'mitigation'/'runs.jsonl'),'--summary',str(out/'mitigation'/'summary.json'),'--scorecard',str(out/'mitigation'/'scorecard.md'),'--profile',args.profile_mode,'--no-fail-thresholds' if args.no_fail_thresholds else '']),
            CommandSpec('runtime cost smoke','runtime_cost',[py,'scripts/run_operational_validation.py','--domain','runtime-cost','--scenario','queue','--runs','1','--out',str(out/'operational'/'runtime-cost.jsonl'),'--summary',str(out/'operational'/'runtime-cost-summary.json'),'--scorecard',str(out/'operational'/'runtime-cost-scorecard.md'),'--profile',args.profile_mode,'--no-fail-thresholds' if args.no_fail_thresholds else '']),
            CommandSpec('collector limits smoke','collector_limits',[py,'scripts/run_operational_validation.py','--domain','collector-limits','--scenario','queue-limit-pressure','--out',str(out/'operational'/'collector-limits.jsonl'),'--summary',str(out/'operational'/'collector-limits-summary.json'),'--scorecard',str(out/'operational'/'collector-limits-scorecard.md'),'--profile',args.profile_mode,'--no-fail-thresholds' if args.no_fail_thresholds else ''])
        ]
    if args.profile in {'ci','full','publish'}:
        plan += [
            CommandSpec('benchmark tests','diagnostics',[py,'-m','unittest','scripts.tests.test_diagnostic_benchmark']),
            CommandSpec('diagnostic matrix tests','diagnostic_matrix',[py,'-m','unittest','scripts.tests.test_run_diagnostic_matrix']),
            CommandSpec('mitigation tests','mitigation',[py,'-m','unittest','scripts.tests.test_run_mitigation_matrix']),
            CommandSpec('operational tests','operational',[py,'-m','unittest','scripts.tests.test_run_operational_validation']),
            CommandSpec('docs contract tests','docs',[py,'-m','unittest','scripts.tests.test_validate_docs_contracts']),
            CommandSpec('demo fixture drift','docs',[py,'scripts/check_demo_fixture_drift.py','--profile',args.profile_mode]),
        ]
    if args.profile in {'full','publish'}:
        r=str(args.runs)
        plan += [
            CommandSpec('diagnostic matrix full','diagnostic_matrix',[py,'scripts/run_diagnostic_matrix.py','--runs',r,'--scenario','queue','--scenario','blocking','--scenario','executor','--scenario','downstream','--out',str(out/'diagnostic-matrix'/'runs.jsonl'),'--summary',str(out/'diagnostic-matrix'/'summary.json'),'--scorecard',str(out/'diagnostic-matrix'/'scorecard.md'),'--profile',args.profile_mode,'--no-fail-thresholds' if args.no_fail_thresholds else '']),
            CommandSpec('mitigation full','mitigation',[py,'scripts/run_mitigation_matrix.py','--scenario','queue','--scenario','blocking','--scenario','downstream','--scenario','db-pool','--out',str(out/'mitigation'/'runs.jsonl'),'--summary',str(out/'mitigation'/'summary.json'),'--scorecard',str(out/'mitigation'/'scorecard.md'),'--profile',args.profile_mode,'--no-fail-thresholds' if args.no_fail_thresholds else '']),
            CommandSpec('operational all','operational',[py,'scripts/run_operational_validation.py','--domain','all','--runs',r,'--out',str(out/'operational'/'operational-validation.jsonl'),'--summary',str(out/'operational'/'operational-validation-summary.json'),'--scorecard',str(out/'operational'/'operational-validation-scorecard.md'),'--artifact-root',str(out/'operational'),'--profile',args.profile_mode,'--no-fail-thresholds' if args.no_fail_thresholds else ''])]
    include_cargo = (args.profile in {'full','publish'} and not args.skip_cargo) or args.include_cargo
    if include_cargo:
        plan += [CommandSpec('cargo fmt','cargo',['cargo','fmt','--check']),CommandSpec('cargo clippy','cargo',['cargo','clippy','--workspace','--all-targets','--','-D','warnings']),CommandSpec('cargo test','cargo',['cargo','test','--workspace'])]
    for s in plan:
        s.argv=[x for x in s.argv if x!='']
    return plan

def run_command(spec:CommandSpec, log_dir:Path)->dict[str,Any]:
    start=utc_now(); p=log_dir/f"{spec.track}-{spec.name.replace(' ','_')}.log"
    proc=subprocess.run(spec.argv,capture_output=True,text=True)
    p.write_text(proc.stdout+'\n'+proc.stderr,encoding='utf-8')
    end=utc_now()
    return {'name':spec.name,'track':spec.track,'argv':spec.argv,'start_time_utc':start,'end_time_utc':end,'duration_seconds':0.0,'exit_code':proc.returncode,'log_path':str(p)}

def collect_environment(profile_mode:str)->dict[str,Any]:
    def cap(cmd:list[str])->str|None:
        try:return subprocess.run(cmd,capture_output=True,text=True,check=True).stdout.strip()
        except Exception:return None
    return {'schema_version':1,'git_sha':cap(['git','rev-parse','HEAD']),'git_branch':cap(['git','rev-parse','--abbrev-ref','HEAD']),'rustc':cap(['rustc','--version']),'cargo':cap(['cargo','--version']),'python':sys.executable,'target':platform.machine(),'os':platform.system(),'kernel':platform.release(),'cpu_model':platform.processor() or None,'physical_cores':None,'logical_cores':os.cpu_count(),'memory_gb':None,'build_profile':profile_mode,'features':[],'tokio_unstable':False,'timestamp_utc':utc_now()}

def write_commands_jsonl(path:Path, results:list[dict[str,Any]])->None:
    path.parent.mkdir(parents=True,exist_ok=True)
    path.write_text(''.join(json.dumps(r)+'\n' for r in results),encoding='utf-8')

def summarize_results(results:list[dict[str,Any]], profile:str, profile_mode:str, out_dir:Path, started:str, finished:str)->dict[str,Any]:
    failed=[r for r in results if r['exit_code']!=0]
    tracks={}
    for t in ['diagnostics','diagnostic_matrix','mitigation','runtime_cost','collector_limits','docs','cargo','operational']:
        rs=[r for r in results if r['track']==t]
        tracks[t]={'status':'skipped' if not rs else ('failed' if any(r['exit_code']!=0 for r in rs) else 'passed')}
    return {'schema_version':1,'profile':profile,'profile_mode':profile_mode,'out_dir':str(out_dir),'started_at_utc':started,'finished_at_utc':finished,'duration_seconds':0.0,'status':'failed' if failed else 'passed','commands':{'total':len(results),'passed':sum(1 for r in results if r['exit_code']==0),'failed':len(failed)},'tracks':tracks,'failed_commands':[{'name':r['name'],'exit_code':r['exit_code']} for r in failed]}

def write_summary(path:Path, summary:dict[str,Any])->None:
    path.write_text(json.dumps(summary,indent=2)+'\n',encoding='utf-8')

def write_scorecard(path:Path, summary:dict[str,Any])->None:
    lines=['# Tailtriage validation scorecard','',f"Profile: {summary['profile']}",f"Build profile: {summary['profile_mode']}",f"Status: {summary['status']}",f"Generated: {summary['finished_at_utc']}",'','Root cause is not proven by this validation suite. Runtime-cost numbers are machine/workload scoped. Collector-limit checks do not claim no drops. Generated outputs are local unless explicitly published.','']
    path.write_text('\n'.join(lines)+'\n',encoding='utf-8')

def main()->int:
    ap=argparse.ArgumentParser()
    ap.add_argument('--profile',choices=['smoke','ci','full','publish'],default='smoke')
    ap.add_argument('--out')
    ap.add_argument('--runs',type=int)
    ap.add_argument('--profile-mode',choices=['dev','release'],default='dev')
    ap.add_argument('--skip-cargo',action='store_true')
    ap.add_argument('--include-cargo',action='store_true')
    ap.add_argument('--no-fail-fast',action='store_true')
    ap.add_argument('--no-fail-thresholds',action='store_true')
    ap.add_argument('--dry-run',action='store_true')
    ap.add_argument('--python',default=sys.executable)
    args=ap.parse_args()
    if args.runs is None: args.runs={'smoke':1,'ci':1,'full':30,'publish':50}[args.profile]
    if args.out is None: args.out=str(derive_publish_dir() if args.profile=='publish' else default_out_dir(args.profile))
    validate_args(args)
    out=Path(args.out); plan=build_plan(args)
    if args.dry_run:
        for p in plan: print(' '.join(p.argv))
        return 0
    out.mkdir(parents=True,exist_ok=True); (out/'logs').mkdir(parents=True,exist_ok=True)
    env=collect_environment(args.profile_mode); (out/'environment.json').write_text(json.dumps(env,indent=2)+'\n',encoding='utf-8')
    started=utc_now(); results=[]
    for spec in plan:
        r=run_command(spec,out/'logs'); results.append(r)
        if r['exit_code']!=0 and not args.no_fail_fast: break
    write_commands_jsonl(out/'logs'/'commands.jsonl',results)
    finished=utc_now(); summary=summarize_results(results,args.profile,args.profile_mode,out,started,finished)
    write_summary(out/'summary.json',summary); write_scorecard(out/'scorecard.md',summary)
    return 0 if summary['status']=='passed' else 1

if __name__=='__main__':
    raise SystemExit(main())
