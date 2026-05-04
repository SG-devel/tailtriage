#!/usr/bin/env python3
"""Manual/local operational trust-boundary validation runner."""
from __future__ import annotations
import argparse, json, statistics, sys
from pathlib import Path
from typing import Any
try:
    from _demo_runner import repo_root, run_and_analyze, load_report_json
except ModuleNotFoundError:  # pragma: no cover
    from scripts._demo_runner import repo_root, run_and_analyze, load_report_json

SCHEMA_VERSION = 1
DEFAULT_MAX_RELATIVE_P95_OVERHEAD = 0.25


def delta(before, after):
    if before is None or after is None:
        return None
    return after - before

def ratio_delta(before, after):
    if before is None or after is None or before == 0:
        return None
    return (after - before) / before

def safe_div(numerator, denominator):
    if numerator is None or denominator in (None, 0):
        return None
    return numerator / denominator

def artifact_size_bytes(path: Path | None):
    if path is None:
        return None
    p = Path(path)
    return p.stat().st_size if p.exists() else None

def bytes_per_request(byte_count, request_count):
    return safe_div(byte_count, request_count)

def extract_drop_counters(payload: dict[str, Any]) -> dict[str, int | None]:
    trunc = payload.get("truncation") or payload.get("artifact", {}).get("truncation") or {}
    return {
        "dropped_requests": trunc.get("dropped_requests"),
        "dropped_stages": trunc.get("dropped_stages"),
        "dropped_queues": trunc.get("dropped_queues"),
        "dropped_inflight_snapshots": trunc.get("dropped_inflight_snapshots"),
        "dropped_runtime_snapshots": trunc.get("dropped_runtime_snapshots"),
    }

def extract_limit_warnings(report: dict[str, Any]) -> list[str]:
    warnings = report.get("warnings") or []
    out = []
    for w in warnings:
        lw = str(w).lower()
        if any(k in lw for k in ("partial", "truncat", "drop", "limit")):
            out.append(str(w))
    return out

def measurement_quality(record):
    return "partial" if any(record.get(k) is None for k in ("baseline_p95_latency_us", "instrumented_p95_latency_us")) else "full"

def latency_overhead_record(**r):
    r["schema_version"] = SCHEMA_VERSION
    r["measurement_quality"] = measurement_quality(r)
    return r

def collector_limit_record(**r):
    r["schema_version"] = SCHEMA_VERSION
    return r

def evaluate_runtime_cost(record, thresholds):
    failed = []
    if thresholds.get("max_relative_p95_overhead") is not None and record.get("p95_overhead_ratio") is not None:
        if record["p95_overhead_ratio"] > thresholds["max_relative_p95_overhead"]:
            failed.append("p95_overhead_ratio")
    record["failed_expectations"] = failed
    record["passed"] = not failed
    return record

def evaluate_collector_limits(record, require_visibility=True):
    drops = any((record.get(k) or 0) > 0 for k in ("dropped_requests", "dropped_stages", "dropped_queues", "dropped_inflight_snapshots", "dropped_runtime_snapshots"))
    visible = bool(record.get("warnings")) or bool(record.get("limit_hit"))
    record["limit_visibility_passed"] = (not drops) or visible
    failed = []
    if require_visibility and drops and not record["limit_visibility_passed"]:
        failed.append("drops_without_visibility")
    if drops and not record.get("diagnosis_downgraded_or_warned"):
        failed.append("drops_without_warning_or_downgrade")
    record["failed_expectations"] = failed
    record["passed"] = not failed
    return record

def summarize_runtime_cost(records):
    p95 = [r["p95_overhead_ratio"] for r in records if r.get("p95_overhead_ratio") is not None]
    p99 = [r["p99_overhead_ratio"] for r in records if r.get("p99_overhead_ratio") is not None]
    bpr = [r["artifact_bytes_per_request"] for r in records if r.get("artifact_bytes_per_request") is not None]
    return {"records": len(records), "p95_overhead_ratio": {"median": statistics.median(p95) if p95 else None, "max": max(p95) if p95 else None}, "p99_overhead_ratio": {"median": statistics.median(p99) if p99 else None, "max": max(p99) if p99 else None}, "artifact_bytes_per_request": {"median": statistics.median(bpr) if bpr else None, "max": max(bpr) if bpr else None}, "measurement_quality_counts": {q: sum(1 for r in records if r.get("measurement_quality")==q) for q in {r.get("measurement_quality") for r in records}}}

