#!/usr/bin/env python3
"""Run the diagnostic corpus with separate execution, accuracy, and Report contracts."""
import argparse
import json
import subprocess
import tempfile
from collections import Counter, defaultdict
from pathlib import Path

KINDS={"application_queue_saturation","blocking_pool_pressure","executor_pressure_suspected","downstream_stage_dominates","insufficient_evidence"}
TYPES={"run_artifact","tracing_span_jsonl","analysis_report","synthetic_analysis_report"}
TYPE_CLASS={"run_artifact":"analyzer_execution","tracing_span_jsonl":"analyzer_execution","analysis_report":"report_contract","synthetic_analysis_report":"report_contract"}
COMMON=("id","artifact","artifact_type","validation_class","accuracy_eligible","tags","notes","expected_primary_kinds","required_visible_suspects","must_include_evidence","must_include_next_checks","expected_warnings","allowed_warnings")
OLD={"required_top2","acceptable_primary","top1_required"}
CONF_ORDER={"low":0,"medium":1,"high":2}

def load_json(path):
    with Path(path).open(encoding="utf-8") as f:return json.load(f)

def _strings(value,name,cid,nonempty=False):
    if not isinstance(value,list) or any(not isinstance(x,str) or not x for x in value) or (nonempty and not value):
        raise ValueError(f"{name} must be a {'non-empty ' if nonempty else ''}list of non-empty strings for {cid}")

def validate_manifest(manifest):
    if not isinstance(manifest,dict) or not isinstance(manifest.get("cases"),list):raise ValueError("manifest must be an object containing a cases list")
    if manifest.get("schema_version") != 2:raise ValueError("manifest schema_version must be 2")
    seen=set(); labels={}
    for c in manifest["cases"]:
        cid=c.get("id","<unknown>")
        for key in COMMON:
            if key not in c:raise ValueError(f"case missing required field: {key}")
        stale=OLD & c.keys()
        if stale:raise ValueError(f"version-1 field is not allowed for {cid}: {sorted(stale)[0]}")
        if not isinstance(cid,str) or not cid.strip():raise ValueError("case id must be a non-empty string")
        if cid in seen:raise ValueError(f"duplicate case id: {cid}")
        seen.add(cid)
        typ=c["artifact_type"]; cls=c["validation_class"]
        if typ not in TYPES:raise ValueError(f"unknown artifact_type for {cid}: {typ}")
        if cls != TYPE_CLASS[typ]:raise ValueError(f"artifact_type {typ} requires validation_class {TYPE_CLASS[typ]} for {cid}")
        if not isinstance(c["accuracy_eligible"],bool):raise ValueError(f"accuracy_eligible must be a bool for {cid}")
        for key in ("expected_primary_kinds","required_visible_suspects"):
            _strings(c[key],key,cid,True)
            if any(x not in KINDS for x in c[key]):raise ValueError(f"{key} contains unknown diagnosis kind for {cid}")
        for key in ("tags","must_include_evidence","must_include_next_checks","expected_warnings","allowed_warnings"):_strings(c[key],key,cid)
        if "*" in c["expected_warnings"]+c["allowed_warnings"]:raise ValueError(f"wildcard '*' is not allowed in warnings lists for {cid}")
        if not isinstance(c["artifact"],str) or not c["artifact"] or not isinstance(c["notes"],str) or not c["notes"]:raise ValueError(f"artifact and notes must be non-empty strings for {cid}")
        if cls=="report_contract":
            if c["accuracy_eligible"]:raise ValueError(f"report_contract must be accuracy ineligible for {cid}")
            for key in ("ground_truth","observation_id","execution_expectation","artifact_policy"):
                if key in c:raise ValueError(f"{key} is not allowed on report_contract for {cid}")
        else:
            expectation=c.get("execution_expectation","success")
            if expectation not in {"success","failure"}:raise ValueError(f"unknown execution_expectation for {cid}")
            policy=c.get("artifact_policy","strict")
            if policy not in {"strict","allow_ambiguous"}:raise ValueError(f"unknown artifact_policy for {cid}")
            if "artifact_policy" in c and typ!="run_artifact":raise ValueError(f"artifact_policy is allowed only on run_artifact for {cid}")
            if "command" in c or "args" in c:raise ValueError(f"arbitrary command arguments are not allowed for {cid}")
            if expectation=="failure":
                if c["accuracy_eligible"]:raise ValueError(f"failure case must be accuracy ineligible for {cid}")
                for key in ("failure_stage","expected_error_substrings","forbidden_error_substrings","stdout_expectation"):
                    if key not in c:raise ValueError(f"failure case missing {key} for {cid}")
                if c["failure_stage"] not in ({"analyze"} if typ=="run_artifact" else {"import","analyze"}):raise ValueError(f"invalid failure_stage for {cid}")
                _strings(c["expected_error_substrings"],"expected_error_substrings",cid);_strings(c["forbidden_error_substrings"],"forbidden_error_substrings",cid)
                if c["stdout_expectation"] not in {"empty","non_empty","ignore"}:raise ValueError(f"invalid stdout_expectation for {cid}")
            if c["accuracy_eligible"]:
                for key in ("observation_id","ground_truth"):
                    if not isinstance(c.get(key),str) or not c[key].strip():raise ValueError(f"accuracy eligible case requires non-empty {key} for {cid}")
                if c["ground_truth"] not in KINDS:raise ValueError(f"unknown ground_truth for {cid}")
                if "exact_primary_kind" in c and c["exact_primary_kind"]!=c["ground_truth"]:raise ValueError(f"exact_primary_kind must equal ground_truth for {cid}")
                label=(c["ground_truth"],tuple(c["expected_primary_kinds"]),tuple(c["required_visible_suspects"]),c.get("exact_primary_kind"))
                oid=c["observation_id"]
                if oid in labels and labels[oid]!=label:raise ValueError(f"observation labels disagree for {oid}")
                labels[oid]=label
            else:
                for key in ("ground_truth","observation_id"):
                    if key in c:raise ValueError(f"{key} is allowed only on accuracy eligible cases for {cid}")

