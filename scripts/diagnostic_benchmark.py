#!/usr/bin/env python3
from __future__ import annotations
import argparse,json,sys
from collections import defaultdict
from pathlib import Path
ALLOWED_GT={"application_queue_saturation","blocking_pool_pressure","executor_pressure_suspected","downstream_stage_dominates","insufficient_evidence"}

def load_manifest(p:Path):
    m=json.loads(p.read_text())
    cases=m.get('cases')
    if not isinstance(cases,list): raise ValueError('manifest.cases must be a list')
    seen=set()
    for c in cases:
        for f in ['id','artifact','artifact_type','ground_truth','acceptable_top2','tags','must_include_evidence','allowed_warnings','notes']:
            if f not in c: raise ValueError(f"case missing field: {f}")
        if c['id'] in seen: raise ValueError(f"duplicate id: {c['id']}")
        seen.add(c['id'])
        if c['ground_truth'] not in ALLOWED_GT: raise ValueError(f"unknown ground_truth: {c['ground_truth']}")
        if c['ground_truth'] not in c['acceptable_top2']: raise ValueError(f"acceptable_top2 must include ground_truth for {c['id']}")
        if c['artifact_type']!='analysis_report': raise ValueError('unsupported artifact_type')
    return cases

def collect_evidence(rep):
    out=[]
    for s in [rep.get('primary_suspect') or {},*(rep.get('secondary_suspects') or [])]:
        out.extend(s.get('evidence') or [])
    return out

def bucket(conf): return conf if conf in {'low','medium','high'} else 'unknown'

def run(manifest, root):
    total=len(manifest);top1=top2=0;hcw=0;e_pass=0;uwc=0
    failed=[];per=defaultdict(int);conf=defaultdict(lambda:{'correct':0,'total':0});cm=defaultdict(lambda:defaultdict(int))
    for c in manifest:
        rep=json.loads((root/c['artifact']).read_text())
        p=rep.get('primary_suspect') or {}
        pk=p.get('kind');pc=p.get('confidence');ps=p.get('score',0)
        secs=[s.get('kind') for s in rep.get('secondary_suspects') or []]
        if pk==c['ground_truth']: top1+=1
        if any(k in c['acceptable_top2'] for k in [pk,*secs][:2]): top2+=1
        cor=pk==c['ground_truth']
        b=bucket(pc);conf[b]['total']+=1;conf[b]['correct']+=1 if cor else 0
        if (not cor) and pc=='high': hcw+=1
        per[c['ground_truth']]+=1;cm[c['ground_truth']][pk]+=1
        ev=collect_evidence(rep)
        if all(any(req.lower() in e.lower() for e in ev) for req in c['must_include_evidence']): e_pass+=1
        else: failed.append({'id':c['id'],'reason':'required evidence missing'})
        warns=rep.get('warnings') or []
        bad=[w for w in warns if not any(ok.lower() in w.lower() for ok in c['allowed_warnings'])]
        if bad:
            uwc+=len(bad);failed.append({'id':c['id'],'reason':'unexpected warnings','warnings':bad})
    return {
        'total_cases':total,'top1_accuracy':top1/total if total else 0,'top2_recall':top2/total if total else 0,
        'high_confidence_wrong_count':hcw,'per_ground_truth':dict(per),
        'confusion_matrix':{k:dict(v) for k,v in cm.items()},
        'confidence_bucket_accuracy':{k:(v['correct']/v['total'] if v['total'] else 0) for k,v in conf.items()},
        'required_evidence_pass_rate':e_pass/total if total else 0,'unexpected_warning_count':uwc,'failed_cases':failed
    }

def main():
    ap=argparse.ArgumentParser();ap.add_argument('--manifest',required=True);ap.add_argument('--output');ap.add_argument('--min-top1',type=float,default=0.75);ap.add_argument('--min-top2',type=float,default=0.90)
    a=ap.parse_args();root=Path(__file__).resolve().parents[1]
    try: cases=load_manifest(root/Path(a.manifest))
    except Exception as e: print(f"ERROR: {e}",file=sys.stderr);return 2
    res=run(cases,root)
    print(json.dumps({k:res[k] for k in ['total_cases','top1_accuracy','top2_recall','high_confidence_wrong_count','required_evidence_pass_rate','unexpected_warning_count']},indent=2))
    if a.output:
        out=root/Path(a.output);out.parent.mkdir(parents=True,exist_ok=True);out.write_text(json.dumps(res,indent=2)+'\n')
    if res['failed_cases'] or res['top1_accuracy']<a.min_top1 or res['top2_recall']<a.min_top2 or res['unexpected_warning_count']>0:
        return 1
    return 0
if __name__=='__main__': raise SystemExit(main())