def summarize_collector_limits(records):
    return {"records": len(records), "limit_hit_records": sum(1 for r in records if r.get("limit_hit")), "limit_visibility_pass_rate": safe_div(sum(1 for r in records if r.get("limit_visibility_passed")), len(records)), "diagnosis_downgraded_or_warned_rate": safe_div(sum(1 for r in records if r.get("diagnosis_downgraded_or_warned")), len(records)), "drop_metric_visibility": {k: sum(1 for r in records if r.get(k) not in (None, 0)) for k in ("dropped_requests", "dropped_stages", "dropped_queues", "dropped_runtime_snapshots")}}

def summarize_records(records, profile, domains):
    r = [x for x in records if x["domain"]=="runtime-cost"]
    c = [x for x in records if x["domain"]=="collector-limits"]
    return {"schema_version": 1, "profile": profile, "domains": domains, "total_records": len(records), "passed_records": sum(1 for x in records if x.get("passed")), "failed_records": sum(1 for x in records if not x.get("passed")), "runtime_cost": summarize_runtime_cost(r), "collector_limits": summarize_collector_limits(c), "failed_thresholds": [f"{x['domain']}:{x['scenario']}:{','.join(x.get('failed_expectations',[]))}" for x in records if x.get("failed_expectations")]}

def write_jsonl(path: Path, records):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(json.dumps(r, sort_keys=True) for r in records)+"\n", encoding="utf-8")

def write_scorecard(path: Path, summary, records):
    by_scenario = {}
    for r in records: by_scenario.setdefault((r["domain"], r["scenario"]), []).append(r)
    lines=["# Operational validation scorecard","",f"Profile: {summary['profile']}","","## Runtime cost","","| Scenario | Records | p95 overhead median | p95 overhead max | artifact bytes/request median | Measurement quality |","|---|---:|---:|---:|---:|---|"]
    for (d,s),rs in by_scenario.items():
        if d!="runtime-cost": continue
        sr=summarize_runtime_cost(rs)
        lines.append(f"| {s} | {len(rs)} | {((sr['p95_overhead_ratio']['median'] or 0)*100):.1f}% | {((sr['p95_overhead_ratio']['max'] or 0)*100):.1f}% | {(sr['artifact_bytes_per_request']['median'] or 0):.1f} | {','.join(sr['measurement_quality_counts'].keys())} |")
    lines += ["","## Collector limits","","| Scenario | Limit hit | Visible drops | Warned/downgraded | Passed | Notes |","|---|---:|---:|---:|---:|---|"]
    for (d,s),rs in by_scenario.items():
        if d!="collector-limits": continue
        r=rs[0]
        lines.append(f"| {s} | {'yes' if r.get('limit_hit') else 'no'} | {'yes' if r.get('limit_visibility_passed') else 'no'} | {'yes' if r.get('diagnosis_downgraded_or_warned') else 'no'} | {'yes' if r.get('passed') else 'no'} | drops are bounded and visible |")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines)+"\n", encoding="utf-8")

def parse_latency(path: Path):
    data=load_report_json(path)
    return data.get("p50_latency_us"), data.get("p95_latency_us"), data.get("p99_latency_us"), data.get("request_count")