def _command(stage,case,input_path,output_path=None):
    base=["cargo","run","--quiet","-p","tailtriage-cli","--"]
    if stage=="import":return base+["import","tracing-spans-jsonl",str(input_path),"--service","validation-tracing","--output",str(output_path)]
    cmd=base+["analyze",str(input_path),"--format","json"]
    if case.get("artifact_policy","strict")=="allow_ambiguous":cmd.append("--allow-ambiguous-artifact")
    return cmd

def _invoke(command):return subprocess.run(command,capture_output=True,text=True)

def _execute(case,path):
    stages=[]
    with tempfile.TemporaryDirectory(prefix=f"tailtriage-{case['id']}-") as td:
        analyze_path=path
        if case["artifact_type"]=="tracing_span_jsonl":
            analyze_path=Path(td)/"imported-run.json"; r=_invoke(_command("import",case,path,analyze_path));stages.append(("import",r))
            if r.returncode!=0:return None,stages
            if not analyze_path.exists():
                r.returncode=1;r.stderr += "\nimport did not create run artifact";return None,stages
        r=_invoke(_command("analyze",case,analyze_path));stages.append(("analyze",r))
        if r.returncode:return None,stages
        try:return json.loads(r.stdout),stages
        except json.JSONDecodeError:return None,stages

def extract(report):
    if not isinstance(report,dict) or not isinstance(report.get("primary_suspect"),dict) or not isinstance(report.get("secondary_suspects"),list) or not isinstance(report.get("warnings"),list):raise ValueError("invalid Report shape")
    p=report["primary_suspect"]; kind=p.get("kind");conf=p.get("confidence")
    if kind not in KINDS or conf not in CONF_ORDER:raise ValueError("invalid primary suspect")
    suspects=[p]+[x for x in report["secondary_suspects"] if isinstance(x,dict)]
    def flatten(field):return [v for s in suspects for v in s.get(field,[]) if isinstance(v,str)]
    return {"top1":kind,"top2":[s.get("kind") for s in suspects[:2] if s.get("kind")],"primary_confidence":conf,"evidence":flatten("evidence"),"next_checks":flatten("next_checks"),"confidence_notes":flatten("confidence_notes"),"warnings":report["warnings"],"evidence_quality":report.get("evidence_quality",{}),"route_breakdowns":report.get("route_breakdowns",[]),"temporal_segments":report.get("temporal_segments",[])}

