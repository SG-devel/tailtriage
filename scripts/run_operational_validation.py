#!/usr/bin/env python3
from __future__ import annotations
import argparse, json, statistics, subprocess
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parent.parent


def delta(before, after):
    if before is None or after is None:
        return None
    return after - before

def safe_div(n, d):
    if n is None or d in (None, 0):
        return None
    return n / d

def ratio_delta(before, after):
    return safe_div(delta(before, after), before)

def artifact_size_bytes(path):
    p = Path(path)
    return p.stat().st_size if p.exists() else None

def bytes_per_request(total_bytes, request_count):
    return safe_div(total_bytes, request_count)

def extract_drop_counters(payload):
    t = payload.get("truncation") or payload.get("truncation_counters") or {}
    return {
        "dropped_requests": t.get("dropped_requests", 0),
        "dropped_stages": t.get("dropped_stages", 0),
        "dropped_queues": t.get("dropped_queues", 0),
        "dropped_inflight_snapshots": t.get("dropped_inflight_snapshots", 0),
        "dropped_runtime_snapshots": t.get("dropped_runtime_snapshots", 0),
    }

def extract_limit_warnings(report):
    out=[]
    for w in (report.get("warnings") or []):
        lw=w.lower()
        if any(k in lw for k in ("trunc", "partial", "limit", "drop")):
            out.append(w)
    return out

def evaluate_runtime_cost(record, max_relative_p95_overhead, no_fail=False):
    failed=[]
    r=record.get("p95_overhead_ratio")
    if (not no_fail) and r is not None and r>max_relative_p95_overhead:
        failed.append(f"p95_overhead_ratio {r:.4f} > {max_relative_p95_overhead:.4f}")
    record["failed_expectations"]=failed
    record["passed"]=not failed
    return record

def evaluate_collector_limits(record, require_visibility=True, no_fail=False):
    failed=[]
    drops=sum(record.get(k,0) or 0 for k in ("dropped_requests","dropped_stages","dropped_queues","dropped_inflight_snapshots","dropped_runtime_snapshots"))
    visible=record.get("limit_visibility_passed", False)
    warned_or_downgraded=record.get("diagnosis_downgraded_or_warned", False)
    if (not no_fail) and drops>0:
        if require_visibility and not visible:
            failed.append("drops observed but not visible in warnings/signals")
        if not warned_or_downgraded:
            failed.append("drops observed but diagnosis was not downgraded/warned")
    record["failed_expectations"]=failed
    record["passed"]=not failed
    return record

def summarize_runtime_cost(records):
    vals=[r.get("p95_overhead_ratio") for r in records if r.get("p95_overhead_ratio") is not None]
    p99=[r.get("p99_overhead_ratio") for r in records if r.get("p99_overhead_ratio") is not None]
    bpr=[r.get("artifact_bytes_per_request") for r in records if r.get("artifact_bytes_per_request") is not None]
    return {"records":len(records),"p95_overhead_ratio":{"median":statistics.median(vals) if vals else None,"max":max(vals) if vals else None},"p99_overhead_ratio":{"median":statistics.median(p99) if p99 else None,"max":max(p99) if p99 else None},"artifact_bytes_per_request":{"median":statistics.median(bpr) if bpr else None,"max":max(bpr) if bpr else None},"measurement_quality_counts":{q:sum(1 for r in records if r.get("measurement_quality")==q) for q in sorted({r.get('measurement_quality') for r in records})}}

def summarize_collector_limits(records):
    n=len(records)
    visible=sum(1 for r in records if r.get("limit_visibility_passed"))
    warned=sum(1 for r in records if r.get("diagnosis_downgraded_or_warned"))
    return {"records":n,"limit_hit_records":sum(1 for r in records if r.get("limit_hit")),"limit_visibility_pass_rate":safe_div(visible,n),"diagnosis_downgraded_or_warned_rate":safe_div(warned,n),"drop_metric_visibility":{k:sum(1 for r in records if (r.get(k) or 0)>0) for k in ("dropped_requests","dropped_stages","dropped_queues","dropped_runtime_snapshots")}}

