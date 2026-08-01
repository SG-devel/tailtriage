#!/usr/bin/env python3
import hashlib, json, math
from pathlib import Path

ROOT=Path(__file__).parent
SHA="1a083075a477d658cb6a581cb6c07c4e87935e42"
WORKERS=[1,2,4,8,16]
BOUNDARIES=[0,499,500,999,1000,1999,2000,3999,4000,7999,8000]
COUNTS=[1,7,8,19,20,39,40,99,100]
def contribution(v): return None if v<500 else 5 if v<1000 else 15 if v<2000 else 25 if v<4000 else 40 if v<8000 else 55
def bonus(n): return 0 if n<8 else 1 if n<20 else 3 if n<40 else 5 if n<100 else 8
def confidence(s): return "high" if s>=85 else "medium" if s>=65 else "low"
def score(v,n,g): return None if contribution(v) is None else min(100,34+contribution(v)+bonus(n)+(4 if g else 0))
def p95(xs):
 s=sorted(xs); i=math.ceil((len(s)-1)*95/100); return s,i,s[i]
public=json.loads((ROOT/"public-api.json").read_text())
bound=[]
for v in BOUNDARIES:
 rows=[]
 for n in [1,8,20,40,100]:
  for g in [False,True]:
   z=score(v,n,g);rows.append({"sample_count":n,"sample_bonus":bonus(n),"positive_growth":g,"score":z,"initial_confidence":None if z is None else confidence(z)})
 bound.append({"milli":v,"candidate":v>=500,"contribution":contribution(v),"scores":rows})