def _contains(required,actual):return all(any(r.lower() in a.lower() for a in actual) for r in required)
def _assert_case(case,ext):
    visible=ext["top2"]; unexpected=[w for w in ext["warnings"] if not any(x.lower() in w.lower() for x in case["expected_warnings"]+case["allowed_warnings"])]
    missing=[x for x in case["expected_warnings"] if not any(x.lower() in w.lower() for w in ext["warnings"])]
    checks={"primary_ok":ext["top1"] in case["expected_primary_kinds"],"visible_suspects_ok":all(x in visible for x in case["required_visible_suspects"]),"exact_primary_ok":case.get("exact_primary_kind",ext["top1"])==ext["top1"],"evidence_ok":_contains(case["must_include_evidence"],ext["evidence"]),"next_check_ok":_contains(case["must_include_next_checks"],ext["next_checks"]),"warnings_ok":not unexpected and not missing}
    if "max_primary_confidence" in case:checks["confidence_ceiling_ok"]=CONF_ORDER[ext["primary_confidence"]]<=CONF_ORDER[case["max_primary_confidence"]]
    if "expected_evidence_quality" in case:checks["evidence_quality_ok"]=ext["evidence_quality"].get("quality")==case["expected_evidence_quality"]
    if "expected_signal_statuses" in case:checks["signal_status_ok"]=all(ext["evidence_quality"].get(k)==v for k,v in case["expected_signal_statuses"].items())
    if "must_include_confidence_notes" in case:checks["confidence_notes_ok"]=_contains(case["must_include_confidence_notes"],ext["confidence_notes"])
    if "expected_top_level_warnings" in case:checks["top_level_warnings_ok"]=_contains(case["expected_top_level_warnings"],ext["warnings"])
    for prefix,field,shape_key,warning_key in (("route","route_breakdowns","expected_route_breakdowns","must_include_route_warning"),("temporal","temporal_segments","expected_temporal_segments","must_include_temporal_warning")):
        items=ext[field] if isinstance(ext[field],list) else []
        if shape_key in case:checks[prefix+"_shape_ok"]=(bool(items)==(case[shape_key]=="non_empty"))
        if warning_key in case:
            nested=[w for x in items if isinstance(x,dict) for w in x.get("warnings",[]) if isinstance(w,str)]
            checks[prefix+"_warnings_ok"]=_contains(case[warning_key],nested)
    checks["unexpected_warnings"]=unexpected;checks["missing_expected_warnings"]=missing
    return {"id":case["id"],"primary_kind":ext["top1"],"first_secondary_kind":ext["top2"][1] if len(ext["top2"])>1 else None,"primary_confidence":ext["primary_confidence"],**checks}, all(v for k,v in checks.items() if k.endswith("_ok"))

def _failure_contract(case,stages):
    if not stages:return False,"command was not invoked"
    stage,res=stages[-1]; text=res.stdout+"\n"+res.stderr
    if res.returncode==0:return False,"expected execution failure succeeded"
    errors=[]
    if stage!=case["failure_stage"]:errors.append(f"failed at {stage}, expected {case['failure_stage']}")
    errors += [f"missing diagnostic: {x}" for x in case["expected_error_substrings"] if x not in text]
    errors += [f"forbidden diagnostic: {x}" for x in case["forbidden_error_substrings"] if x in text]
    want=case["stdout_expectation"]
    if want=="empty" and res.stdout:errors.append("stdout was not empty")
    if want=="non_empty" and not res.stdout:errors.append("stdout was empty")
    return not errors,"; ".join(errors)

