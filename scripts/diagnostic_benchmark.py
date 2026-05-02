#!/usr/bin/env python3
from __future__ import annotations
import argparse, json
from pathlib import Path

ALLOWED_GT={"application_queue_saturation","blocking_pool_pressure","executor_pressure_suspected","downstream_stage_dominates","insufficient_evidence"}
REQUIRED=("id","artifact","artifact_type","ground_truth","acceptable_top2","tags","must_include_evidence","allowed_warnings","notes")


def parse_args():
 p=argparse.ArgumentParser()
 p.add_argument("--manifest",required=True)
 p.add_argument("--output")
 p.add_argument("--min-top1",type=float,default=0.75)
 p.add_argument("--min-top2",type=float,default=0.90)
 return p.parse_args()

def load_manifest(path:Path):
 data=json.loads(path.read_text())
 if not isinstance(data,list): raise SystemExit("manifest must be a JSON array")
 seen=set()
 for c in data:
  for k in REQUIRED:
   if k not in c: raise SystemExit(f"manifest case missing required field: {k}")
  if c["id"] in seen: raise SystemExit(f"duplicate case id: {c['id']}")
  seen.add(c["id"])
  if c["artifact_type"]!="analysis_report": raise SystemExit(f"unsupported artifact_type: {c['artifact_type']}")
  if c["ground_truth"] not in ALLOWED_GT: raise SystemExit(f"unknown ground_truth: {c['ground_truth']}")
  if c["ground_truth"] not in c["acceptable_top2"]: raise SystemExit(f"acceptable_top2 must include ground_truth for {c['id']}")
 return data

def conf_bucket(v:str)->str:
 return v if v in {"low","medium","high"} else "unknown"

def run_case(root:Path, case:dict):
 rep=json.loads((root/case['artifact']).read_text())
 p=(rep.get('primary_suspect') or {})
 sk=[(s or {}).get('kind') for s in (rep.get('secondary_suspects') or [])]
 top2=[p.get('kind'), *sk[:1]]
 ev=list(p.get('evidence') or [])
 for s in rep.get('secondary_suspects') or []: ev.extend((s or {}).get('evidence') or [])
 warns=rep.get('warnings') or []
 return {
  'primary_kind':p.get('kind'),'primary_conf':p.get('confidence'),'primary_score':p.get('score'),
  'secondary_kinds':sk,'top2':top2,'evidence':ev,'warnings':warns,
 }

def main():
 a=parse_args(); mp=Path(a.manifest); root=Path.cwd()
 cases=load_manifest(mp)
 total=len(cases); top1=top2=ev_pass=0; unexpected=0; failed=[]
 per={}; conf={}; cm={}
 for c in cases:
  r=run_case(root,c)
  gt=c['ground_truth']; pred=r['primary_kind']
  per.setdefault(gt,{"cases":0,"top1_correct":0,"top2_correct":0}); per[gt]['cases']+=1
  cm.setdefault(gt,{})
  cm[gt][pred]=cm[gt].get(pred,0)+1
  ok1=pred==gt; ok2=any(k in c['acceptable_top2'] for k in r['top2'] if k)
  if ok1: top1+=1; per[gt]['top1_correct']+=1
  if ok2: top2+=1; per[gt]['top2_correct']+=1
  b=conf_bucket(r['primary_conf']); conf.setdefault(b,{"cases":0,"top1_correct":0}); conf[b]['cases']+=1
  if ok1: conf[b]['top1_correct']+=1
  ev_ok=all(any(req in e for e in r['evidence']) for req in c['must_include_evidence'])
  if ev_ok: ev_pass+=1
  allow=c['allowed_warnings'];
  if allow==["*"]: unexpected_case=0
  else: unexpected_case=sum(1 for w in r['warnings'] if not any(s in w for s in allow))
  unexpected += unexpected_case
  if (not ok2) or (not ev_ok) or unexpected_case>0:
   failed.append({"id":c['id'],"ground_truth":gt,"predicted_top1":pred,"top2":r['top2'],"evidence_ok":ev_ok,"unexpected_warnings":unexpected_case})
 m={"total_cases":total,"top1_accuracy":round(top1/total,6),"top2_recall":round(top2/total,6),"required_evidence_pass_rate":round(ev_pass/total,6),"unexpected_warning_count":unexpected,"per_ground_truth":per,"confidence_bucket_accuracy":conf,"confusion_matrix":cm,"failed_cases":failed}
 print(f"cases={total} top1={m['top1_accuracy']:.3f} top2={m['top2_recall']:.3f} evidence={m['required_evidence_pass_rate']:.3f} unexpected_warnings={unexpected}")
 if a.output:
  out=Path(a.output); out.parent.mkdir(parents=True,exist_ok=True); out.write_text(json.dumps(m,indent=2)+"\n")
 fail=False
 if m['top1_accuracy']<a.min_top1: print('top1 threshold failed'); fail=True
 if m['top2_recall']<a.min_top2: print('top2 threshold failed'); fail=True
 if m['required_evidence_pass_rate']<1.0: print('required evidence check failed'); fail=True
 if unexpected>0: print('unexpected warnings present'); fail=True
 if fail: raise SystemExit(1)

if __name__=='__main__': main()