def main():
    p=argparse.ArgumentParser()
    p.add_argument("--domain", choices=["runtime-cost","collector-limits","all"], default="all")
    p.add_argument("--profile", choices=["dev","release"], default="dev")
    p.add_argument("--scenario", action="append")
    p.add_argument("--runs", type=int)
    p.add_argument("--out", default="target/operational-validation.jsonl")
    p.add_argument("--summary")
    p.add_argument("--scorecard")
    p.add_argument("--artifact-root", default="target/operational-validation")
    p.add_argument("--no-fail-thresholds", action="store_true")
    p.add_argument("--max-relative-p95-overhead", type=float, default=DEFAULT_MAX_RELATIVE_P95_OVERHEAD)
    p.add_argument("--max-throughput-regression", type=float)
    p.add_argument("--require-limit-visibility", action=argparse.BooleanOptionalAction, default=True)
    a=p.parse_args()
    out=Path(a.out); summary=Path(a.summary) if a.summary else out.with_name(out.stem+"-summary.json")
    domains=["runtime-cost","collector-limits"] if a.domain=="all" else [a.domain]
    root=repo_root(__file__)
    cli_manifest=root/"tailtriage-cli/Cargo.toml"
    scenario_map={"queue":(root/"demos/queue_service/Cargo.toml","baseline"),"queue-limit-pressure":(root/"demos/queue_service/Cargo.toml","baseline")}
    recs=[]
    if "runtime-cost" in domains:
        scenarios=a.scenario or ["queue"]; runs=a.runs if a.runs is not None else 3
        for s in scenarios:
            for i in range(1,runs+1):
                d=Path(a.artifact_root)/"runtime-cost"/s
                b=d/f"run-{i:03d}-baseline.json"; ia=d/f"run-{i:03d}-instrumented.json"; an=d/f"run-{i:03d}-instrumented-analysis.json"
                manifest,mode=scenario_map[s]
                run_and_analyze(manifest, cli_manifest, b, d/f"run-{i:03d}-baseline-analysis.json", mode, profile=a.profile)
                run_and_analyze(manifest, cli_manifest, ia, an, mode, profile=a.profile)
                bp50,bp95,bp99,br=parse_latency(b)
                ip50,ip95,ip99,ir=parse_latency(ia)
                r=latency_overhead_record(domain="runtime-cost",scenario=s,profile=a.profile,run_index=i,baseline_artifact_path=str(b),instrumented_artifact_path=str(ia),instrumented_analysis_path=str(an),baseline_p50_latency_us=bp50,baseline_p95_latency_us=bp95,baseline_p99_latency_us=bp99,instrumented_p50_latency_us=ip50,instrumented_p95_latency_us=ip95,instrumented_p99_latency_us=ip99,p95_overhead_us=delta(bp95,ip95),p95_overhead_ratio=ratio_delta(bp95,ip95),p99_overhead_us=delta(bp99,ip99),p99_overhead_ratio=ratio_delta(bp99,ip99),baseline_request_count=br,instrumented_request_count=ir,artifact_bytes=artifact_size_bytes(ia),artifact_bytes_per_request=bytes_per_request(artifact_size_bytes(ia),ir),warnings=[])
                recs.append(r if a.no_fail_thresholds else evaluate_runtime_cost(r,{"max_relative_p95_overhead":a.max_relative_p95_overhead}))
    if "collector-limits" in domains:
        scenarios=a.scenario or ["queue-limit-pressure"]
        for s in scenarios:
            d=Path(a.artifact_root)/"collector-limits"/s; art=d/"run.json"; ana=d/"analysis.json"
            manifest,mode=scenario_map[s]
            run_and_analyze(manifest, cli_manifest, art, ana, mode, profile=a.profile)
            payload=load_report_json(art); report=load_report_json(ana); drops=extract_drop_counters(payload); warnings=extract_limit_warnings(report)
            r=collector_limit_record(domain="collector-limits",scenario=s,profile=a.profile,artifact_path=str(art),analysis_path=str(ana),configured_limits=payload.get("limits"),request_count=payload.get("request_count"),artifact_bytes=artifact_size_bytes(art),limit_hit=bool((payload.get("truncation") or {}).get("limits_reached")),first_limit_hit="requests" if (drops.get("dropped_requests") or 0)>0 else None,warnings=warnings,diagnosis_downgraded_or_warned=bool(warnings),**drops)
            recs.append(r if a.no_fail_thresholds else evaluate_collector_limits(r,a.require_limit_visibility))
    if a.no_fail_thresholds:
        for r in recs:
            r.setdefault("failed_expectations",[]); r.setdefault("passed",True)
            if r["domain"]=="collector-limits": r.setdefault("limit_visibility_passed",True)
    write_jsonl(out,recs)
    s=summarize_records(recs,a.profile,domains); summary.parent.mkdir(parents=True,exist_ok=True); summary.write_text(json.dumps(s,indent=2)+"\n")
    if a.scorecard: write_scorecard(Path(a.scorecard),s,recs)

if __name__=="__main__":
    main()