def run(manifest_path,min_top1=.75,min_top2=.90,max_high_confidence_wrong=0):
    path=Path(manifest_path).resolve();manifest=load_json(path);validate_manifest(manifest)
    analyzer=[];contracts=[];failed_a=[];failed_r=[];members=defaultdict(list);paths=Counter();expected_failures=unexpected_failures=successes=run_executions=tracing_executions=0
    for c in manifest["cases"]:
        paths[c["artifact_type"]]+=1; artifact=(path.parent/c["artifact"]).resolve()
        if c["validation_class"]=="report_contract":
            try: row,ok=_assert_case(c,extract(load_json(artifact)))
            except Exception as e:row={"id":c["id"],"error":str(e)};ok=False
            contracts.append(row)
            if not ok:failed_r.append(row)
            continue
        report,stages=_execute(c,artifact)
        if any(stage=="analyze" for stage,_ in stages):
            if c["artifact_type"]=="run_artifact":run_executions+=1
            else:tracing_executions+=1
        if c.get("execution_expectation","success")=="failure":
            ok,error=_failure_contract(c,stages);row={"id":c["id"],"expected_failure":True,"passed":ok,"error":error};analyzer.append(row)
            if ok:expected_failures+=1
            else:unexpected_failures+=1;failed_a.append(row)
            continue
        if report is None:
            unexpected_failures+=1;row={"id":c["id"],"error":"analyzer execution failed unexpectedly"};analyzer.append(row);failed_a.append(row);continue
        successes+=1
        try:ext=extract(report);row,ok=_assert_case(c,ext)
        except Exception as e:row={"id":c["id"],"error":str(e)};ok=False;ext=None
        analyzer.append(row)
        if not ok:failed_a.append(row)
        if c["accuracy_eligible"] and ext is not None:members[c["observation_id"]].append((c,ext))
    observations=[]
    for oid,group in members.items():
        ids=[c["id"] for c,_ in group]; signatures={(e["top1"],tuple(e["top2"]),e["primary_confidence"]) for _,e in group}
        if len(signatures)!=1:
            failed_a.append({"observation_id":oid,"member_case_ids":ids,"error":"equivalent encoding diagnosis or confidence disagreement"});continue
        c,e=group[0];observations.append({"observation_id":oid,"member_case_ids":ids,"ground_truth":c["ground_truth"],"expected_primary_kinds":c["expected_primary_kinds"],"top1":e["top1"],"top2":e["top2"],"confidence":e["primary_confidence"]})
    per=Counter();confusion=defaultdict(Counter);buckets=defaultdict(lambda:{"total":0,"correct":0});hcw=0
    for o in observations:
        gt=o["ground_truth"];correct=o["top1"]==gt;per[gt]+=1;confusion[gt][o["top1"]]+=1;b=buckets[o["confidence"]];b["total"]+=1;b["correct"]+=correct
        if o["confidence"]=="high" and o["top1"] not in o["expected_primary_kinds"]:hcw+=1
    n=len(observations);top1=sum(o["top1"]==o["ground_truth"] for o in observations)/n if n else None;top2=sum(o["ground_truth"] in o["top2"] for o in observations)/n if n else None
    accuracy={"observation_count":n,"encoding_count":sum(len(x) for x in members.values()),"top1_accuracy":top1,"top2_recall":top2,"high_confidence_wrong_count":hcw,"per_ground_truth_counts":dict(per),"confusion_matrix":{k:dict(v) for k,v in confusion.items()},"confidence_bucket_accuracy":{k:{**v,"accuracy":v["correct"]/v["total"]} for k,v in buckets.items()},"observations":observations}
    metrics={"schema_version":2,"manifest_case_count":len(manifest["cases"]),"analyzer_execution":{"case_count":len(analyzer),"success_count":successes,"expected_failure_count":expected_failures,"unexpected_failure_count":unexpected_failures,"run_artifact_count":run_executions,"tracing_jsonl_count":tracing_executions,"cases":analyzer},"analyzer_accuracy":accuracy,"report_contract":{"case_count":len(contracts),"passed_count":len(contracts)-len(failed_r),"failed_count":len(failed_r),"analysis_report_count":paths["analysis_report"],"synthetic_report_count":paths["synthetic_analysis_report"],"cases":contracts},"validated_paths":dict(paths),"failed_analyzer_cases":failed_a,"failed_report_contract_cases":failed_r}
    failures=[]
    if failed_a:failures.append("one or more analyzer-execution cases failed")
    if failed_r:failures.append("one or more report-contract cases failed")
    if not analyzer:failures.append("diagnostic corpus contains zero analyzer-executed cases")
    if not observations:failures.append("diagnostic corpus contains zero accuracy-eligible analyzer observations")
    else:
        if top1<min_top1:failures.append(f"top1_accuracy {top1:.3f} below threshold {min_top1:.3f}")
        if top2<min_top2:failures.append(f"top2_recall {top2:.3f} below threshold {min_top2:.3f}")
        if hcw>max_high_confidence_wrong:failures.append(f"high_confidence_wrong_count {hcw} exceeds max {max_high_confidence_wrong}")
    return metrics,failures

def main():
    p=argparse.ArgumentParser();p.add_argument("--manifest",required=True);p.add_argument("--output");p.add_argument("--min-top1",type=float,default=.75);p.add_argument("--min-top2",type=float,default=.90);p.add_argument("--max-high-confidence-wrong",type=int,default=0);a=p.parse_args()
    try:m,f=run(a.manifest,a.min_top1,a.min_top2,a.max_high_confidence_wrong)
    except Exception as e:print(f"ERROR: {e}");raise SystemExit(1)
    if a.output:Path(a.output).write_text(json.dumps(m,indent=2,sort_keys=True)+"\n")
    acc=m["analyzer_accuracy"];print(f"manifest_case_count={m['manifest_case_count']}");print(f"analyzer_execution_case_count={m['analyzer_execution']['case_count']}");print(f"accuracy_observation_count={acc['observation_count']}");print("top1_accuracy="+("n/a" if acc["top1_accuracy"] is None else f"{acc['top1_accuracy']:.3f}"));print("top2_recall="+("n/a" if acc["top2_recall"] is None else f"{acc['top2_recall']:.3f}"));print(f"high_confidence_wrong_count={acc['high_confidence_wrong_count']}")
    for x in f:print("FAIL:",x)
    if f:raise SystemExit(1)
if __name__=="__main__":main()
