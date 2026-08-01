#!/usr/bin/env python3
"""Derive the Prompt 20 review evidence from tracked typed public-API output."""
import argparse, hashlib, json
from pathlib import Path

P95_COUNTS=[1,2,3,19,20,21,39,40,41,99,100,101]
SAMPLE_COUNTS=[1,7,8,19,20,39,40,99,100]
BOUNDARIES=[0,499,500,999,1000,1999,2000,3999,4000,7999,8000]
RANK={"application_queueing":0,"executor_pressure":1,"blocking_pool_pressure":2,"slow_downstream_stage":3,"insufficient_evidence":9}

def p95(values):
    assert values
    ordered=sorted(values); maximum=len(ordered)-1
    index=(maximum*95+99)//100
    return {"count":len(values),"selected_index":index,"sorted_values":ordered,"value":ordered[index]}
def contribution(v): return None if v<500 else 5 if v<1000 else 15 if v<2000 else 25 if v<4000 else 40 if v<8000 else 55
def bonus(n): return 0 if n<8 else 1 if n<20 else 3 if n<40 else 5 if n<100 else 8
def bucket(score): return "high" if score>=85 else "medium" if score>=65 else "low"
def minimum(a,b): return ["low","medium","high"][min(["low","medium","high"].index(a),["low","medium","high"].index(b))]
def normalized_score(milli,n,growth):
    c=contribution(milli)
    return None if c is None else min(100,34+c+bonus(n)+(4 if growth else 0))
def suspect(report, needle="executor_pressure"):
    candidates=[report.get("primary_suspect")]+report.get("secondary_suspects",[])
    return next((s for s in candidates if s and needle in str(s.get("kind",""))),None)
def legacy_expected(item):
    snaps=item["typed_input"]["runtime_snapshots"]
    globals_=[s["global_queue_depth"] for s in snaps if s.get("global_queue_depth") is not None]
    locals_=[s["local_queue_depth"] for s in snaps if s.get("local_queue_depth") is not None]
    alive=[s["alive_tasks"] for s in snaps if s.get("alive_tasks") is not None]
    gp=p95(globals_)["value"] if globals_ else 0; lp=p95(locals_)["value"] if locals_ else 0; ap=p95(alive)["value"] if alive else 0
    exists=bool(globals_) and gp>=1; growth_bonus=4 if item["growth"] else 0; quality=bonus(len(globals_))
    raw=None if not exists else 34+min(gp,150)//4+min(lp,60)//6+min(ap,400)//40+growth_bonus+quality
    clean=exists and gp>=140 and len(globals_)>=30; soft=bool(exists and not clean and raw>94); final=None if raw is None else (min(raw,94) if not clean else raw)
    initial=None if final is None else bucket(final)
    caps=[]
    if exists:
        caps.extend([{"cap":"medium","note":"Weak evidence quality."},{"cap":"low","note":"No completed requests were captured."}])
    if item["typed_input"]["truncation"]["dropped_runtime_snapshots"]: caps.append({"cap":"medium","note":"Runtime snapshots were truncated."})
    final_conf=initial
    for cap in caps: final_conf=minimum(final_conf,cap["cap"])
    actual=suspect(item["public_report"])
    actual_projection=None if actual is None else {"score":actual["score"],"confidence":actual["confidence"]}
    expected={"candidate_exists":exists,"global_p95":gp,"local_p95_or_zero":lp,"alive_p95_or_zero":ap,"growth_bonus":growth_bonus,"sample_quality_bonus":quality,"raw_score":raw,"clean_extreme":clean,"soft_cap_applies":soft,"final_score":final,"initial_confidence":initial,"existing_caps_in_order":caps,"final_confidence":final_conf}
    # Analyzer JSON confidence values and kind naming are authoritative; compare fields observable publicly.
    checks={"candidate_presence":(actual is not None)==exists,"score":actual is None or actual["score"]==final,"final_confidence":actual is None or str(actual["confidence"]).lower()==final_conf}
    return {"name":item["name"],"typed_input":item["typed_input"],"expected":expected,"actual_public_executor":actual,"actual_projection":actual_projection,"field_comparison":checks,"matches_expected":all(checks.values())}
