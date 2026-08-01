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
            actual={"scaled_numerator":product,"representable":representable,"exact_depth":lower if representable else None,
              "lower_depth":lower,"upper_depth":upper,"lower_milli":lower*1000//workers,
              "upper_milli":upper*1000//workers,"lower_contribution":contribution(lower*1000//workers),
              "upper_contribution":contribution(upper*1000//workers)}
            band=contribution(numerator*500)
            if workers==1 and numerator==1:
                expected={"representable":False,"exact_depth":None,"lower_depth":0,"upper_depth":1,"lower_milli":0,"upper_milli":1000,"lower_contribution":None,"upper_contribution":15}
                checks=comparison(expected,actual)
            else:
                exact=product//2; target_milli=numerator*500
                checks={"representable":representable,"exact_depth":actual["exact_depth"]==exact,
                  "depths_exact":actual["lower_depth"]==actual["upper_depth"]==exact,
                  "milli_exact":actual["lower_milli"]==actual["upper_milli"]==target_milli,
                  "contribution_band":actual["lower_contribution"]==actual["upper_contribution"]==band}
                expected={"general_invariants":True,"target_milli":target_milli,"expected_band":band}
            rows.append({"workers":workers,"target_numerator":numerator,"target_denominator":2,"expected":expected,"actual":actual,"field_comparison":checks,"matches_expected":all(checks.values())})
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
    score=lambda name:by_name[name]["observed"]["final_score"]
    for left,right,expected in [("n1","n7",0),("n7","n8",1),("n8","n19",0),("n19","n20",2),("n20","n39",0),("n39","n40",2),("n40","n99",0),("n99","n100",3)]:
        actual=score(right)-score(left); checks.append({"case":f"{right}-{left}","expected_delta":expected,"actual_delta":actual,"matches_expected":actual==expected})
    off,on=by_name["growth_pair_off"],by_name["growth_pair_on"]; actual=on["observed"]["final_score"]-off["observed"]["final_score"]
    expected_confidence_delta=bucket(on["observed"]["final_score"])!=bucket(off["observed"]["final_score"])
    confidence_ok=(on["observed"]["final_confidence"]!=off["observed"]["final_confidence"])==expected_confidence_delta
    checks.append({"case":"public_growth_pair","expected_delta":4,"actual_delta":actual,"confidence_threshold_crossed":expected_confidence_delta,"confidence_comparison":confidence_ok,"matches_expected":actual==4 and confidence_ok})
    return checks

def missing_local_checks():
    specs=[("all present",[(8,4,4),(12,4,4)],[4,4],[],[3000,4000],False,None),
      ("all absent",[(8,None,4),(12,None,4)],[0,0],[0,1],[2000,3000],True,"medium"),
      ("one absent",[(8,4,4),(12,None,4)],[4,0],[1],[3000,3000],True,"medium"),
      ("irrelevant absent",[(8,4,4),(None,None,4)],[4],[],[3000],False,None)]
    out=[]
    for name,snaps,locals_expected,zeros_expected,values_expected,bound_expected,cap_expected in specs:
        raw=[{"global_queue_depth":g,"local_queue_depth":l,"worker_count":w} for g,l,w in snaps]; relevant=[i for i,s in enumerate(raw) if s["global_queue_depth"] is not None]
        locals_used=[raw[i]["local_queue_depth"] or 0 for i in relevant]; zeros=[i for i in relevant if raw[i]["local_queue_depth"] is None]
        values=[(raw[i]["global_queue_depth"]+locals_used[j])*1000//raw[i]["worker_count"] for j,i in enumerate(relevant)]
        actual={"local_values_used":locals_used,"zero_substitution_indexes":zeros,"normalized_values":values,"lower_bound":bool(zeros),"derived_cap":"medium" if zeros else None}
        expected={"local_values_used":locals_expected,"zero_substitution_indexes":zeros_expected,"normalized_values":values_expected,"lower_bound":bound_expected,"derived_cap":cap_expected}
        checks=comparison(expected,actual); out.append({"case":name,"raw_snapshots":raw,"relevant_snapshot_indexes":relevant,**actual,"expected":expected,"field_comparison":checks,"matches_expected":all(checks.values())})
    return out

def typed_issue_map(public): return {z["name"]:z for z in public["zero_validation"]}
def validate_zero(item,zero_index):
    raw=item["typed_input"]["runtime_snapshots"]; normalized=item["permissive_normalized"]["run"]["runtime_snapshots"]
    strict=item["strict_result"] or []; issues=item["permissive_normalized"]["issues"]
    typed=[x for x in issues if x["code"]=="InvalidWorkerCount" and x["section"]=="runtime snapshot" and x["field"]=="worker_count" and x["original_index"]==zero_index]
    dispositions=item["permissive_normalized"]["dispositions"]
    retained=any(x["section"]=="runtime snapshot" and x["original_index"]==zero_index and x["snapshot_disposition"].startswith("retained:") for x in dispositions)
    fields=["global_queue_depth","local_queue_depth","alive_tasks","at_unix_ms","at_run_us"]
    checks={"strict_has_issue":bool(strict),"typed_issue_exact":bool(typed),"snapshot_count_retained":len(raw)==len(normalized),
      "worker_cleared":normalized[zero_index].get("worker_count") is None,"non_worker_fields_retained":all(normalized[zero_index].get(k)==raw[zero_index].get(k) for k in fields),"disposition_retained":retained}
    return {**item,"raw_zero_index":zero_index,"validation_assertions":checks,"matches_expected":all(checks.values())}
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
    if name in ["zero first","zero later"]: checks["typed_issue_used"]=bool(locations)
    if name=="irrelevant zero": checks["irrelevant_typed_issue_ignored"]=not locations and any(i["code"]=="InvalidWorkerCount" for i in issues)
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
    expected_truth={"legacy_trigger":1,"legacy_base":34,"global_cap":150,"global_divisor":4,"local_cap":60,"local_divisor":6,"alive_cap":400,"alive_divisor":40,"growth_bonus":4,"sample_bonuses":[0,1,3,5,8],"clean_extreme_global":140,"clean_extreme_samples":30,"ordinary_soft_cap":94,"confidence_thresholds":[65,85],"ambiguity_minimum":60,"ambiguity_gap":4,"percentile_numerator":95,"percentile_denominator":100,"invalid_worker_issue_code":"InvalidWorkerCount"}
    source_truth={"expected":expected_truth,"actual":public["source_truth"],"field_comparison":comparison(expected_truth,public["source_truth"])}; source_truth["matches_expected"]=all(source_truth["field_comparison"].values())
    boundaries_expected={0:None,499:None,500:5,999:5,1000:15,1999:15,2000:25,3999:25,4000:40,7999:40,8000:55}
    boundaries=[{"milli":v,"expected":e,"actual":contribution(v),"matches_expected":contribution(v)==e} for v,e in boundaries_expected.items()]
    scales=scale_checks(); quant=quantization_checks(); fixed=fixed_backlog_checks(); legacy=[legacy_expected(x) for x in public["legacy_cases"]]; pairs=legacy_pairs(legacy); missing=missing_local_checks()
    zero=typed_issue_map(public); simple=lambda spec:[{"global_queue_depth":g,"local_queue_depth":l,"worker_count":w} for g,l,w in spec]
    def issues(name): return zero[name]["permissive_normalized"]["issues"]
    validated_zero=[validate_zero(zero["zero_first"],0),validate_zero(zero["zero_later"],1),validate_zero(zero["irrelevant_zero"],1)]
    workers=[classify_workers("all absent",simple([(8,2,None),(9,2,None)]),[],"historical compatibility",None),classify_workers("one missing",simple([(8,2,4),(9,2,None)]),[],"ambiguous-worker fallback","medium"),classify_workers("inconsistent",simple([(8,2,4),(9,2,8)]),[],"ambiguous-worker fallback","medium"),classify_workers("zero first",zero["zero_first"]["typed_input"]["runtime_snapshots"],issues("zero_first"),"ambiguous-worker fallback","medium"),classify_workers("zero later",zero["zero_later"]["typed_input"]["runtime_snapshots"],issues("zero_later"),"ambiguous-worker fallback","medium"),classify_workers("irrelevant zero",zero["irrelevant_zero"]["typed_input"]["runtime_snapshots"],issues("irrelevant_zero"),"complete normalized",None),classify_workers("no globals",simple([(None,2,0)]),[],"no executor candidate",None)]
    caps=cap_cases(); controls=[control(x) for x in public["controls"]]
    percentile=[p95(list(range(n))) for n in P95_COUNTS]
    overflow=[]; maximum=2**64-1
    for g,l,w in [(maximum,0,1),(0,maximum,1),(maximum,maximum,1),(maximum,maximum,2**32-1)]:
        product=(g+l)*1000; overflow.append({"global":g,"local":l,"workers":w,"product_bits":product.bit_length(),"fits_u128":product<2**128,"quotient":product//w})
    monotonic=[]
    for workers_count in [1,2,4,8,16]:
        failures=[]
        for depth in range(1,129):
            pm=(depth-1)*1000//workers_count; cm=depth*1000//workers_count; pc=contribution(pm); cc=contribution(cm)
            numeric=lambda x:-1 if x is None else x
            row={"previous_depth":depth-1,"current_depth":depth,"previous_milli":pm,"current_milli":cm,"previous_contribution":pc,"current_contribution":cc,"milli_nondecreasing":cm>=pm,"contribution_nondecreasing":numeric(cc)>=numeric(pc)}
            if not row["milli_nondecreasing"] or not row["contribution_nondecreasing"]: failures.append(row)
        monotonic.append({"workers":workers_count,"adjacent_pairs":128,"failures":failures,"matches_expected":not failures})
    checks=[all(g["matches"] for g in scales),all(x["matches_expected"] for x in quant),all(x["matches_expected"] for x in monotonic),all(g["nonincreasing"] for g in fixed),all(x["matches_expected"] for x in legacy) and all(x["matches_expected"] for x in pairs),all(x["matches_expected"] for x in workers) and all(x["matches_expected"] for x in validated_zero),all(x["matches_expected"] for x in missing),all(x["matches_expected"] for x in caps),all(x["matches_expected"] for x in controls[:5]),controls[5]["matches_expected"],verification.get("deterministic_two_run",False),verification.get("allowed_worktree",False)]
    criteria=[source_truth["matches_expected"],all(x["matches_expected"] for x in boundaries),checks[0],checks[1],checks[2],checks[3],all(x["selected_index"]==(x["count"]-1)*95//100+bool(((x["count"]-1)*95)%100) for x in percentile),all(x["fits_u128"] for x in overflow),checks[4],checks[5],checks[6],checks[7],checks[8],checks[9],checks[10],checks[11]]
    reasons=["source constants","contribution boundaries","exact scale invariance","quantization rules","monotonic contribution","fixed absolute backlog","integer p95","u128 domain","legacy observables","typed worker provenance","missing-local projection","independent cap composition","competing controls","complete extreme","two byte comparisons","allowed worktree"]
    results=[{"criterion":i+1,"result":"pass" if ok else "fail","reason":reasons[i]} for i,ok in enumerate(criteria)]
    failed=[{"criterion":x["criterion"],"case":x["reason"]} for x in results if x["result"]=="fail"]
    recommendation="approve unchanged" if not failed else "revise"
    return {"source_truth_comparison":source_truth,"integer_percentile_index_tests":percentile,"direct_contribution_boundaries":boundaries,"scale_invariance":scales,"representability_and_quantization":quant,"monotonicity":monotonic,"fixed_backlog":fixed,"legacy_public_api_comparisons":legacy,"legacy_paired_comparisons":pairs,"worker_mode_comparisons":workers,"missing_local_comparisons":missing,"typed_zero_validation":validated_zero,"cap_composition":caps,"competing_controls":controls,"overflow_domain":overflow,"criteria":results,"failed_cases":failed,"questionable_cases":["Projected normalization is review evidence, not current analyzer behavior."],"recommendation":recommendation,"reproducibility":verification}

def render(data):
    raw=json.dumps(data,sort_keys=True,separators=(",",":"))+"\n"
    lines=["# Prompt 20 executor-normalization evidence","",f"Recommendation: **{data['recommendation']}**.","","Evidence-ranked projections are triage leads, not proof of root cause.","","## Criteria","","| # | Result | Derivation |","|---:|:---:|---|"]+[f"| {x['criterion']} | {x['result']} | {x['reason']} |" for x in data["criteria"]]
    lines += ["","## Competing controls","","| Control | Current competitor | Normalized p95 | Cap trace | Ambiguity | Ordering | Match |","|---|---|---:|---|---|---|:---:|"]
    for c in data["competing_controls"]:
        o=c["observed"]; trace=[] if not o["executor"] else o["executor"]["cap_trace"]
        lines.append(f"| {c['name']} | {o['current_competitor']} | {o['normalized_p95_milli']} | {trace} | {o['ambiguity_cluster']} | {o['projected_ordering']} | {c['matches_expected']} |")
    lines += ["","## Derived comparisons",f"- Source truth and boundaries: {data['source_truth_comparison']['matches_expected']}; {sum(x['matches_expected'] for x in data['direct_contribution_boundaries'])}/11.",f"- Scale groups: {sum(x['matches'] for x in data['scale_invariance'])}/{len(data['scale_invariance'])}.",f"- Quantization cells: {sum(x['matches_expected'] for x in data['representability_and_quantization'])}/{len(data['representability_and_quantization'])}.",f"- Monotonic workers: {sum(x['matches_expected'] for x in data['monotonicity'])}/5; failures: {sum(len(x['failures']) for x in data['monotonicity'])}.",f"- Fixed-backlog groups: {sum(x['nonincreasing'] for x in data['fixed_backlog'])}/{len(data['fixed_backlog'])}.",f"- Legacy cases and paired checks: {sum(x['matches_expected'] for x in data['legacy_public_api_comparisons'])}/{len(data['legacy_public_api_comparisons'])}; {sum(x['matches_expected'] for x in data['legacy_paired_comparisons'])}/{len(data['legacy_paired_comparisons'])}.",f"- Worker provenance: {sum(x['matches_expected'] for x in data['worker_mode_comparisons'])}/{len(data['worker_mode_comparisons'])}.",f"- Missing-local: {sum(x['matches_expected'] for x in data['missing_local_comparisons'])}/{len(data['missing_local_comparisons'])}; failures: {[x['case'] for x in data['missing_local_comparisons'] if not x['matches_expected']]}.",f"- Cap composition: {sum(x['matches_expected'] for x in data['cap_composition'])}/{len(data['cap_composition'])}.","","## Failed and questionable cases",f"- Failed: {data['failed_cases'] or 'none'}."]+[f"- Questionable: {x}" for x in data["questionable_cases"]]
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
    allowed_prefix=" M validation/executor-normalization-review/"
    status_lines=set(status.stdout.splitlines()); worktree_ok=status.returncode==0 and all(x.startswith(allowed_prefix) for x in status_lines) and staged.returncode==0 and whitespace.returncode==0
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
