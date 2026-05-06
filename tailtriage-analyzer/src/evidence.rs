use serde::Serialize;
use tailtriage_core::Run;

use super::LOW_COMPLETED_REQUEST_THRESHOLD;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
/// Overall evidence-quality level for this capture.
pub enum EvidenceQualityLevel {
    /// Evidence coverage is sufficient for a strong triage interpretation.
    Strong,
    /// Evidence coverage has important limitations.
    Partial,
    /// Evidence coverage is too sparse/truncated for stable interpretation.
    Weak,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
/// Coverage status for one signal family.
pub enum SignalCoverageStatus {
    /// Signal family has usable data.
    Present,
    /// Signal family is absent.
    Missing,
    /// Signal family exists but has limited interpretability.
    Partial,
    /// Signal family had capture drops due to truncation.
    Truncated,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
/// Structured capture-coverage and interpretation-quality summary.
pub struct EvidenceQuality {
    /// Number of completed request events captured.
    pub request_count: usize,
    /// Number of queue events captured.
    pub queue_event_count: usize,
    /// Number of stage events captured.
    pub stage_event_count: usize,
    /// Number of runtime snapshots captured.
    pub runtime_snapshot_count: usize,
    /// Number of in-flight snapshots captured.
    pub inflight_snapshot_count: usize,
    /// Coverage status for request events.
    pub requests: SignalCoverageStatus,
    /// Coverage status for queue events.
    pub queues: SignalCoverageStatus,
    /// Coverage status for stage events.
    pub stages: SignalCoverageStatus,
    /// Coverage status for runtime snapshots.
    pub runtime_snapshots: SignalCoverageStatus,
    /// Coverage status for in-flight snapshots.
    pub inflight_snapshots: SignalCoverageStatus,
    /// Whether any capture truncation limit was hit.
    pub truncated: bool,
    /// Number of dropped request events.
    pub dropped_requests: u64,
    /// Number of dropped stage events.
    pub dropped_stages: u64,
    /// Number of dropped queue events.
    pub dropped_queues: u64,
    /// Number of dropped in-flight snapshots.
    pub dropped_inflight_snapshots: u64,
    /// Number of dropped runtime snapshots.
    pub dropped_runtime_snapshots: u64,
    /// Overall quality level for this report's evidence coverage.
    pub quality: EvidenceQualityLevel,
    /// Interpretation limitations inferred from coverage/truncation.
    pub limitations: Vec<String>,
}

pub(super) fn evidence_quality(run: &Run) -> EvidenceQuality {
    let requests = request_status(run);
    let queues = family_status(run.queues.is_empty(), run.truncation.dropped_queues);
    let stages = family_status(run.stages.is_empty(), run.truncation.dropped_stages);
    let runtime_snapshots = runtime_status(run);
    let inflight_snapshots = family_status(
        run.inflight.is_empty(),
        run.truncation.dropped_inflight_snapshots,
    );
    let limitations = evidence_limitations(run, queues, stages, runtime_snapshots);
    let non_request_truncated = matches!(queues, SignalCoverageStatus::Truncated)
        || matches!(stages, SignalCoverageStatus::Truncated)
        || matches!(runtime_snapshots, SignalCoverageStatus::Truncated)
        || matches!(inflight_snapshots, SignalCoverageStatus::Truncated);
    let explanatory_present =
        !run.queues.is_empty() || !run.stages.is_empty() || !run.runtime_snapshots.is_empty();
    let quality = if run.requests.is_empty()
        || run.requests.len() < LOW_COMPLETED_REQUEST_THRESHOLD
        || run.truncation.dropped_requests > 0
        || !explanatory_present
    {
        EvidenceQualityLevel::Weak
    } else if non_request_truncated
        || (run.queues.is_empty() && run.stages.is_empty())
        || runtime_snapshots == SignalCoverageStatus::Partial
    {
        EvidenceQualityLevel::Partial
    } else {
        EvidenceQualityLevel::Strong
    };

    EvidenceQuality {
        request_count: run.requests.len(),
        queue_event_count: run.queues.len(),
        stage_event_count: run.stages.len(),
        runtime_snapshot_count: run.runtime_snapshots.len(),
        inflight_snapshot_count: run.inflight.len(),
        requests,
        queues,
        stages,
        runtime_snapshots,
        inflight_snapshots,
        truncated: run.truncation.is_truncated() || run.truncation.limits_hit,
        dropped_requests: run.truncation.dropped_requests,
        dropped_stages: run.truncation.dropped_stages,
        dropped_queues: run.truncation.dropped_queues,
        dropped_inflight_snapshots: run.truncation.dropped_inflight_snapshots,
        dropped_runtime_snapshots: run.truncation.dropped_runtime_snapshots,
        quality,
        limitations,
    }
}

fn request_status(run: &Run) -> SignalCoverageStatus {
    if run.requests.is_empty() {
        SignalCoverageStatus::Missing
    } else if run.truncation.dropped_requests > 0 {
        SignalCoverageStatus::Truncated
    } else if run.requests.len() < LOW_COMPLETED_REQUEST_THRESHOLD {
        SignalCoverageStatus::Partial
    } else {
        SignalCoverageStatus::Present
    }
}

fn family_status(is_empty: bool, dropped: u64) -> SignalCoverageStatus {
    if dropped > 0 {
        SignalCoverageStatus::Truncated
    } else if is_empty {
        SignalCoverageStatus::Missing
    } else {
        SignalCoverageStatus::Present
    }
}

fn runtime_status(run: &Run) -> SignalCoverageStatus {
    if run.truncation.dropped_runtime_snapshots > 0 {
        SignalCoverageStatus::Truncated
    } else if run.runtime_snapshots.is_empty() {
        SignalCoverageStatus::Missing
    } else if run
        .runtime_snapshots
        .iter()
        .all(|s| s.blocking_queue_depth.is_none())
        || run
            .runtime_snapshots
            .iter()
            .all(|s| s.local_queue_depth.is_none())
        || run
            .runtime_snapshots
            .iter()
            .all(|s| s.global_queue_depth.is_none())
    {
        SignalCoverageStatus::Partial
    } else {
        SignalCoverageStatus::Present
    }
}

fn evidence_limitations(
    run: &Run,
    queues: SignalCoverageStatus,
    stages: SignalCoverageStatus,
    runtime_snapshots: SignalCoverageStatus,
) -> Vec<String> {
    let mut limitations = Vec::new();
    if run.requests.len() < LOW_COMPLETED_REQUEST_THRESHOLD {
        limitations
            .push("Low completed-request count can make suspect ranking unstable.".to_string());
    }
    if matches!(
        queues,
        SignalCoverageStatus::Missing | SignalCoverageStatus::Truncated
    ) && matches!(
        stages,
        SignalCoverageStatus::Missing | SignalCoverageStatus::Truncated
    ) {
        limitations.push("Queue and stage instrumentation are both unavailable, limiting application vs downstream interpretation.".to_string());
    }
    if run.runtime_snapshots.is_empty() {
        limitations.push("Runtime snapshots are missing, limiting executor and blocking-pressure interpretation.".to_string());
    } else if runtime_snapshots == SignalCoverageStatus::Partial {
        limitations.push("Runtime snapshots have missing queue-depth fields, limiting executor vs blocking differentiation.".to_string());
    }
    if run.truncation.is_truncated() || run.truncation.limits_hit {
        limitations.push(
            "Capture truncation dropped evidence and can reduce diagnosis completeness."
                .to_string(),
        );
    }
    limitations
}

pub(super) fn truncation_warnings(run: &Run) -> Vec<String> {
    let mut warnings = Vec::new();
    if run.truncation.limits_hit || run.truncation.is_truncated() {
        warnings.push("Capture limits were hit during this run; dropped evidence can reduce diagnosis completeness and confidence.".to_string());
    }
    if run.truncation.dropped_requests > 0 {
        warnings.push(format!("Capture truncated requests: dropped {} request events after reaching the configured max_requests limit. This dropped evidence can reduce diagnosis completeness and confidence.", run.truncation.dropped_requests));
    }
    if run.truncation.dropped_stages > 0 {
        warnings.push(format!("Capture truncated stages: dropped {} stage events after reaching the configured max_stages limit. This dropped evidence can reduce diagnosis completeness and confidence.", run.truncation.dropped_stages));
    }
    if run.truncation.dropped_queues > 0 {
        warnings.push(format!("Capture truncated queues: dropped {} queue events after reaching the configured max_queues limit. This dropped evidence can reduce diagnosis completeness and confidence.", run.truncation.dropped_queues));
    }
    if run.truncation.dropped_inflight_snapshots > 0 {
        warnings.push(format!("Capture truncated in-flight snapshots: dropped {} entries after reaching max_inflight_snapshots. This dropped evidence can reduce diagnosis completeness and confidence.", run.truncation.dropped_inflight_snapshots));
    }
    if run.truncation.dropped_runtime_snapshots > 0 {
        warnings.push(format!("Capture truncated runtime snapshots: dropped {} entries after reaching max_runtime_snapshots. This dropped evidence can reduce diagnosis completeness and confidence.", run.truncation.dropped_runtime_snapshots));
    }
    warnings
}
