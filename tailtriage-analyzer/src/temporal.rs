use tailtriage_core::{RequestEvent, Run};

use super::{
    analyze_run_internal, DiagnosisKind, SignalCoverageStatus, TemporalSegment,
    TEMPORAL_MIN_REQUEST_COUNT, TEMPORAL_MIN_SEGMENT_REQUEST_COUNT, TEMPORAL_SHARE_SHIFT_PERMILLE,
};
use crate::route;

const TEMPORAL_RUNTIME_ATTRIBUTION_WARNING: &str = "Runtime and in-flight evidence is sparse in this segment after timestamp filtering; executor/blocking attribution is limited.";
pub(super) const TEMPORAL_SUSPECT_SHIFT_WARNING: &str = "Temporal segments show different primary suspects; inspect temporal_segments before acting on the global suspect.";
pub(super) const TEMPORAL_P95_SHIFT_WARNING: &str =
    "Temporal segments show a large p95 latency shift between early and late requests.";
pub(super) const TEMPORAL_OVERLAP_ATTRIBUTION_WARNING: &str = "Segment windows overlap under concurrent requests; timestamp-filtered runtime/in-flight attribution is approximate.";

fn filtered_run_for_temporal_segment(
    run: &Run,
    request_ids: &[String],
    start: u64,
    end: u64,
) -> Run {
    let mut filtered = route::filtered_run_for_route(run, request_ids);
    filtered.runtime_snapshots = run
        .runtime_snapshots
        .iter()
        .filter(|s| s.at_unix_ms >= start && s.at_unix_ms <= end)
        .cloned()
        .collect();
    filtered.inflight = run
        .inflight
        .iter()
        .filter(|s| s.at_unix_ms >= start && s.at_unix_ms <= end)
        .cloned()
        .collect();
    filtered
}

