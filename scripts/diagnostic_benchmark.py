#!/usr/bin/env python3
from __future__ import annotations
import argparse, json
from collections import Counter, defaultdict
from pathlib import Path

ALLOWED_GT={"application_queue_saturation","blocking_pool_pressure","executor_pressure_suspected","downstream_stage_dominates","insufficient_evidence"}
CONF_ORDER=["low","medium","high"]

def _fail(msg):
    raise ValueError(msg)

def validate_manifest(m):
    if not isinstance(m,dict) or "cases" not in m or not isinstance(m["cases"],list): _fail("manifest must contain cases[]")
    ids=set()
    for i,c in enumerate(m["cases"]):
        for f in ["id","artifact","artifact_type","ground_truth","acceptable_top2","tags","must_include_evidence","allowed_warnings","notes"]:
            if f not in c: _fail(f"case[{i}] missing field: {f}")
        if c["id"] in ids: _fail(f"duplicate id: {c['id']}")
        ids.add(c["id"])
        if c["artifact_type"]!="analysis_report": _fail(f"unsupported artifact_type for {c['id']}: {c['artifact_type']}")
        if c["ground_truth"] not in ALLOWED_GT: _fail(f"unknown ground_truth for {c['id']}")
        if c["ground_truth"] not in c["acceptable_top2"]: _fail(f"acceptable_top2 must include ground_truth for {c['id']}")

def _evidence(report):
    lines=[]
    for s in [report.get("primary_suspect",{})]+report.get("secondary_suspects",[]):
        lines.extend([str(x) for x in s.get("evidence",[])])
    return lines

def _suspects(report):
    p=report.get("primary_suspect",{})
    sec=report.get("secondary_suspects",[])
    return [p.get("kind","")]+[s.get("kind","") for s in sec]

def run(manifest_path,min_top1,min_top2,output=None):
    root=manifest_path.parent
    m=json.loads(manifest_path.read_text())
    validate_manifest(m)
    total=len(m["cases"]); top1=top2=0; evpass=0; unexpected_warn=0; high_wrong=0
    gt_counts=Counter(); conf=defaultdict(lambda:{"total":0,"correct":0}); matrix=defaultdict(lambda:Counter()); failed=[]
    for c in m["cases"]:
        report=json.loads((root/c["artifact"]).read_text())
        primary=report.get("primary_suspect",{})
        pk=primary.get("kind","")
        confv=primary.get("confidence","low")
        top2k=_suspects(report)[:2]
        gt=c["ground_truth"]; gt_counts[gt]+=1; matrix[gt][pk]+=1
        is_top1=pk==gt
        is_top2=any(k in c["acceptable_top2"] for k in top2k)
        if is_top1: top1+=1
        if is_top2: top2+=1
        conf[confv]["total"]+=1
        if is_top1: conf[confv]["correct"]+=1
        if (not is_top1) and confv=="high": high_wrong+=1
        ev_all="\n".join(_evidence(report)).lower()
        missing=[s for s in c["must_include_evidence"] if s.lower() not in ev_all]
        ev_ok=not missing
        if ev_ok: evpass+=1
        warns=[str(w) for w in report.get("warnings",[])]
        disallowed=[w for w in warns if not any(a in w for a in c["allowed_warnings"])]
        if disallowed: unexpected_warn+=1
        if (not is_top2) or (not ev_ok) or disallowed:
            failed.append({"id":c["id"],"top2_ok":is_top2,"missing_evidence":missing,"unexpected_warnings":disallowed,"primary":pk,"ground_truth":gt})
    metrics={
      "total_cases":total,
      "top1_accuracy":top1/total if total else 0.0,
      "top2_recall":top2/total if total else 0.0,
      "high_confidence_wrong_count":high_wrong,
      "per_ground_truth":dict(gt_counts),
      "confusion_matrix":{k:dict(v) for k,v in matrix.items()},
      "confidence_bucket_accuracy":{k:{"total":v["total"],"accuracy":(v["correct"]/v["total"] if v["total"] else 0.0)} for k,v in conf.items()},
      "required_evidence_pass_rate":evpass/total if total else 0.0,
      "unexpected_warning_count":unexpected_warn,
      "failed_cases":failed,
    }
    print(f"cases={total} top1={metrics['top1_accuracy']:.3f} top2={metrics['top2_recall']:.3f} high_conf_wrong={high_wrong} evidence_pass={metrics['required_evidence_pass_rate']:.3f} unexpected_warnings={unexpected_warn}")
    if output:
      Path(output).parent.mkdir(parents=True,exist_ok=True)
      Path(output).write_text(json.dumps(metrics,indent=2,sort_keys=True)+"\n")
    if failed: raise SystemExit(2)
    if metrics["top1_accuracy"]<min_top1 or metrics["top2_recall"]<min_top2: raise SystemExit(3)
    return metrics

def main():
    ap=argparse.ArgumentParser()
    ap.add_argument("--manifest",required=True,type=Path)
    ap.add_argument("--output")
    ap.add_argument("--min-top1",type=float,default=0.75)
    ap.add_argument("--min-top2",type=float,default=0.90)
    a=ap.parse_args()
    try:
      run(a.manifest,a.min_top1,a.min_top2,a.output)
    except ValueError as e:
      print(f"manifest validation failed: {e}")
      raise SystemExit(1)

if __name__=="__main__": main()