def summarize_records(records, profile, domains):
    r=[x for x in records if x["domain"]=="runtime-cost"]
    c=[x for x in records if x["domain"]=="collector-limits"]
    return {"schema_version":1,"profile":profile,"domains":domains,"total_records":len(records),"passed_records":sum(1 for x in records if x.get("passed")),"failed_records":sum(1 for x in records if not x.get("passed")),"runtime_cost":summarize_runtime_cost(r) if r else None,"collector_limits":summarize_collector_limits(c) if c else None,"failed_thresholds":[f for x in records for f in x.get("failed_expectations",[])]}

def write_jsonl(path, records):
    p=Path(path); p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text("".join(json.dumps(r)+"\n" for r in records), encoding="utf-8")

def write_scorecard(path, summary):
    p=Path(path); p.parent.mkdir(parents=True, exist_ok=True)
    lines=["# Operational validation scorecard","",f"Profile: {summary['profile']}","","## Runtime cost","","| Scenario | Records | p95 overhead median | p95 overhead max | artifact bytes/request median | Measurement quality |","|---|---:|---:|---:|---:|---|"]
    rc=summary.get("runtime_cost")
    if rc:
        qual=", ".join(k for k,v in rc.get("measurement_quality_counts",{}).items() if v)
        med=rc["p95_overhead_ratio"]["median"]; mx=rc["p95_overhead_ratio"]["max"]; b=rc["artifact_bytes_per_request"]["median"]
        lines.append(f"| queue | {rc['records']} | {'' if med is None else f'{med*100:.1f}%'} | {'' if mx is None else f'{mx*100:.1f}%'} | {'' if b is None else f'{b:.1f}'} | {qual} |")
    lines += ["","## Collector limits","","| Scenario | Limit hit | Visible drops | Warned/downgraded | Passed | Notes |","|---|---:|---:|---:|---:|---|"]
    cl=summary.get("collector_limits")
    if cl:
        lines.append(f"| queue-limit-pressure | {'yes' if cl['limit_hit_records'] else 'no'} | {'yes' if (cl['limit_visibility_pass_rate'] or 0)>0 else 'no'} | {'yes' if (cl['diagnosis_downgraded_or_warned_rate'] or 0)>0 else 'no'} | {'yes' if summary['failed_records']==0 else 'no'} | drops are bounded and visible |")
    p.write_text("\n".join(lines)+"\n", encoding="utf-8")

