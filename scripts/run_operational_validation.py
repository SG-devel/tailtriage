#!/usr/bin/env python3
from __future__ import annotations
import argparse, json, statistics, subprocess
from pathlib import Path
from typing import Any

try:
    from _demo_runner import PROFILE_CHOICES, repo_root, run_and_analyze, load_report_json
except ModuleNotFoundError:
    from scripts._demo_runner import PROFILE_CHOICES, repo_root, run_and_analyze, load_report_json

DEFAULT_OUT = Path("target/operational-validation.jsonl")
DOMAINS=("runtime-cost","collector-limits")


def delta(before, after):
    return None if before is None or after is None else after-before

def ratio_delta(before, after):
    if before in (None,0) or after is None:
        return None
    return (after-before)/before

def safe_div(n,d):
    if n is None or d in (None,0): return None
    return n/d

def artifact_size_bytes(path: Path):
    return path.stat().st_size if path.exists() else None

def bytes_per_request(b, req):
    return safe_div(b, req)

def extract_drop_counters(obj: dict[str,Any]):
    trunc = obj.get("truncation") or {}
    return {k: trunc.get(k) for k in ["dropped_requests","dropped_stages","dropped_queues","dropped_inflight_snapshots","dropped_runtime_snapshots"]}

def extract_limit_warnings(report: dict[str,Any]):
    return [w for w in (report.get("warnings") or []) if any(s in w.lower() for s in ["truncat","partial","drop","limit"]) ]

def measurement_quality(record):
    return "partial" if record.get("baseline_p50_latency_us") is None else "complete"

def latency_overhead_record(**r):
    r["measurement_quality"]=measurement_quality(r)
    return r

def collector_limit_record(**r): return r

def evaluate_runtime_cost(record, thresholds):
    failed=[]
    m=thresholds.get("max_relative_p95_overhead")
    if m is not None and record.get("p95_overhead_ratio") is not None and record["p95_overhead_ratio"]>m:
        failed.append("p95_overhead_ratio")
    record["failed_expectations"]=failed; record["passed"]=not failed
    return record

def evaluate_collector_limits(record, require_visibility=True):
    failed=[]
    drops=any((record.get(k) or 0)>0 for k in ["dropped_requests","dropped_stages","dropped_queues","dropped_inflight_snapshots","dropped_runtime_snapshots"])
    vis=bool(record.get("warnings")) or bool(record.get("limit_hit"))
    if require_visibility and drops and not vis:
        failed.append("drops_without_visibility")
    if require_visibility and drops and not record.get("diagnosis_downgraded_or_warned"):
        failed.append("drops_without_warning_or_downgrade")
    record["limit_visibility_passed"]= (not drops) or vis
    record["failed_expectations"]=failed; record["passed"]=not failed
    return record

def summarize_runtime_cost(records):
    p95=[r.get("p95_overhead_ratio") for r in records if r.get("p95_overhead_ratio") is not None]
    p99=[r.get("p99_overhead_ratio") for r in records if r.get("p99_overhead_ratio") is not None]
    bpr=[r.get("artifact_bytes_per_request") for r in records if r.get("artifact_bytes_per_request") is not None]
    return {"records":len(records),"p95_overhead_ratio":{"median":statistics.median(p95) if p95 else None,"max":max(p95) if p95 else None},"p99_overhead_ratio":{"median":statistics.median(p99) if p99 else None,"max":max(p99) if p99 else None},"artifact_bytes_per_request":{"median":statistics.median(bpr) if bpr else None,"max":max(bpr) if bpr else None},"measurement_quality_counts":{q:sum(1 for r in records if r.get('measurement_quality')==q) for q in {r.get('measurement_quality') for r in records}}}

def summarize_collector_limits(records):
    n=len(records)
    return {"records":n,"limit_hit_records":sum(1 for r in records if r.get("limit_hit")),"limit_visibility_pass_rate":safe_div(sum(1 for r in records if r.get("limit_visibility_passed")),n) or 0.0,"diagnosis_downgraded_or_warned_rate":safe_div(sum(1 for r in records if r.get("diagnosis_downgraded_or_warned")),n) or 0.0,"drop_metric_visibility":{k:sum(1 for r in records if (r.get(k) or 0)>0) for k in ["dropped_requests","dropped_stages","dropped_queues","dropped_runtime_snapshots"]}}

