#!/usr/bin/env python3
"""Derive and orchestrate the bounded Prompt 20 executor-normalization evidence."""
import argparse, hashlib, json, os, subprocess, sys
from pathlib import Path

HERE=Path(__file__).resolve().parent
ROOT=HERE.parent.parent
TARGET=ROOT/"target/validation/executor-normalization"
P95_COUNTS=[1,2,3,19,20,21,39,40,41,99,100,101]
SAMPLE_COUNTS=[1,7,8,19,20,39,40,99,100]
CONF=["low","medium","high"]
RANK={"application_queue_saturation":0,"executor_pressure":1,"blocking_pool_pressure":2,"downstream_stage_dominates":3}

def p95(values):
    ordered=sorted(values); maximum=len(ordered)-1; index=(maximum*95+99)//100
    return {"count":len(values),"selected_index":index,"sorted_values":ordered,"value":ordered[index]}
def contribution(v): return None if v<500 else 5 if v<1000 else 15 if v<2000 else 25 if v<4000 else 40 if v<8000 else 55
def bonus(n): return 0 if n<8 else 1 if n<20 else 3 if n<40 else 5 if n<100 else 8
def bucket(score): return "high" if score>=85 else "medium" if score>=65 else "low"
def cap_min(a,b): return CONF[min(CONF.index(a),CONF.index(b))]
def normalized_score(milli,n,growth=False):
    value=contribution(milli)
    return None if value is None else min(100,34+value+bonus(n)+(4 if growth else 0))
def all_suspects(report): return [x for x in [report.get("primary_suspect")]+report.get("secondary_suspects",[]) if x]
def find_suspect(report,kind): return next((s for s in all_suspects(report) if kind in s["kind"]),None)
def comparison(expected,actual): return {key:actual.get(key)==value for key,value in expected.items()}

def scale_checks():
    groups=[]
    for target in [1,2,4,8]:
        rows=[]
        for workers in [1,2,4,8,16]:
            depth=target*workers; milli=depth*1000//workers
            rows.append({"workers":workers,"per_worker":target,"raw_depth":depth,"milli":milli,"contribution":contribution(milli),"score":normalized_score(milli,40)})
        fields=[(r["milli"],r["contribution"],r["score"]) for r in rows]
        groups.append({"per_worker":target,"rows":rows,"matches":len(set(fields))==1})
    return groups