pub(super) fn temporal_segments(
    run: &Run,
    global_warnings: &mut Vec<String>,
) -> Vec<TemporalSegment> {
    if run.requests.len() < TEMPORAL_MIN_REQUEST_COUNT {
        return vec![];
    }
    let mut requests = run.requests.clone();
    requests.sort_by(|a, b| {
        a.started_at_unix_ms
            .cmp(&b.started_at_unix_ms)
            .then_with(|| a.request_id.cmp(&b.request_id))
    });
    let split = requests.len() / 2;
    let (early, late) = requests.split_at(split);
    if early.len() < TEMPORAL_MIN_SEGMENT_REQUEST_COUNT
        || late.len() < TEMPORAL_MIN_SEGMENT_REQUEST_COUNT
    {
        return vec![];
    }
    let build = |name: &str, seg: &[RequestEvent]| {
        let ids: Vec<String> = seg.iter().map(|r| r.request_id.clone()).collect();
        let start = seg.iter().map(|r| r.started_at_unix_ms).min();
        let finish = seg.iter().map(|r| r.finished_at_unix_ms).max();
        let mut analyzed = match (start, finish) {
            (Some(s), Some(f)) => {
                analyze_run_internal(&filtered_run_for_temporal_segment(run, &ids, s, f))
            }
            _ => analyze_run_internal(&route::filtered_run_for_route(run, &ids)),
        };
        let sparse_runtime =
            analyzed.evidence_quality.runtime_snapshots != SignalCoverageStatus::Present;
        let sparse_inflight =
            analyzed.evidence_quality.inflight_snapshots != SignalCoverageStatus::Present;
        if matches!(
            analyzed.primary_suspect.kind,
            DiagnosisKind::ExecutorPressureSuspected | DiagnosisKind::BlockingPoolPressure
        ) && (sparse_runtime || sparse_inflight)
        {
            analyzed
                .warnings
                .push(TEMPORAL_RUNTIME_ATTRIBUTION_WARNING.to_string());
        }
        TemporalSegment {
            name: name.to_string(),
            request_count: analyzed.request_count,
            started_at_unix_ms: start,
            finished_at_unix_ms: finish,
            p50_latency_us: analyzed.p50_latency_us,
            p95_latency_us: analyzed.p95_latency_us,
            p99_latency_us: analyzed.p99_latency_us,
            p95_queue_share_permille: analyzed.p95_queue_share_permille,
            p95_service_share_permille: analyzed.p95_service_share_permille,
            evidence_quality: analyzed.evidence_quality,
            primary_suspect: analyzed.primary_suspect,
            secondary_suspects: analyzed.secondary_suspects,
            warnings: analyzed.warnings,
        }
    };
    let mut early_seg = build("early", early);
    let mut late_seg = build("late", late);
    let suspect_shift_raw = early_seg.primary_suspect.kind != late_seg.primary_suspect.kind;
    let p95_shift = has_material_p95_shift(early_seg.p95_latency_us, late_seg.p95_latency_us);
    let queue_move = matches!((early_seg.p95_queue_share_permille, late_seg.p95_queue_share_permille), (Some(a), Some(b)) if a.abs_diff(b) >= TEMPORAL_SHARE_SHIFT_PERMILLE);
    let service_move = matches!((early_seg.p95_service_share_permille, late_seg.p95_service_share_permille), (Some(a), Some(b)) if a.abs_diff(b) >= TEMPORAL_SHARE_SHIFT_PERMILLE);
    let runtime_sparse = early_seg.evidence_quality.runtime_snapshots
        != SignalCoverageStatus::Present
        || early_seg.evidence_quality.inflight_snapshots != SignalCoverageStatus::Present
        || late_seg.evidence_quality.runtime_snapshots != SignalCoverageStatus::Present
        || late_seg.evidence_quality.inflight_snapshots != SignalCoverageStatus::Present;
    let runtime_dependent_shift = matches!(
        (
            &early_seg.primary_suspect.kind,
            &late_seg.primary_suspect.kind
        ),
        (
            DiagnosisKind::ExecutorPressureSuspected | DiagnosisKind::BlockingPoolPressure,
            _
        ) | (
            _,
            DiagnosisKind::ExecutorPressureSuspected | DiagnosisKind::BlockingPoolPressure
        )
    );
    let suspect_shift = suspect_shift_raw
        && (!runtime_sparse || !runtime_dependent_shift || p95_shift || queue_move || service_move);
    let material = suspect_shift || p95_shift || queue_move || service_move;
    if !material {
        return vec![];
    }
    if suspect_shift {
        global_warnings.push(TEMPORAL_SUSPECT_SHIFT_WARNING.to_string());
    }
    if p95_shift {
        global_warnings.push(TEMPORAL_P95_SHIFT_WARNING.to_string());
    }
    apply_temporal_overlap_attribution_warning(&mut early_seg, &mut late_seg);
    vec![early_seg, late_seg]
}

pub(super) fn apply_temporal_overlap_attribution_warning(
    early_seg: &mut TemporalSegment,
    late_seg: &mut TemporalSegment,
) {
    let windows_overlap = matches!(
        (
            early_seg.started_at_unix_ms,
            early_seg.finished_at_unix_ms,
            late_seg.started_at_unix_ms,
            late_seg.finished_at_unix_ms,
        ),
        (Some(_), Some(early_finish), Some(late_start), Some(_)) if early_finish >= late_start
    );
    let has_segment_runtime_or_inflight_samples = early_seg.evidence_quality.runtime_snapshot_count
        > 0
        || early_seg.evidence_quality.inflight_snapshot_count > 0
        || late_seg.evidence_quality.runtime_snapshot_count > 0
        || late_seg.evidence_quality.inflight_snapshot_count > 0;
    if windows_overlap && has_segment_runtime_or_inflight_samples {
        early_seg
            .warnings
            .push(TEMPORAL_OVERLAP_ATTRIBUTION_WARNING.to_string());
        late_seg
            .warnings
            .push(TEMPORAL_OVERLAP_ATTRIBUTION_WARNING.to_string());
    }
}

pub(super) fn has_material_p95_shift(left: Option<u64>, right: Option<u64>) -> bool {
    let (Some(a), Some(b)) = (left, right) else {
        return false;
    };
    let lower = a.min(b);
    let higher = a.max(b);
    if lower == 0 {
        return false;
    }
    higher.saturating_mul(2) >= lower.saturating_mul(3)
}