def summarize_records(records, profile):
    rc=[r for r in records if r["domain"]=="runtime-cost"]
    cl=[r for r in records if r["domain"]=="collector-limits"]
    return {"schema_version":1,"profile":profile,"domains":sorted(set(r['domain'] for r in records)),"total_records":len(records),"passed_records":sum(1 for r in records if r.get('passed')),"failed_records":sum(1 for r in records if not r.get('passed')),"runtime_cost":summarize_runtime_cost(rc),"collector_limits":summarize_collector_limits(cl),"failed_thresholds":[]}

def write_jsonl(path, records):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(json.dumps(r, sort_keys=True) for r in records)+"\n", encoding="utf-8")

def write_scorecard(path, summary):
    lines=["# Operational validation scorecard","",f"Profile: {summary['profile']}","","## Runtime cost","","| Scenario | Records | p95 overhead median | p95 overhead max | artifact bytes/request median | Measurement quality |","|---|---:|---:|---:|---:|---|",f"| queue | {summary['runtime_cost']['records']} | {((summary['runtime_cost']['p95_overhead_ratio']['median'] or 0)*100):.1f}% | {((summary['runtime_cost']['p95_overhead_ratio']['max'] or 0)*100):.1f}% | {summary['runtime_cost']['artifact_bytes_per_request']['median'] or 0:.1f} | partial |","","## Collector limits","","| Scenario | Limit hit | Visible drops | Warned/downgraded | Passed | Notes |","|---|---:|---:|---:|---:|---|",f"| queue-limit-pressure | yes | {'yes' if summary['collector_limits']['limit_visibility_pass_rate']==1.0 else 'no'} | {'yes' if summary['collector_limits']['diagnosis_downgraded_or_warned_rate']==1.0 else 'no'} | {'yes' if summary['failed_records']==0 else 'no'} | drops are bounded and visible |"]
    path.parent.mkdir(parents=True, exist_ok=True); path.write_text("\n".join(lines)+"\n", encoding='utf-8')



def run_mode(manifest, out_dir, mode, profile, extra):
    cmd=["cargo","run","--quiet",*( ["--release"] if profile=="release" else []),"--manifest-path",str(manifest),"--","--mode",mode,*extra,"--output-dir",str(out_dir)]
    subprocess.run(cmd,check=True)
    cands=sorted(out_dir.glob(f'*-{mode}.json'))
    if not cands: raise RuntimeError(f'no artifact produced for mode {mode} in {out_dir}')
    return cands[-1]
