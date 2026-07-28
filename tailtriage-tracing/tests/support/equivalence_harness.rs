#![allow(clippy::semicolon_if_nothing_returned)]
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tailtriage_analyzer::{analyze_run, AnalyzeOptions, Report};
use tailtriage_core::{CaptureLimits, CaptureMode, EffectiveCoreConfig, Run};
use tailtriage_tracing::{import_jsonl_path, ImportOptions};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComparableRequest {
    pub request_id: String,
    pub route: String,
    pub kind: Option<String>,
    pub outcome: String,
    pub latency_us: u64,
    pub started_at_run_us: Option<u64>,
    pub finished_at_run_us: Option<u64>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComparableStage {
    pub request_id: String,
    pub stage: String,
    pub success: bool,
    pub completed: bool,
    pub latency_us: u64,
    pub started_at_run_us: Option<u64>,
    pub finished_at_run_us: Option<u64>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComparableQueue {
    pub request_id: String,
    pub queue: String,
    pub depth_at_start: Option<u64>,
    pub completed: bool,
    pub wait_us: u64,
    pub waited_from_run_us: Option<u64>,
    pub waited_until_run_us: Option<u64>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComparableSemanticTruncation {
    pub limits_hit: bool,
    pub dropped_requests: u64,
    pub dropped_stages: u64,
    pub dropped_queues: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepresentableRunProjection {
    pub service_name: String,
    pub mode: CaptureMode,
    pub effective_core_config: Option<EffectiveCoreConfig>,
    pub requests: Vec<ComparableRequest>,
    pub stages: Vec<ComparableStage>,
    pub queues: Vec<ComparableQueue>,
    pub semantic_truncation: ComparableSemanticTruncation,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnsupportedParityEvidence {
    PartialStage,
    PartialQueue,
    Inflight,
    RuntimeSnapshots,
}

pub fn project_run(run: &Run) -> Result<RepresentableRunProjection, UnsupportedParityEvidence> {
    if run.stages.iter().any(|x| !x.completed) {
        return Err(UnsupportedParityEvidence::PartialStage);
    }
    if run.queues.iter().any(|x| !x.completed) {
        return Err(UnsupportedParityEvidence::PartialQueue);
    }
    if !run.inflight.is_empty() {
        return Err(UnsupportedParityEvidence::Inflight);
    }
    if !run.runtime_snapshots.is_empty() {
        return Err(UnsupportedParityEvidence::RuntimeSnapshots);
    }
    Ok(RepresentableRunProjection {
        service_name: run.metadata.service_name.clone(),
        mode: run.metadata.mode,
        effective_core_config: run.metadata.effective_core_config,
        requests: run
            .requests
            .iter()
            .map(|x| ComparableRequest {
                request_id: x.request_id.clone(),
                route: x.route.clone(),
                kind: x.kind.clone(),
                outcome: x.outcome.clone(),
                latency_us: x.latency_us,
                started_at_run_us: x.started_at_run_us,
                finished_at_run_us: x.finished_at_run_us,
            })
            .collect(),
        stages: run
            .stages
            .iter()
            .map(|x| ComparableStage {
                request_id: x.request_id.clone(),
                stage: x.stage.clone(),
                success: x.success,
                completed: x.completed,
                latency_us: x.latency_us,
                started_at_run_us: x.started_at_run_us,
                finished_at_run_us: x.finished_at_run_us,
            })
            .collect(),
        queues: run
            .queues
            .iter()
            .map(|x| ComparableQueue {
                request_id: x.request_id.clone(),
                queue: x.queue.clone(),
                depth_at_start: x.depth_at_start,
                completed: x.completed,
                wait_us: x.wait_us,
                waited_from_run_us: x.waited_from_run_us,
                waited_until_run_us: x.waited_until_run_us,
            })
            .collect(),
        semantic_truncation: ComparableSemanticTruncation {
            limits_hit: run.truncation.limits_hit,
            dropped_requests: run.truncation.dropped_requests,
            dropped_stages: run.truncation.dropped_stages,
            dropped_queues: run.truncation.dropped_queues,
        },
    })
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComparableReportProjection {
    pub request_count: usize,
    pub p50_latency_us: Option<u64>,
    pub p95_latency_us: Option<u64>,
    pub p99_latency_us: Option<u64>,
    pub p95_queue_share_permille: Option<u64>,
    pub p95_service_share_permille: Option<u64>,
    pub inflight_trend: Value,
    pub evidence_quality: Value,
    pub primary_suspect: Value,
    pub secondary_suspects: Value,
    pub warnings: Vec<String>,
    pub route_breakdowns: Value,
    pub temporal_segments: Value,
}
pub fn project_report(r: &Report) -> ComparableReportProjection {
    let v = serde_json::to_value(r).unwrap();
    let mut temporal = v["temporal_segments"].clone();
    for x in temporal.as_array_mut().unwrap() {
        x.as_object_mut().unwrap().remove("started_at_unix_ms");
        x.as_object_mut().unwrap().remove("finished_at_unix_ms");
    }
    ComparableReportProjection {
        request_count: r.request_count,
        p50_latency_us: r.p50_latency_us,
        p95_latency_us: r.p95_latency_us,
        p99_latency_us: r.p99_latency_us,
        p95_queue_share_permille: r.p95_queue_share_permille,
        p95_service_share_permille: r.p95_service_share_permille,
        inflight_trend: v["inflight_trend"].clone(),
        evidence_quality: v["evidence_quality"].clone(),
        primary_suspect: v["primary_suspect"].clone(),
        secondary_suspects: v["secondary_suspects"].clone(),
        warnings: r.warnings.clone(),
        route_breakdowns: v["route_breakdowns"].clone(),
        temporal_segments: temporal,
    }
}
pub fn fixture_path(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/equivalence")
        .join(format!("{name}.jsonl"))
}
pub fn import_case(name: &str, limits: Option<CaptureLimits>) -> Run {
    let mut o = ImportOptions::new("equivalence-service").mode(CaptureMode::Light);
    if let Some(l) = limits {
        o = o.capture_limits(l)
    }
    import_jsonl_path(fixture_path(name), o)
        .unwrap()
        .run()
        .clone()
}
pub fn native_case(name: &str) -> Run {
    serde_json::from_str(include_str!("../fixtures/equivalence/native_runs.json"))
        .map(|m: std::collections::BTreeMap<String, Run>| m[name].clone())
        .unwrap()
}
pub fn report(run: &Run) -> ComparableReportProjection {
    project_report(&analyze_run(run, AnalyzeOptions::default()))
}

pub fn expected_run(name: &str) -> RepresentableRunProjection {
    serde_json::from_str(
        &std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/expected/equivalence")
                .join(format!("{name}.run.json")),
        )
        .unwrap(),
    )
    .unwrap()
}
pub fn expected_report(name: &str) -> ComparableReportProjection {
    serde_json::from_str(
        &std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/expected/equivalence")
                .join(format!("{name}.report.json")),
        )
        .unwrap(),
    )
    .unwrap()
}