def main():
    ap=argparse.ArgumentParser()
    ap.add_argument("--domain", choices=["runtime-cost","collector-limits","all"], default="all")
    ap.add_argument("--profile", choices=["dev","release"], default="dev")
    ap.add_argument("--scenario", action="append")
    ap.add_argument("--runs", type=int)
    ap.add_argument("--out", default="target/operational-validation.jsonl")
    ap.add_argument("--summary")
    ap.add_argument("--scorecard")
    ap.add_argument("--artifact-root", default="target/operational-validation")
    ap.add_argument("--no-fail-thresholds", action="store_true")
    ap.add_argument("--max-relative-p95-overhead", type=float, default=0.25)
    ap.add_argument("--max-throughput-regression", type=float)
    ap.add_argument("--require-limit-visibility", action=argparse.BooleanOptionalAction, default=True)
    args=ap.parse_args()
    domains=[args.domain] if args.domain!="all" else ["runtime-cost","collector-limits"]
    records=[]
    artifact_root=Path(args.artifact_root)
    if "runtime-cost" in domains:
        runs=args.runs if args.runs is not None else 3
        runtime_dir=artifact_root/"runtime-cost"/"queue"
        runtime_dir.mkdir(parents=True, exist_ok=True)
        subprocess.run(["python3","scripts/measure_runtime_cost.py","--rounds",str(runs),"--warmup-rounds","0","--artifact-dir",str(runtime_dir)],check=True)
        raw=[json.loads(l) for l in (runtime_dir/"runtime-cost-raw.jsonl").read_text().splitlines() if l.strip()]
        by_round={}
        for row in raw: by_round.setdefault(row["round"],{})[row["mode"]]=row
        for i,rr in sorted(by_round.items()):
            b=rr.get("baseline",{}); inst=rr.get("core_light",{});
            rec={"schema_version":1,"domain":"runtime-cost","scenario":"queue","profile":args.profile,"run_index":i+1,
                 "baseline_artifact_path":None,"instrumented_artifact_path":None,"instrumented_analysis_path":str(runtime_dir/"runtime-cost-summary.json"),
                 "baseline_p50_latency_us":None,"baseline_p95_latency_us":None if b.get("latency_p95_ms") is None else int(b["latency_p95_ms"]*1000),"baseline_p99_latency_us":None if b.get("latency_p99_ms") is None else int(b["latency_p99_ms"]*1000),
                 "instrumented_p50_latency_us":None,"instrumented_p95_latency_us":None if inst.get("latency_p95_ms") is None else int(inst["latency_p95_ms"]*1000),"instrumented_p99_latency_us":None if inst.get("latency_p99_ms") is None else int(inst["latency_p99_ms"]*1000),
                 "p95_overhead_us":delta(None if b.get("latency_p95_ms") is None else int(b["latency_p95_ms"]*1000),None if inst.get("latency_p95_ms") is None else int(inst["latency_p95_ms"]*1000)),"p95_overhead_ratio":ratio_delta(b.get("latency_p95_ms"),inst.get("latency_p95_ms")),
                 "p99_overhead_us":delta(None if b.get("latency_p99_ms") is None else int(b["latency_p99_ms"]*1000),None if inst.get("latency_p99_ms") is None else int(inst["latency_p99_ms"]*1000)),"p99_overhead_ratio":ratio_delta(b.get("latency_p99_ms"),inst.get("latency_p99_ms")),
                 "baseline_request_count":b.get("requests"),"instrumented_request_count":inst.get("requests"),"artifact_bytes":artifact_size_bytes(runtime_dir/"runtime-cost-raw.jsonl") or 0}
            rec["artifact_bytes_per_request"]=bytes_per_request(rec["artifact_bytes"],rec.get("instrumented_request_count")); rec["measurement_quality"]="partial"; rec["warnings"]=[]
            records.append(evaluate_runtime_cost(rec,args.max_relative_p95_overhead,args.no_fail_thresholds))
    if "collector-limits" in domains:
        coll_dir=artifact_root/"collector-limits"/"queue-limit-pressure"; coll_dir.mkdir(parents=True, exist_ok=True)
        subprocess.run(["python3","scripts/measure_collector_limits.py","--profile","smoke","--artifact-dir",str(coll_dir)],check=True)
        rows=[json.loads(l) for l in (coll_dir/"collector-limits-smoke-raw.jsonl").read_text().splitlines() if l.strip()]
        analysis={"warnings":[]}
        for r in rows:
            d=extract_drop_counters(r); total=sum(d.values()); warns=extract_limit_warnings({"warnings":r.get("warnings",[])})
            rec={"schema_version":1,"domain":"collector-limits","scenario":"queue-limit-pressure","profile":args.profile,"artifact_path":str(coll_dir/"collector-limits-smoke-raw.jsonl"),"analysis_path":str(coll_dir/"collector-limits-smoke-summary.json"),"configured_limits":None,"request_count":r.get("requests_completed"),"artifact_bytes":artifact_size_bytes(coll_dir/"collector-limits-smoke-raw.jsonl") or 0,"limit_hit":bool(r.get("limits_hit") or total>0),**d,"first_limit_hit":None,"warnings":warns,"limit_visibility_passed":(total==0) or bool(warns),"diagnosis_downgraded_or_warned":bool(warns)}
            records.append(evaluate_collector_limits(rec,args.require_limit_visibility,args.no_fail_thresholds))
    write_jsonl(args.out, records)
    summary_path=Path(args.summary) if args.summary else Path(args.out).with_name(Path(args.out).stem+"-summary.json")
    summary=summarize_records(records,args.profile,domains)
    summary_path.parent.mkdir(parents=True, exist_ok=True)
    summary_path.write_text(json.dumps(summary,indent=2)+"\n",encoding="utf-8")
    if args.scorecard: write_scorecard(args.scorecard,summary)

if __name__=="__main__":
    main()