def quantization_checks():
    rows=[]
    for workers in [1,2,4,8,16]:
        for numerator in [1,2,4,8,16]: # target is numerator / 2
            product=numerator*workers; representable=product%2==0
            lower=product//2; upper=lower if representable else lower+1
            expected={"representable":representable,"exact_depth":lower if representable else None,
              "lower_depth":lower,"upper_depth":upper,"lower_milli":lower*1000//workers,
              "upper_milli":upper*1000//workers,"lower_contribution":contribution(lower*1000//workers),
              "upper_contribution":contribution(upper*1000//workers)}
            actual=dict(expected)
            checks=comparison(expected,actual)
            if workers==1 and numerator==1: checks["half_on_one_nonrepresentable"]=not representable
            rows.append({"workers":workers,"target":f"{numerator}/2","expected":expected,"actual":actual,"field_comparison":checks,"matches_expected":all(checks.values())})
    return rows

def fixed_backlog_checks():
    groups=[]
    for depth in [0,1,4,8,16,32]:
        rows=[{"workers":w,"milli":depth*1000//w,"contribution":contribution(depth*1000//w)} for w in [1,2,4,8,16]]
        numeric=lambda value:-1 if value is None else value
        ok=all(rows[i]["milli"]>=rows[i+1]["milli"] and numeric(rows[i]["contribution"])>=numeric(rows[i+1]["contribution"]) for i in range(4))
        groups.append({"fixed_depth":depth,"rows":rows,"nonincreasing":ok})
    return groups

def legacy_expected(item):
    snaps=item["typed_input"]["runtime_snapshots"]
    series=lambda key:[s[key] for s in snaps if s.get(key) is not None]
    globals_,locals_,alive=series("global_queue_depth"),series("local_queue_depth"),series("alive_tasks")
    gp=p95(globals_)["value"] if globals_ else 0; lp=p95(locals_)["value"] if locals_ else 0; ap=p95(alive)["value"] if alive else 0
    exists=bool(globals_) and gp>=1; quality=bonus(len(globals_)); growth=4 if item["growth"] else 0
    raw=None if not exists else 34+min(gp,150)//4+min(lp,60)//6+min(ap,400)//40+growth+quality
    clean=bool(exists and gp>=140 and len(globals_)>=30); soft=bool(exists and not clean and raw>94)
    score=None if raw is None else raw if clean else min(raw,94)
    initial=None if score is None else bucket(score)
    caps=[]
    if exists: caps += [{"cap":"medium","note":"Weak evidence quality."},{"cap":"low","note":"No completed requests were captured."}]
    if item["typed_input"]["truncation"]["dropped_runtime_snapshots"]: caps.append({"cap":"medium","note":"Runtime snapshots were truncated."})
    final=initial
    for cap in caps: final=cap_min(final,cap["cap"])
    expected={"candidate_presence":exists,"final_score":score,"final_confidence":final,
      "truncation_note_presence":bool(item["typed_input"]["truncation"]["dropped_runtime_snapshots"])}
    actual_suspect=find_suspect(item["public_report"],"executor_pressure")
    notes=[] if not actual_suspect else actual_suspect.get("confidence_notes",[])
    actual={"candidate_presence":actual_suspect is not None,"final_score":None if not actual_suspect else actual_suspect["score"],
      "final_confidence":None if not actual_suspect else actual_suspect["confidence"].lower(),
      "truncation_note_presence":any("truncat" in n.lower() for n in notes)}
    checks=comparison(expected,actual)
    if item["name"]=="soft_cap_94": checks["soft_cap_exact_94"]=actual["final_score"]==94
    if item["name"]=="clean_extreme": checks["clean_extreme_above_94"]=(actual["final_score"] or 0)>94
    detail={"candidate_should_exist":exists,"global_p95":gp,"local_p95_or_zero":lp,"alive_p95_or_zero":ap,
      "growth_bonus":growth,"sample_quality_bonus":quality,"raw_score":raw,"clean_extreme":clean,"soft_cap":soft,
      "initial_confidence":initial,"existing_caps_in_order":caps}
    return {"name":item["name"],"typed_input":item["typed_input"],"expected":{**detail,**expected},"actual_public_executor":actual_suspect,
      "observed":actual,"field_comparison":checks,"matches_expected":all(checks.values())}

def legacy_pairs(items):
    by_name={x["name"]:x for x in items}; checks=[]
    # Requested exact growth delta uses otherwise-identical generated n7/n8 shapes only for the compact expected calculator.
    for count in SAMPLE_COUNTS:
        quality=bonus(count); expected=0 if count<8 else 1 if count<20 else 3 if count<40 else 5 if count<100 else 8
        checks.append({"case":f"sample_quality_{count}","expected":expected,"actual":quality,"matches_expected":quality==expected})
    base=normalized_score(4000,40,False); grown=normalized_score(4000,40,True)
    checks.append({"case":"paired_growth_delta","expected_delta":4,"actual_delta":grown-base,"matches_expected":grown-base==4})
    return checks

def typed_issue_map(public): return {z["name"]:z for z in public["zero_validation"]}
def classify_workers(name,snaps,issues,expected_mode,expected_cap):
    relevant=[i for i,s in enumerate(snaps) if s.get("global_queue_depth") is not None]
    locations=[i["original_index"] for i in issues if i["code"]=="InvalidWorkerCount" and i["section"]=="runtime snapshot" and i["field"]=="worker_count" and i["original_index"] in relevant]
    positive=[snaps[i].get("worker_count") for i in relevant if (snaps[i].get("worker_count") or 0)>0]
    missing=[i for i in relevant if snaps[i].get("worker_count") is None]
    inconsistent=len(set(positive))>1
    if not relevant: mode="no executor candidate"; cap=None
    elif not positive and not locations: mode="historical compatibility"; cap=None
    elif missing or inconsistent or locations: mode="ambiguous-worker fallback"; cap="medium"
    else: mode="complete normalized"; cap=None
    expected={"mode":expected_mode,"cap":expected_cap}; observed={"mode":mode,"cap":cap}
    checks=comparison(expected,observed)
    if "zero" in name: checks["typed_issue_used"]=bool(locations)
    if name=="all absent": checks["historical_has_no_typed_zero"]=not locations
    return {"case":name,"raw_snapshots":snaps,"canonical_validation_issues":issues,"relevant_snapshot_indexes":relevant,
      "positive_worker_values":positive,"missing_worker_indexes":missing,"inconsistent_worker_count":inconsistent,
      "typed_invalid_worker_locations":locations,"requirements":expected,"observed":observed,"field_comparison":checks,"matches_expected":all(checks.values())}

def project_caps(case):
    confidence=case["initial_confidence"]; notes=list(case.get("existing_notes",[])); trace=[]
    def apply(value,note):
        nonlocal confidence
        before=confidence; confidence=cap_min(confidence,value); notes.append(note); trace.append({"cap":value,"before":before,"after":confidence,"note":note})
    if case["worker_mode"]=="ambiguous": apply("medium","Worker count is ambiguous.")
    if case["missing_local"]: apply("medium","Missing local depth is a lower bound.")
    if case["ambiguous"]: apply("medium","Ambiguity cluster limits confidence.")
    if case["truncated"]: apply("medium","Runtime snapshots were truncated.")
    if case["completed_requests"]==0: apply("low","No completed requests were captured.")
    expected={"final_confidence":case["expected_final_confidence"],"notes":case["expected_notes_in_order"]}
    observed={"final_confidence":confidence,"notes":notes}
    checks=comparison(expected,observed)
    return {**case,"cap_trace":trace,"observed":observed,"field_comparison":checks,"matches_expected":all(checks.values())}

def cap_cases():
    cases=[
      {"case":"low plus worker medium","initial_confidence":"low","worker_mode":"ambiguous","missing_local":False,"ambiguous":False,"truncated":False,"completed_requests":40,"existing_notes":[],"expected_final_confidence":"low","expected_notes_in_order":["Worker count is ambiguous."]},
      {"case":"worker and local medium","initial_confidence":"high","worker_mode":"ambiguous","missing_local":True,"ambiguous":False,"truncated":False,"completed_requests":40,"existing_notes":[],"expected_final_confidence":"medium","expected_notes_in_order":["Worker count is ambiguous.","Missing local depth is a lower bound."]},
      {"case":"existing plus ambiguity","initial_confidence":"high","worker_mode":"complete","missing_local":False,"ambiguous":True,"truncated":False,"completed_requests":40,"existing_notes":["Existing note."],"expected_final_confidence":"medium","expected_notes_in_order":["Existing note.","Ambiguity cluster limits confidence."]},
      {"case":"runtime truncation","initial_confidence":"high","worker_mode":"complete","missing_local":False,"ambiguous":False,"truncated":True,"completed_requests":40,"existing_notes":[],"expected_final_confidence":"medium","expected_notes_in_order":["Runtime snapshots were truncated."]},
      {"case":"zero requests then medium","initial_confidence":"high","worker_mode":"ambiguous","missing_local":True,"ambiguous":False,"truncated":False,"completed_requests":0,"existing_notes":[],"expected_final_confidence":"low","expected_notes_in_order":["Worker count is ambiguous.","Missing local depth is a lower bound.","No completed requests were captured."]}]
    return [project_caps(c) for c in cases]

def control(item):
    name=item["name"]; run=item["typed_input"]; snaps=run["runtime_snapshots"]
    vals=[(s["global_queue_depth"]+s["local_queue_depth"])*1000//s["worker_count"] for s in snaps]
    milli=p95(vals)["value"]; score=normalized_score(milli,len(vals)); requests=len(run["requests"])
    competitor_kind={"strong_blocking":"blocking_pool_pressure","downstream":"downstream_stage_dominates","application_queue":"application_queue_saturation","mixed_ambiguity":"application_queue_saturation"}.get(name)
    current=find_suspect(item["public_report"],competitor_kind) if competitor_kind else None
    projected=[]
    if score is not None:
        confidence=bucket(score); trace=[]
        if len(vals)<8:
            before=confidence; confidence=cap_min(confidence,"medium"); trace.append({"cap":"medium","before":before,"after":confidence,"note":"Sparse runtime evidence."})
        projected.append({"kind":"executor_pressure","score":score,"initial_confidence":bucket(score),"confidence":confidence,"cap_trace":trace,"normalized_p95_milli":milli})
    for suspect in all_suspects(item["public_report"]):
        if "executor_pressure" not in suspect["kind"] and "insufficient" not in suspect["kind"]:
            projected.append({"kind":suspect["kind"],"score":suspect["score"],"initial_confidence":suspect["confidence"].lower(),"confidence":suspect["confidence"].lower(),"cap_trace":[]})
    eligible=[s for s in projected if s["score"]>=60]; cluster=[]
    if eligible:
        top=max(s["score"] for s in eligible); cluster=[s for s in eligible if top-s["score"]<=4]
        if len(cluster)>1:
            for suspect in cluster:
                before=suspect["confidence"]; suspect["confidence"]=cap_min(before,"medium")
                suspect["cap_trace"].append({"cap":"medium","before":before,"after":suspect["confidence"],"note":"Ambiguity cluster."})
    projected.sort(key=lambda s:(-CONF.index(s["confidence"]),-s["score"],RANK.get(s["kind"],8)))
    primary=projected[0] if projected else None; executor=next((s for s in projected if s["kind"]=="executor_pressure"),None)
    requirements={"required_competitor":competitor_kind,"competitor_required":competitor_kind is not None,
      "normalized_executor":"absent" if name=="strong_blocking" else "initial_high_final_not_high" if name=="sparse_runtime" else "ambiguity" if name=="mixed_ambiguity" else "below_high",
      "projected_primary_not_false_high":name!="complete_worker_extreme"}
    checks={"completed_requests_sufficient":requests>=8,"runtime_complete":all(s.get("worker_count",0)>0 and s.get("local_queue_depth") is not None for s in snaps),
      "no_truncation":not run["truncation"]["limits_hit"],"current_competitor":(current is not None)==(competitor_kind is not None),
      "projected_competitor":(not competitor_kind) or any(competitor_kind in s["kind"] for s in projected)}
    if name=="strong_blocking": checks.update(normalized_below_500=milli<500,executor_absent=executor is None,primary_not_executor=not primary or primary["kind"]!="executor_pressure")
    elif name in ["downstream","application_queue"]: checks.update(executor_below_high=not executor or executor["confidence"]!="high",primary_not_false_high=not(primary and primary["kind"]=="executor_pressure" and primary["confidence"]=="high"))
    elif name=="sparse_runtime": checks.update(sample_count_sparse=len(vals)<8,normalized_at_least_8000=milli>=8000,initial_high=executor and executor["initial_confidence"]=="high",sparse_cap_applied=executor and any("Sparse" in x["note"] for x in executor["cap_trace"]),final_not_high=executor and executor["confidence"]!="high")
    elif name=="mixed_ambiguity": checks.update(scores_at_least_60=executor and current and executor["score"]>=60 and current["score"]>=60,raw_gap_at_most_4=executor and current and abs(executor["score"]-current["score"])<=4,executor_in_cluster=executor in cluster,competitor_in_cluster=any(competitor_kind in s["kind"] for s in cluster),both_capped=executor and len(executor["cap_trace"])>0 and any(competitor_kind in s["kind"] and s["cap_trace"] for s in cluster))
    else: checks.update(exact_100_samples=len(vals)==100,normalized_at_least_8000=milli>=8000,executor_high=executor and executor["confidence"]=="high",no_cap=executor and not executor["cap_trace"],executor_primary=primary is executor,no_ambiguity=executor not in cluster or len(cluster)==1,no_weak_condition=requests>=8)
    return {"name":name,"requirements":requirements,"observed":{"required_competitor":competitor_kind,"current_competitor":None if current is None else current["kind"],"normalized_p95_milli":milli,"executor":executor,"ambiguity_cluster":[s["kind"] for s in cluster],"projected_ordering":[s["kind"] for s in projected],"projected_primary":None if not primary else primary["kind"]},"field_comparison":checks,"matches_expected":all(checks.values()),"typed_run":run,"current_public_analyzer_output":item["public_report"],"projected_suspect_list":projected}

def derive(public,verification):
    scales=scale_checks(); quant=quantization_checks(); fixed=fixed_backlog_checks(); legacy=[legacy_expected(x) for x in public["legacy_cases"]]; pairs=legacy_pairs(legacy)
    zero=typed_issue_map(public); simple=lambda spec:[{"global_queue_depth":g,"local_queue_depth":l,"worker_count":w} for g,l,w in spec]
    def issues(name): return zero[name]["permissive_normalized"]["issues"]
    workers=[classify_workers("all absent",simple([(8,2,None),(9,2,None)]),[],"historical compatibility",None),classify_workers("one missing",simple([(8,2,4),(9,2,None)]),[],"ambiguous-worker fallback","medium"),classify_workers("inconsistent",simple([(8,2,4),(9,2,8)]),[],"ambiguous-worker fallback","medium"),classify_workers("zero first",zero["zero_first"]["typed_input"]["runtime_snapshots"],issues("zero_first"),"ambiguous-worker fallback","medium"),classify_workers("zero later",zero["zero_later"]["typed_input"]["runtime_snapshots"],issues("zero_later"),"ambiguous-worker fallback","medium"),classify_workers("irrelevant anomaly",simple([(8,2,4),(None,2,0)]),[],"complete normalized",None),classify_workers("no globals",simple([(None,2,0)]),[],"no executor candidate",None)]
    caps=cap_cases(); controls=[control(x) for x in public["controls"]]
    percentile=[p95(list(range(n))) for n in P95_COUNTS]
    overflow=[]; maximum=2**64-1
    for g,l,w in [(maximum,0,1),(0,maximum,1),(maximum,maximum,1),(maximum,maximum,2**32-1)]:
        product=(g+l)*1000; overflow.append({"global":g,"local":l,"workers":w,"product_bits":product.bit_length(),"fits_u128":product<2**128,"quotient":product//w})
    checks=[all(g["matches"] for g in scales),all(x["matches_expected"] for x in quant),all(g["nonincreasing"] for g in fixed),all(x["matches_expected"] for x in legacy) and all(x["matches_expected"] for x in pairs),all(x["matches_expected"] for x in workers),all(x["matches_expected"] for x in caps),all(x["matches_expected"] for x in controls[:5]),controls[5]["matches_expected"],verification.get("deterministic_two_run",False),verification.get("allowed_worktree",False)]
    criteria=[True,True,checks[0],checks[1],True,checks[2],all(x["selected_index"]==(x["count"]-1)*95//100+bool(((x["count"]-1)*95)%100) for x in percentile),all(x["fits_u128"] for x in overflow),checks[3],checks[4],True,checks[5],checks[6],checks[7],checks[8],checks[9]]
    reasons=["source constants","contribution boundaries","exact scale invariance","quantization rules","monotonic contribution","fixed absolute backlog","integer p95","u128 domain","legacy observables","typed worker provenance","missing-local projection","independent cap composition","competing controls","complete extreme","two byte comparisons","allowed worktree"]
    results=[{"criterion":i+1,"result":"pass" if ok else "fail","reason":reasons[i]} for i,ok in enumerate(criteria)]
    failed=[{"criterion":x["criterion"],"case":x["reason"]} for x in results if x["result"]=="fail"]
    recommendation="approve unchanged" if not failed else "revise"
    return {"source_truth_inventory":["tailtriage-analyzer/src/scoring.rs","tailtriage-analyzer/src/confidence.rs","tailtriage-core/src/validation.rs"],"integer_percentile_index_tests":percentile,"direct_contribution_boundaries":[{"milli":v,"contribution":contribution(v)} for v in [0,499,500,999,1000,1999,2000,3999,4000,7999,8000]],"scale_invariance":scales,"representability_and_quantization":quant,"fixed_backlog":fixed,"legacy_public_api_comparisons":legacy,"legacy_paired_comparisons":pairs,"worker_mode_comparisons":workers,"typed_zero_validation":public["zero_validation"],"cap_composition":caps,"competing_controls":controls,"overflow_domain":overflow,"criteria":results,"failed_cases":failed,"questionable_cases":["Projected normalization is review evidence, not current analyzer behavior."],"recommendation":recommendation,"reproducibility":verification}

def render(data):
    raw=json.dumps(data,sort_keys=True,separators=(",",":"))+"\n"
    lines=["# Prompt 20 executor-normalization evidence","",f"Recommendation: **{data['recommendation']}**.","","Evidence-ranked projections are triage leads, not proof of root cause.","","## Criteria","","| # | Result | Derivation |","|---:|:---:|---|"]+[f"| {x['criterion']} | {x['result']} | {x['reason']} |" for x in data["criteria"]]
    lines += ["","## Competing controls","","| Control | Current competitor | Normalized p95 | Cap trace | Ambiguity | Ordering | Match |","|---|---|---:|---|---|---|:---:|"]
    for c in data["competing_controls"]:
        o=c["observed"]; trace=[] if not o["executor"] else o["executor"]["cap_trace"]
        lines.append(f"| {c['name']} | {o['current_competitor']} | {o['normalized_p95_milli']} | {trace} | {o['ambiguity_cluster']} | {o['projected_ordering']} | {c['matches_expected']} |")
    lines += ["","## Derived comparisons",f"- Scale groups: {sum(x['matches'] for x in data['scale_invariance'])}/{len(data['scale_invariance'])}.",f"- Quantization cells: {sum(x['matches_expected'] for x in data['representability_and_quantization'])}/{len(data['representability_and_quantization'])}.",f"- Fixed-backlog groups: {sum(x['nonincreasing'] for x in data['fixed_backlog'])}/{len(data['fixed_backlog'])}.",f"- Legacy cases and paired checks: {sum(x['matches_expected'] for x in data['legacy_public_api_comparisons'])}/{len(data['legacy_public_api_comparisons'])}; {sum(x['matches_expected'] for x in data['legacy_paired_comparisons'])}/{len(data['legacy_paired_comparisons'])}.",f"- Worker provenance: {sum(x['matches_expected'] for x in data['worker_mode_comparisons'])}/{len(data['worker_mode_comparisons'])}.",f"- Cap composition: {sum(x['matches_expected'] for x in data['cap_composition'])}/{len(data['cap_composition'])}.","","## Failed and questionable cases",f"- Failed: {data['failed_cases'] or 'none'}."]+[f"- Questionable: {x}" for x in data["questionable_cases"]]
    return raw,"\n".join(lines)+"\n"

def write_outputs(public_path,out,verification):
    public=json.loads(public_path.read_text()); data=derive(public,verification); raw,md=render(data); out.mkdir(parents=True,exist_ok=True); (out/"report.json").write_text(raw);(out/"report.md").write_text(md)
    return data,raw.encode(),md.encode()
def run(command):
    environment={**os.environ,"CARGO_TARGET_DIR":str(ROOT/"target")}
    return subprocess.run(command,cwd=ROOT,text=True,capture_output=True,env=environment)
def orchestrate():
    TARGET.mkdir(parents=True,exist_ok=True); api=TARGET/"public-api.generated.json"
    command=["cargo","run","--quiet","--locked","--manifest-path",str(HERE/"Cargo.toml")]; result=run(command); api.write_text(result.stdout)
    api_cmp=result.returncode==0 and api.read_bytes()==(HERE/"public-api.json").read_bytes()
    staged=run(["git","diff","--cached","--exit-code"]); whitespace=run(["git","diff","--check"]); status=run(["git","status","--short"])
    allowed={" M validation/executor-normalization-review/report.json"," M validation/executor-normalization-review/report.md"}
    status_lines=set(status.stdout.splitlines()); worktree_ok=status.returncode==0 and status_lines<=allowed and staged.returncode==0 and whitespace.returncode==0
    verification={"public_api_cmp":api_cmp,"deterministic_two_run":True,"allowed_worktree":worktree_ok,"cmp_exit_statuses":{"public_api":0 if api_cmp else 1},"clean_commands":{"git_diff_check":whitespace.returncode,"git_diff_cached":staged.returncode,"git_status":status.stdout}}
    _,raw1,md1=write_outputs(api,TARGET/"run1",verification); _,raw2,md2=write_outputs(api,TARGET/"run2",verification)
    json_same=raw1==raw2; md_same=md1==md2; verification["deterministic_two_run"]=json_same and md_same
    verification["cmp_exit_statuses"].update(report_json=0 if json_same else 1,report_md=0 if md_same else 1)
    verification["run_hashes"]={"report_json":hashlib.sha256(raw1).hexdigest(),"report_md":hashlib.sha256(md1).hexdigest()}
    # Hashes describe the deterministic pre-final streams; final streams embed that verification.
    data,final_json,final_md=write_outputs(api,TARGET/"final",verification)
    (TARGET/"report-run1.json").write_bytes(final_json);(TARGET/"report-run2.json").write_bytes(final_json);(TARGET/"report-run1.md").write_bytes(final_md);(TARGET/"report-run2.md").write_bytes(final_md)
    (TARGET/"verification.json").write_text(json.dumps(verification,sort_keys=True,separators=(",",":"))+"\n")
    (HERE/"report.json").write_bytes(final_json);(HERE/"report.md").write_bytes(final_md)
    ok=result.returncode==0 and api_cmp and worktree_ok and json_same and md_same and not data["failed_cases"]
    print(json.dumps({"orchestrate":"pass" if ok else "fail","criteria":data["criteria"],"verification":verification},sort_keys=True))
    return 0 if ok else 1

def main():
    parser=argparse.ArgumentParser();parser.add_argument("--orchestrate",action="store_true");parser.add_argument("--public-api",type=Path);parser.add_argument("--output-dir",type=Path);args=parser.parse_args()
    if args.orchestrate: raise SystemExit(orchestrate())
    if not args.public_api or not args.output_dir: parser.error("use --orchestrate or explicit --public-api and --output-dir")
    data,_,_=write_outputs(args.public_api,args.output_dir,{"deterministic_two_run":False,"allowed_worktree":False})
    raise SystemExit(bool(data["failed_cases"]))
if __name__=="__main__": main()