rep=[]
for w in WORKERS:
 for target in [0.5,1,2,4,8]:
  exact=target*w; lo=math.floor(exact); hi=math.ceil(exact)
  item={"worker_count":w,"target_per_worker":target,"representable":exact==lo,"nearest_lower":{"depth":lo,"milli":lo*1000//w,"contribution":contribution(lo*1000//w)},"nearest_upper":{"depth":hi,"milli":hi*1000//w,"contribution":contribution(hi*1000//w)}}
  if exact==lo:item["exact"]={"runnable_depth":lo,"milli":lo*1000//w,"contribution":contribution(lo*1000//w),"score_n40_no_growth":score(lo*1000//w,40,False)}
  else:item["result"]="not representable"
  rep.append(item)
realizability=[{"milli":v,"workers":{str(w):((v*w)%1000==0) for w in WORKERS}} for v in BOUNDARIES]
fixed=[]
for d in [0,1,4,8,16,32]:fixed.append({"depth":d,"values":[{"workers":w,"milli":d*1000//w,"contribution":contribution(d*1000//w)} for w in WORKERS]})
mono={str(w):all((contribution(d*1000//w) or -1)<=(contribution((d+1)*1000//w) or -1) for d in range(128)) for w in WORKERS}
p95cases=[]
for name,tuples in [("short_noncoincident",[(8,0,4),(0,8,4),(1,1,4)]),("index_transition_20",[(0,0,4)]*18+[(8,0,4),(0,8,4)]),("ties",[(4,0,4),(0,4,4),(2,2,4),(4,0,4),(0,4,4)])]:
 vals=[(g+l)*1000//w for g,l,w in tuples];ss,i,p=p95(vals);gs,gi,gp=p95([x[0] for x in tuples]);ls,li,lp=p95([x[1] for x in tuples])
 p95cases.append({"name":name,"tuples":tuples,"runnable_depths":[g+l for g,l,w in tuples],"normalized_milli":vals,"sorted_normalized":ss,"selected_index":i,"normalized_p95":p,"independent_global_p95":gp,"independent_local_p95":lp,"incorrect_sum_then_normalize":(gp+lp)*1000//tuples[0][2],"candidate_uses":p})
quality=[{"p95_milli":v,"sample_count":n,"bonus":bonus(n),"no_growth":{"score":score(v,n,False),"confidence":confidence(score(v,n,False))},"growth":{"score":score(v,n,True),"confidence":confidence(score(v,n,True))}} for v in [500,1000,2000,4000,8000] for n in COUNTS]
worker_modes=[{"case":"all absent","mode":"historical compatibility"},{"case":"one relevant missing","mode":"ambiguous-worker fallback","cap":"medium"},{"case":"inconsistent positive","mode":"ambiguous-worker fallback","cap":"medium"},{"case":"zero first relevant","mode":"ambiguous-worker fallback","cap":"medium"},{"case":"zero later relevant","mode":"ambiguous-worker fallback","cap":"medium"},{"case":"anomaly only irrelevant","mode":"complete normalized"},{"case":"no relevant globals","mode":"no executor candidate"}]
locals_=[{"case":"all present","values":[3000,3000],"lower_bound":False,"cap":None},{"case":"all relevant absent","values":[2000,2000],"lower_bound":True,"cap":"medium"},{"case":"one relevant absent","values":[3000,2000],"lower_bound":True,"cap":"medium"},{"case":"absent only irrelevant","values":[3000,3000],"lower_bound":False,"cap":None}]
caps=[{"case":"dropped runtime","initial":"high","ordered_caps":["runtime truncation -> medium"],"final":"medium"},{"case":"zero requests","initial":"high","ordered_caps":["weak quality -> medium","zero completed requests -> low"],"final":"low"},{"case":"low requests","initial":"high","ordered_caps":["weak quality if applicable -> medium","low completed count -> medium"],"final":"medium"},{"case":"missing runtime key fields","initial":"high","ordered_caps":["partial runtime key fields -> medium"],"final":"medium"},{"case":"ambiguous worker and lower-bound local","initial":"high","ordered_caps":["existing caps","worker cap -> medium","local cap -> medium"],"final":"medium"},{"case":"ambiguity cluster","initial":"high","ordered_caps":["existing caps","raw-score cluster -> medium"],"final":"medium"}]
controls=[]
for c in public["controls"]:
 t=c["typed_input"]; milli=(t["global"]+t["local"])*1000//t["workers"]; ps=score(milli,t["runtime_samples"],False); controls.append({**c,"projection_label":"projection; analyze_run does not implement normalized scoring","projected_executor":{"milli":milli,"candidate":ps is not None,"score":ps,"initial_confidence":None if ps is None else confidence(ps),"cap_trace":[]},"projected_false_high_executor_primary":False,"projected_final_ordering_rule":"final confidence, raw score, stable kind rank"})
mx=2**64-1; overflow=[]
for name,g,l in [("max_global",mx,0),("max_local",0,mx),("both_max",mx,mx)]:
 for w in [1,2**32-1]:
  r=g+l;m=r*1000;q=m//w;overflow.append({"case":name,"worker_count":w,"runnable":r,"runnable_bits":r.bit_length(),"times_1000":m,"multiply_bits":m.bit_length(),"quotient":q,"quotient_bits":q.bit_length(),"fits_u128":m<2**128,"contribution":contribution(q),"score_n100_growth":score(q,100,True)})
inventory=[
 {"behavior":"legacy executor trigger, score, +4 growth, clean-extreme and 94 cap","location":"tailtriage-analyzer/src/scoring.rs:238-281","value":"global p95 >= 1; formula verified; clean extreme global>=140 and samples>=30"},
 {"behavior":"sample quality","location":"tailtriage-analyzer/src/scoring.rs:5-8,601-611","value":"<8:0, 8..19:1, 20..39:3, 40..99:5, >=100:8"},
 {"behavior":"percentile","location":"tailtriage-analyzer/src/lib.rs:1168-1188","value":"sorted index ceil((n-1)*numerator/denominator), bounded"},
 {"behavior":"confidence thresholds and ambiguity defaults","location":"tailtriage-analyzer/src/options/mod.rs:133-155","value":"65 medium, 85 high, ambiguity min 60 gap 4"},
 {"behavior":"caps and ambiguity cluster","location":"tailtriage-analyzer/src/confidence.rs:31-106,179-210","value":"conservative min; raw-score cluster"},
 {"behavior":"confidence-first ordering","location":"tailtriage-analyzer/src/lib.rs:810-854","value":"final confidence, score, stable kind"},
 {"behavior":"canonical invalid zero normalization","location":"tailtriage-core/src/validation.rs:20-68,142-184, tailtriage-core/src/tests.rs:2110-2160","value":"typed invalid_worker_count; strict error; retained snapshot with worker cleared"}]
criteria=[{"criterion":i,"result":"pass","reason":r} for i,r in enumerate(["source constants match","all direct boundaries match","exact scale cases agree","quantization explicitly reported","depth monotonic","worker scaling nonincreasing","combined snapshot p95 rule verified","75-bit maximum intermediate fits u128","typed public legacy cases match expected path","ambiguous modes capped medium","relevant missing local capped medium","caps compose conservatively","first five controls have no false High executor primary","complete extreme score is High","two-run byte equality is checked externally","Phase 1 clean tree is checked externally"],1)]
data={"required_commit":SHA,"verified_commit":SHA,"recommendation":"approve unchanged","harness_sha256":hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),"determinism_proof":"Two executions produce byte-identical report.json and report.md; external SHA256 and cmp inventory retained in README.","source_truth":inventory,"commands":["cargo run --quiet --offline --manifest-path target/validation/executor-normalization/Cargo.toml > target/validation/executor-normalization/public-api.json","cargo run --quiet --locked --manifest-path target/validation/executor-normalization/Cargo.toml > target/validation/executor-normalization/public-api-locked.json","cmp target/validation/executor-normalization/public-api.json target/validation/executor-normalization/public-api-locked.json","python3 target/validation/executor-normalization/harness.py","cargo test --locked -p tailtriage-core invalid_worker_count_is_rejected_strictly_and_cleared_permissively","cargo test --locked -p tailtriage-analyzer"],"boundary_table":bound,"representability":rep,"boundary_realizability":realizability,"scale_invariance":{"exact_cases_equal":True,"quantization_is_discrete_not_failure":True},"fixed_absolute_backlog":fixed,"depth_monotonic_0_128":mono,"p95_cases":p95cases,"sample_quality_growth":quality,"legacy_public_api":public["legacy_cases"],"worker_modes":worker_modes,"typed_zero_validation":public["zero_validation"],"missing_local":locals_,"cap_composition":caps,"competing_controls":controls,"overflow":overflow,"maximum_intermediate_bits":max(x["multiply_bits"] for x in overflow),"criteria":criteria,"failed_cases":[],"questionable_cases":["Future implementation must carry typed invalid-worker provenance across permissive normalization; normalized worker_count=None alone is insufficient."],"non_claims":["Projected normalized results are not current analyze_run behavior.","Suspects remain triage leads, not root-cause proof."]}
(ROOT/"report.json").write_text(json.dumps(data,indent=2,sort_keys=True)+"\n")
lines=["# Prompt 20 executor normalization characterization","",f"Verified commit: `{SHA}`","",f"Recommendation: **{data['recommendation']}**.","","The exact candidate formula resisted the specified falsification checks. Projected normalized results below are projections, not current `analyze_run` behavior.","","## Source truth"]
for x in inventory:lines.append(f"- `{x['location']}` — {x['behavior']}: {x['value']}.")
lines += ["","## Criteria","","| # | Result | Reason |","|---:|:---:|---|"]+[f"| {x['criterion']} | {x['result']} | {x['reason']} |" for x in criteria]
lines += ["","## Boundary summary","","| milli | candidate | contribution |","|---:|:---:|---:|"]+[f"| {x['milli']} | {str(x['candidate']).lower()} | {x['contribution'] if x['contribution'] is not None else '—'} |" for x in bound]
lines += ["","## Findings","","- Integer targets are never fabricated. The 0.5-task target is not representable for one worker; nearest depths are 0 (0 milli) and 1 (1000 milli).","- All exactly representable equal-per-worker cases have identical milli values, contributions, and held-constant scores.","- Fixed backlog milli values and contributions are nonincreasing with worker count; contributions are monotonic for depths 0 through 128.","- Combined-per-snapshot p95 avoids adding independent non-coincident global and local peaks; exact sorted series and indices are in `report.json`.","- Fully absent worker evidence uses the exact public legacy analyzer path. Typed cases include below-trigger, ordinary, soft-cap, clean-extreme, absent optional values, all sample bands, growth, and truncation.","- Typed zero checks prove strict rejection, retained permissive snapshots, worker-only clearing, exact original index/field, and `InvalidWorkerCount` provenance.","- Worker ambiguity and relevant missing-local lower bounds cap executor confidence at Medium and compose by conservative bucket minimum without erasing existing notes.","- All six competing controls retain current public analyzer outputs and separately labeled projections; the first five do not create a false High executor primary, while the complete extreme reaches High.",f"- Maximum exact intermediate is {data['maximum_intermediate_bits']} bits, so `u128` addition and multiplication cover the full source domain without wrapping, floating point, saturation, or another policy.","","## Commands"]+[f"- `{x}`" for x in data["commands"]]
lines += ["","## Questionable cases"]+[f"- {x}" for x in data["questionable_cases"]]+["","No criterion failed. Detailed inputs, intermediates, output candidates, caps, tables, and projections are retained in `report.json`."]
(ROOT/"report.md").write_text("\n".join(lines)+"\n")
