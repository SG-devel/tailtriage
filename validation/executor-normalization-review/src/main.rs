use serde_json::{json, Value};
use tailtriage_analyzer::{analyze_run, AnalyzeOptions};
use tailtriage_core::{
    normalize_run_permissive, validate_run_strict, Run, RunEventDispositionKind, RunValidationIssue,
};

fn issue_json(issue: &RunValidationIssue) -> Value {
    json!({"code":format!("{:?}",issue.code),"severity":format!("{:?}",issue.severity),
      "section":issue.location.section.as_str(),"original_index":issue.location.index,
      "field":issue.location.field,"message":issue.message})
}

fn run(
    name: &str,
    globals: &[u64],
    locals: &[Option<u64>],
    workers: &[Option<u64>],
    dropped: u64,
) -> Run {
    let snapshots: Vec<Value> = globals
        .iter()
        .enumerate()
        .map(|(i, global)| {
            json!({
                "at_unix_ms": i + 1, "at_run_us": i * 1000, "alive_tasks": 20,
                "global_queue_depth": global, "local_queue_depth": locals[i],
                "blocking_queue_depth": 0, "remote_schedule_count": 0,
                "worker_count": workers[i]
            })
        })
        .collect();
    serde_json::from_value(json!({
        "schema_version": 2,
        "metadata": {"run_id":name,"service_name":"evidence","service_version":null,
          "started_at_unix_ms":1,"finalized_at_unix_ms":2,"mode":"light","host":null,"pid":null},
        "requests":[],"stages":[],"queues":[],"inflight":[],"runtime_snapshots":snapshots,
        "truncation":{"limits_hit":dropped>0,"dropped_requests":0,"dropped_stages":0,
          "dropped_queues":0,"dropped_inflight_snapshots":0,"dropped_runtime_snapshots":dropped}
    }))
    .expect("evidence Run must deserialize")
}

fn evidence(
    name: &str,
    globals: Vec<u64>,
    locals: Vec<Option<u64>>,
    workers: Vec<Option<u64>>,
    dropped: u64,
) -> Value {
    let input = run(name, &globals, &locals, &workers, dropped);
    json!({"name":name,"typed_input":input,"public_report":analyze_run(&input, AnalyzeOptions::default())})
}

fn main() {
    let mut legacy = Vec::new();
    for (name, n, global, local, alive, growth, dropped) in [
        ("below_trigger", 1, 0, None, None, false, 0),
        ("ordinary", 8, 4, Some(6), Some(40), false, 0),
        ("soft_cap_94", 20, 150, Some(60), Some(400), false, 0),
        ("clean_extreme", 40, 150, Some(60), Some(400), true, 0),
        ("absent_optional", 7, 4, None, None, false, 0),
        ("n1", 1, 4, Some(0), Some(0), false, 0),
        ("n7", 7, 4, Some(0), Some(0), true, 0),
        ("n8", 8, 4, Some(0), Some(0), false, 0),
        ("n19", 19, 4, Some(0), Some(0), true, 0),
        ("n20", 20, 4, Some(0), Some(0), false, 0),
        ("n39", 39, 4, Some(0), Some(0), true, 0),
        ("n40", 40, 4, Some(0), Some(0), false, 0),
        ("n99", 99, 4, Some(0), Some(0), true, 0),
        ("n100", 100, 4, Some(0), Some(0), false, 0),
        ("runtime_truncated", 40, 150, Some(60), Some(400), true, 1),
    ] {
        let globals = vec![global; n];
        let locals = vec![local; n];
        let workers = vec![None; n];
        let mut item = evidence(name, globals, locals, workers, dropped);
        if let Some(alive) = alive {
            for snapshot in item["typed_input"]["runtime_snapshots"]
                .as_array_mut()
                .expect("snapshots")
            {
                snapshot["alive_tasks"] = json!(alive);
            }
        }
        if growth {
            item["typed_input"]["inflight"] = json!([
              {"gauge":"evidence","at_unix_ms":1,"count":1},
              {"gauge":"evidence","at_unix_ms":2,"count":4},
              {"gauge":"evidence","at_unix_ms":3,"count":8}
            ]);
        }
        let adjusted: Run =
            serde_json::from_value(item["typed_input"].clone()).expect("adjusted run");
        item["public_report"] = json!(analyze_run(&adjusted, AnalyzeOptions::default()));
        item["growth"] = json!(growth);
        item["alive_p95_input"] = json!(alive);
        legacy.push(item);
    }
    let zero_run = run(
        "typed_zero",
        &[8, 8],
        &[Some(3), Some(4)],
        &[Some(4), Some(0)],
        0,
    );
    let strict = validate_run_strict(&zero_run)
        .err()
        .map(|e| e.report().issues.iter().map(issue_json).collect::<Vec<_>>());
    let normalized = normalize_run_permissive(&zero_run);
    let normalized_json = json!({"run":normalized.run,"issues":normalized.report.issues.iter().map(issue_json).collect::<Vec<_>>(),
      "dispositions":normalized.dispositions.iter().map(|d| json!({"section":d.section.as_str(),"original_index":d.input_index,
        "snapshot_disposition":match &d.disposition { RunEventDispositionKind::Retained{output_index}=>format!("retained:{output_index}"),RunEventDispositionKind::Excluded{issue_codes}=>format!("excluded:{issue_codes:?}") }})).collect::<Vec<_>>()});
    let controls = [
        (
            "strong_blocking",
            vec![1; 20],
            vec![Some(0); 20],
            vec![Some(4); 20],
        ),
        (
            "downstream",
            vec![8; 20],
            vec![Some(0); 20],
            vec![Some(4); 20],
        ),
        (
            "application_queue",
            vec![1; 20],
            vec![Some(0); 20],
            vec![Some(4); 20],
        ),
        (
            "sparse_runtime",
            vec![32; 7],
            vec![Some(0); 7],
            vec![Some(4); 7],
        ),
        (
            "mixed_ambiguity",
            vec![16; 40],
            vec![Some(0); 40],
            vec![Some(4); 40],
        ),
        (
            "complete_worker_extreme",
            vec![32; 100],
            vec![Some(0); 100],
            vec![Some(4); 100],
        ),
    ]
    .into_iter()
    .map(|(n, g, l, w)| evidence(n, g, l, w, 0))
    .collect::<Vec<_>>();
    println!("{}", serde_json::to_string_pretty(&json!({
      "generator":"tracked Rust public-API evidence generator", "legacy_cases":legacy,
      "zero_validation":{"typed_input":zero_run,"strict_result":strict,"permissive_normalized":normalized_json},
      "controls":controls
    })).expect("serialize"));
}