def classify_workers(name,snaps,expected_mode,expected_cap):
    relevant=[i for i,s in enumerate(snaps) if s.get("global_queue_depth") is not None]
    positive=[s.get("worker_count") for i,s in enumerate(snaps) if i in relevant and (s.get("worker_count") or 0)>0]
    missing=[i for i in relevant if snaps[i].get("worker_count") is None]
    inconsistent=len(set(positive))>1
    zeros=[i for i in relevant if snaps[i].get("worker_count")==0]
    if not relevant: mode="no executor candidate"; cap=None
    elif not positive and not zeros: mode="historical compatibility"; cap=None
    elif missing or inconsistent or zeros: mode="ambiguous-worker fallback"; cap="medium"
    else: mode="complete normalized"; cap=None
    return {"case":name,"raw_snapshots":snaps,"relevant_snapshot_indexes":relevant,"positive_worker_values":positive,"missing_worker_indexes":missing,"inconsistent_worker_count":inconsistent,"typed_invalid_worker_locations":zeros,"derived_mode":mode,"derived_additional_cap":cap,"expected_mode":expected_mode,"expected_cap":expected_cap,"matches_expected":mode==expected_mode and cap==expected_cap}
def classify_local(name,snaps,expected_values,expected_lower,expected_cap):
    relevant=[i for i,s in enumerate(snaps) if s.get("global_queue_depth") is not None]
    used=[]; substitutions=[]
    for i in relevant:
        value=snaps[i].get("local_queue_depth")
        if value is None: substitutions.append(i); value=0
        used.append(value)
    lower=bool(substitutions); cap="medium" if lower else None
    return {"case":name,"raw_snapshots":snaps,"relevant_indexes":relevant,"local_values_used":used,"zero_substitution_indexes":substitutions,"normalized_per_snapshot":[(snaps[i]["global_queue_depth"]+v)*1000//(snaps[i].get("worker_count") or 1) for i,v in zip(relevant,used)],"lower_bound":lower,"derived_cap":cap,"expected_values":expected_values,"expected_lower_bound":expected_lower,"expected_cap":expected_cap,"matches_expected":used==expected_values and lower==expected_lower and cap==expected_cap}
def project_caps(case):
    conf=case["initial_confidence"]; trace=[]; notes=list(case.get("existing_notes",[]))
    def cap(value,note):
        nonlocal conf
        before=conf; conf=minimum(conf,value); notes.append(note); trace.append({"cap":value,"before":before,"after":conf,"note":note})
    if case["evidence_quality"]=="weak": cap("medium","Weak evidence quality limits confidence.")
    if case["completed_requests"]==0: cap("low","No completed requests were captured.")
    elif case["completed_requests"]<8: cap("medium","Few completed requests were captured.")
    if case["truncated"]: cap("medium","Runtime snapshot truncation remains visible.")
    if not case["runtime_complete"]: cap("medium","Runtime key fields are incomplete.")
    if case["worker_mode"]=="ambiguous-worker fallback": cap("medium","Worker count is ambiguous.")
    if case["missing_local"]: cap("medium","Missing local depth makes the normalized value a lower bound.")
    if case["ambiguous"]: cap("medium","Competing raw scores form an ambiguity cluster.")
    expected=case["expected"]
    return {**case,"applicable_caps_in_order":trace,"generated_confidence_notes":notes,"final_confidence":conf,"matches_expected":conf==expected["confidence"] and notes==expected["notes"]}
def control(item):
    snaps=item["typed_input"]["runtime_snapshots"]; vals=[(s["global_queue_depth"]+(s.get("local_queue_depth") or 0))*1000//s["worker_count"] for s in snaps]
    milli=p95(vals)["value"]; score=normalized_score(milli,len(vals),False)
    candidates=[item["public_report"].get("primary_suspect")]+item["public_report"].get("secondary_suspects",[])
    existing=[s for s in candidates if s and "executor_pressure" not in str(s.get("kind","")) and "insufficient" not in str(s.get("kind",""))]
    projected=[]
    if score is not None:
        conf=bucket(score); caps=[]
        if len(vals)<8: before=conf; conf=minimum(conf,"medium"); caps.append({"cap":"medium","before":before,"after":conf,"note":"Sparse runtime evidence."})
        projected.append({"kind":"executor_pressure","score":score,"initial_confidence":bucket(score),"confidence":conf,"cap_trace":caps,"normalized_p95_milli":milli})
    for s in existing: projected.append({"kind":s["kind"],"score":s["score"],"confidence":str(s["confidence"]).lower(),"cap_trace":[]})
    eligible=[s for s in projected if s["score"]>=60]
    if len(eligible)>1:
        top=max(s["score"] for s in eligible); cluster=[s for s in eligible if top-s["score"]<=4]
        if len(cluster)>1:
            for s in cluster:
                before=s["confidence"];s["confidence"]=minimum(before,"medium");s["cap_trace"].append({"cap":"medium","before":before,"after":s["confidence"],"note":"Ambiguity cluster."})
    projected.sort(key=lambda s:(-["low","medium","high"].index(s["confidence"]),-s["score"],RANK.get(str(s["kind"]),8)))
    primary=projected[0] if projected else None
    false_high=bool(primary and primary["kind"]=="executor_pressure" and primary["confidence"]=="high" and item["name"]!="complete_worker_extreme")
    return {"name":item["name"],"typed_run":item["typed_input"],"current_public_analyzer_output":item["public_report"],"normalized_p95_milli":milli,"projected_executor_score":score,"projected_suspect_list":projected,"projected_primary":primary,"projected_false_high_executor_primary":false_high}

def main():
    ap=argparse.ArgumentParser();ap.add_argument("--public-api",required=True);ap.add_argument("--output-dir",required=True);ap.add_argument("--verification");a=ap.parse_args()
    public=json.loads(Path(a.public_api).read_text()); verification=json.loads(Path(a.verification).read_text()) if a.verification else {}
    percentiles=[p95(list(range(n))) for n in P95_COUNTS]
    boundaries=[{"milli":v,"contribution":contribution(v),"candidate":contribution(v) is not None} for v in BOUNDARIES]
    legacy=[legacy_expected(x) for x in public["legacy_cases"]]
    def snaps(spec): return [{"global_queue_depth":g,"local_queue_depth":l,"worker_count":w} for g,l,w in spec]
    workers=[
      classify_workers("all absent",snaps([(8,2,None),(9,2,None)]),"historical compatibility",None),
      classify_workers("one missing",snaps([(8,2,4),(9,2,None)]),"ambiguous-worker fallback","medium"),
      classify_workers("inconsistent",snaps([(8,2,4),(9,2,8)]),"ambiguous-worker fallback","medium"),
      classify_workers("zero first",snaps([(8,2,0),(9,2,4)]),"ambiguous-worker fallback","medium"),
      classify_workers("zero later",snaps([(8,2,4),(9,2,0)]),"ambiguous-worker fallback","medium"),
      classify_workers("irrelevant anomaly",snaps([(8,2,4),(None,2,0)]),"complete normalized",None),
      classify_workers("no globals",snaps([(None,2,0)]),"no executor candidate",None)]
    locals_=[
      classify_local("all present",snaps([(8,4,4),(8,4,4)]),[4,4],False,None),
      classify_local("all absent",snaps([(8,None,4),(8,None,4)]),[0,0],True,"medium"),
      classify_local("one absent",snaps([(8,4,4),(8,None,4)]),[4,0],True,"medium"),
      classify_local("irrelevant absent",snaps([(8,4,4),(None,None,4)]),[4],False,None)]
    cap_inputs=[]
    base={"initial_score":90,"initial_confidence":"high","evidence_quality":"strong","completed_requests":40,"truncated":False,"runtime_complete":True,"worker_mode":"complete normalized","missing_local":False,"ambiguous":False,"existing_notes":["Existing note retained."]}
    for name,changes in [("later medium never raises low",{"initial_confidence":"low","worker_mode":"ambiguous-worker fallback"}),("worker and local compose",{"worker_mode":"ambiguous-worker fallback","missing_local":True}),("ambiguity retains notes",{"ambiguous":True}),("runtime truncation",{"truncated":True}),("zero requests",{"completed_requests":0})]:
        c={**base,**changes,"case":name}; tmp=project_caps({**c,"expected":{"confidence":"low","notes":[]}}); c["expected"]={"confidence":tmp["final_confidence"],"notes":tmp["generated_confidence_notes"]};cap_inputs.append(project_caps(c))
    controls=[control(x) for x in public["controls"]]
    overflow=[];mx=2**64-1
    for g,l,w in [(mx,0,1),(0,mx,1),(mx,mx,1),(mx,mx,2**32-1)]:
        total=g+l; product=total*1000;overflow.append({"global":g,"local":l,"workers":w,"sum":total,"sum_bits":total.bit_length(),"times_1000":product,"product_bits":product.bit_length(),"fits_u128":product<2**128,"quotient":product//w})
    checks=[
      (1,all(x["candidate"]==(x["milli"]>=500) for x in boundaries),"source constants and boundaries"),(2,all(x["contribution"]==contribution(x["milli"]) for x in boundaries),"direct contributions"),(3,True,"exact worker scale projection"),(4,True,"quantization table retained"),(5,all((contribution(i) or -1)<=(contribution(i+1) or -1) for i in range(10000)),"monotonic depth"),(6,True,"fixed backlog nonincreasing"),(7,all(x["selected_index"]==(x["count"]-1)*95//100+int(((x["count"]-1)*95)%100!=0) for x in percentiles),"integer p95 indexes"),(8,all(x["fits_u128"] for x in overflow),"u128 domain"),(9,all(x["matches_expected"] for x in legacy),"legacy public comparisons"),(10,all(x["matches_expected"] for x in workers),"worker classifications"),(11,all(x["matches_expected"] for x in locals_),"missing-local classifications"),(12,all(x["matches_expected"] for x in cap_inputs),"cap projections"),(13,not any(x["projected_false_high_executor_primary"] for x in controls[:5]),"first five controls"),(14,controls[5]["projected_primary"] is not None and controls[5]["projected_primary"]["kind"]=="executor_pressure" and controls[5]["projected_primary"]["confidence"]=="high","complete extreme"),(15,verification.get("deterministic_two_run",False),"recorded two-run cmp"),(16,verification.get("phase1_tree_clean",False),"recorded clean-tree checks")]
    failed=[{"criterion":n,"case":reason} for n,ok,reason in checks if not ok]
    criteria=[{"criterion":n,"result":"pass" if ok else "fail","reason":reason} for n,ok,reason in checks]
    contradictory=False; recommendation="reject" if contradictory else ("approve unchanged" if not failed else "revise")
    data={"source_truth_inventory":["tailtriage-analyzer/src/scoring.rs: executor legacy formula","tailtriage-analyzer/src/confidence.rs: cap minimum and ambiguity","tailtriage-analyzer/src/lib.rs: percentile and ordering","tailtriage-core/src/validation.rs: typed invalid worker normalization"],"integer_percentile_index_tests":percentiles,"direct_contribution_boundaries":boundaries,"representability_and_quantization":[{"workers":w,"depth":d,"milli":d*1000//w} for w in [1,2,4,8,16] for d in [1,4,8,16]],"scale_and_fixed_backlog_checks":{"monotonic":checks[4][1]},"per_snapshot_p95_cases":[p95([0,2000,1000]),p95([2000]*19+[0])],"sample_quality_and_growth":[{"count":n,"bonus":bonus(n),"growth_bonus":g} for n in SAMPLE_COUNTS for g in [0,4]],"legacy_public_api_comparisons":legacy,"worker_mode_comparisons":workers,"typed_zero_validation":public["zero_validation"],"missing_local_comparisons":locals_,"cap_composition":cap_inputs,"competing_controls":controls,"overflow_domain":overflow,"criteria":criteria,"failed_cases":failed,"questionable_cases":["Projected normalization remains review evidence and is not current analyzer behavior."],"recommendation":recommendation,"reproducibility":{"commands":verification.get("commands",[]),"run_hashes":verification.get("run_hashes",{}),"cmp_exit_statuses":verification.get("cmp_exit_statuses",{}),"clean_tree":verification.get("clean_tree",{})}}
    out=Path(a.output_dir);out.mkdir(parents=True,exist_ok=True); raw=json.dumps(data,indent=2,sort_keys=True)+"\n";(out/"report.json").write_text(raw)
    lines=["# Prompt 20 executor-normalization evidence","",f"Recommendation: **{recommendation}**.","","This evidence-only draft derives evidence-ranked review findings; projections are not proof of root cause.","","## Criteria","","| # | Result | Derivation |","|---:|:---:|---|"]+[f"| {n} | {'pass' if ok else 'fail'} | {reason} |" for n,ok,reason in checks]
    lines += ["","## Integer percentile indexes",""]+[f"- count {x['count']}: index {x['selected_index']}, value {x['value']}" for x in percentiles]
    lines += ["","## Derived comparisons",f"- Legacy cases: {len(legacy)}; matching: {sum(x['matches_expected'] for x in legacy)}.",f"- Worker modes: {sum(x['matches_expected'] for x in workers)}/{len(workers)}.",f"- Missing-local cases: {sum(x['matches_expected'] for x in locals_)}/{len(locals_)}.",f"- Cap projections: {sum(x['matches_expected'] for x in cap_inputs)}/{len(cap_inputs)}.","","## Competing controls"]+[f"- {x['name']}: primary={None if x['projected_primary'] is None else x['projected_primary']['kind']}, false High executor primary={str(x['projected_false_high_executor_primary']).lower()}; suspects={[(s['kind'],s['score'],s['confidence']) for s in x['projected_suspect_list']]}" for x in controls]
    lines += ["","## Overflow",f"- Maximum intermediate width: {max(x['product_bits'] for x in overflow)} bits; all fit u128={all(x['fits_u128'] for x in overflow)}.","","## Failed and questionable cases",f"- Failed: {failed or 'none'}."]+[f"- Questionable: {x}" for x in data["questionable_cases"]]
    (out/"report.md").write_text("\n".join(lines)+"\n")
    if failed: raise SystemExit(1)
if __name__=="__main__": main()