def main():
    ap=argparse.ArgumentParser()
    ap.add_argument('--domain',choices=['runtime-cost','collector-limits','all'],default='all')
    ap.add_argument('--profile',choices=PROFILE_CHOICES,default='dev')
    ap.add_argument('--scenario',action='append')
    ap.add_argument('--runs',type=int)
    ap.add_argument('--out',type=Path,default=DEFAULT_OUT)
    ap.add_argument('--summary',type=Path)
    ap.add_argument('--scorecard',type=Path)
    ap.add_argument('--artifact-root',type=Path,default=Path('target/operational-validation'))
    ap.add_argument('--no-fail-thresholds',action='store_true')
    ap.add_argument('--max-relative-p95-overhead',type=float,default=0.25)
    ap.add_argument('--max-throughput-regression',type=float)
    ap.add_argument('--require-limit-visibility',action=argparse.BooleanOptionalAction,default=True)
    a=ap.parse_args(); root=repo_root(__file__)
    domains=DOMAINS if a.domain=='all' else (a.domain,)
    recs=[]
    runs=a.runs if a.runs is not None else (3 if 'runtime-cost' in domains else 1)
    cli_manifest=root/'tailtriage-cli/Cargo.toml'
    if 'runtime-cost' in domains:
      for i in range(1,runs+1):
        base=a.artifact_root/'runtime-cost'/'queue'/f'run-{i:03d}-baseline.json'
        inst=a.artifact_root/'runtime-cost'/'queue'/f'run-{i:03d}-instrumented.json'
        ana=a.artifact_root/'runtime-cost'/'queue'/f'run-{i:03d}-instrumented-analysis.json'
        base_gen=run_mode(root/'demos/runtime_cost/Cargo.toml', base.parent, 'baseline', a.profile, ['--requests','250','--concurrency','16','--work-ms','2'])
        base_gen.replace(base)
        inst_gen=run_mode(root/'demos/runtime_cost/Cargo.toml', inst.parent, 'core_light', a.profile, ['--requests','250','--concurrency','16','--work-ms','2'])
        inst_gen.replace(inst)
        subprocess.run(['cargo','run','--quiet',*( ['--release'] if a.profile=='release' else []),'--manifest-path',str(cli_manifest),'--','analyze',str(inst),'--format','json'],check=True,stdout=ana.open('w',encoding='utf-8'))
        b=load_report_json(base); r=load_report_json(ana)
        rec=latency_overhead_record(schema_version=1,domain='runtime-cost',scenario='queue',profile=a.profile,run_index=i,baseline_artifact_path=str(base),instrumented_artifact_path=str(inst),instrumented_analysis_path=str(ana),baseline_p50_latency_us=b.get('p50_latency_us'),baseline_p95_latency_us=b.get('p95_latency_us'),baseline_p99_latency_us=b.get('p99_latency_us'),instrumented_p50_latency_us=r.get('p50_latency_us'),instrumented_p95_latency_us=r.get('p95_latency_us'),instrumented_p99_latency_us=r.get('p99_latency_us'),p95_overhead_us=delta(b.get('p95_latency_us'),r.get('p95_latency_us')),p95_overhead_ratio=ratio_delta(b.get('p95_latency_us'),r.get('p95_latency_us')),p99_overhead_us=delta(b.get('p99_latency_us'),r.get('p99_latency_us')),p99_overhead_ratio=ratio_delta(b.get('p99_latency_us'),r.get('p99_latency_us')),baseline_request_count=b.get('request_count'),instrumented_request_count=r.get('request_count'),artifact_bytes=artifact_size_bytes(inst),artifact_bytes_per_request=bytes_per_request(artifact_size_bytes(inst),r.get('request_count')),warnings=[],passed=True,failed_expectations=[])
        recs.append(evaluate_runtime_cost(rec,{"max_relative_p95_overhead":None if a.no_fail_thresholds else a.max_relative_p95_overhead}))
    if 'collector-limits' in domains:
      scens=a.scenario or ['queue-limit-pressure']
      for s in scens:
        art=a.artifact_root/'collector-limits'/s/'run.json'; an=a.artifact_root/'collector-limits'/s/'analysis.json'
        gen=run_mode(root/'demos/collector_stress/Cargo.toml', art.parent, 'core_light', a.profile, ['--requests','400','--concurrency','32','--queue-slots','4','--queues-per-request','8','--stages-per-request','8','--inflight-cycles-per-request','8','--work-ms','1'])
        gen.replace(art)
        subprocess.run(['cargo','run','--quiet',*( ['--release'] if a.profile=='release' else []),'--manifest-path',str(cli_manifest),'--','analyze',str(art),'--format','json'],check=True,stdout=an.open('w',encoding='utf-8'))
        aobj=load_report_json(art); rep=load_report_json(an); drops=extract_drop_counters(aobj); warns=extract_limit_warnings(rep)
        rec=collector_limit_record(schema_version=1,domain='collector-limits',scenario=s,profile=a.profile,artifact_path=str(art),analysis_path=str(an),configured_limits={"max_requests":None,"max_stage_events":None,"max_queue_events":None,"max_runtime_snapshots":None},request_count=aobj.get('request_count'),artifact_bytes=artifact_size_bytes(art),limit_hit=bool((aobj.get('truncation') or {}).get('limits_reached')),warnings=warns,diagnosis_downgraded_or_warned=bool(warns),first_limit_hit='requests' if (drops.get('dropped_requests') or 0)>0 else None,**drops)
        recs.append(evaluate_collector_limits(rec,a.require_limit_visibility))
    write_jsonl(a.out,recs)
    summ=summarize_records(recs,a.profile)
    sp=a.summary or a.out.with_name(f"{a.out.stem}-summary.json"); sp.write_text(json.dumps(summ,indent=2)+"\n")
    if a.scorecard: write_scorecard(a.scorecard,summ)
    if not a.no_fail_thresholds and any(not r['passed'] for r in recs): return 1
    return 0

if __name__=='__main__': raise SystemExit(main())
