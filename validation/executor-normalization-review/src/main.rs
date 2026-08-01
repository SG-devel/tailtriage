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

fn fixture_control(name: &str, fixture: &str, samples: usize, depth: u64, workers: u64) -> Value {
    let template: Value = serde_json::from_str(fixture).expect("fixture must deserialize");
    let mut input = run(
        name,
        &vec![depth; samples],
        &vec![Some(0); samples],
        &vec![Some(workers); samples],
        0,
    );
    let mut value = serde_json::to_value(&input).expect("run JSON");
    for section in ["requests", "stages", "queues", "inflight"] {
        value[section] = template[section].clone();
    }
    // Repeat complete request-correlated evidence so request-count confidence caps do not
    // obscure the control. These are the same minimal public event shapes as analyzer fixtures.
    for section in ["requests", "stages", "queues"] {
        let originals = value[section].as_array().cloned().unwrap_or_default();
        let mut expanded = Vec::new();
        for round in 0..4 {
            for mut event in originals.clone() {
                if let Some(id) = event.get("request_id").and_then(Value::as_str) {
                    event["request_id"] = json!(format!("{id}-{round}"));
                }
                expanded.push(event);
            }
        }
        value[section] = json!(expanded);
    }
    input = serde_json::from_value(value).expect("fixture control Run");
    if name == "strong_blocking" {
        for snapshot in &mut input.runtime_snapshots {
            snapshot.blocking_queue_depth = Some(20);
        }
    }
    json!({"name":name,"typed_input":input,"public_report":analyze_run(&input, AnalyzeOptions::default())})
}

fn zero_evidence(name: &str, workers: &[Option<u64>]) -> Value {
    let input = run(name, &[8, 8], &[Some(3), Some(4)], workers, 0);
    let strict = validate_run_strict(&input)
        .err()
        .map(|e| e.report().issues.iter().map(issue_json).collect::<Vec<_>>());
    let normalized = normalize_run_permissive(&input);
    json!({"name":name,"typed_input":input,"strict_result":strict,
      "permissive_normalized":{"run":normalized.run,
      "issues":normalized.report.issues.iter().map(issue_json).collect::<Vec<_>>(),
      "dispositions":normalized.dispositions.iter().map(|d| json!({"section":d.section.as_str(),"original_index":d.input_index,
        "snapshot_disposition":match &d.disposition { RunEventDispositionKind::Retained{output_index}=>format!("retained:{output_index}"),RunEventDispositionKind::Excluded{issue_codes}=>format!("excluded:{issue_codes:?}") }})).collect::<Vec<_>>()}})
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
    let blocking =
        include_str!("../../../tailtriage-analyzer/tests/fixtures/blocking_pressure.json");
    let downstream =
        include_str!("../../../tailtriage-analyzer/tests/fixtures/downstream_stage.json");
    let queue = include_str!("../../../tailtriage-analyzer/tests/fixtures/queue_saturation.json");
    let controls = vec![
        fixture_control("strong_blocking", blocking, 40, 1, 4),
        fixture_control("downstream", downstream, 40, 2, 4),
        fixture_control("application_queue", queue, 40, 1, 4),
        fixture_control("sparse_runtime", downstream, 7, 32, 4),
        fixture_control("mixed_ambiguity", queue, 40, 16, 4),
        fixture_control("complete_worker_extreme", queue, 100, 32, 4),
    ];
    println!("{}", serde_json::to_string(&json!({
      "generator":"tracked Rust public-API evidence generator", "legacy_cases":legacy,
      "zero_validation":[zero_evidence("zero_first", &[Some(0),Some(4)]),zero_evidence("zero_later", &[Some(4),Some(0)])],
      "controls":controls
    })).expect("serialize"));
}
