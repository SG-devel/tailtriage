use tailtriage_core::{RequestEvent, Run};

use super::{
    analyze_run_internal, AnalyzeOptions, DiagnosisKind, SignalCoverageStatus, TemporalSegment,
};
use crate::route;

const TEMPORAL_RUNTIME_ATTRIBUTION_WARNING: &str = "Runtime and in-flight evidence is sparse in this segment after timestamp filtering; executor/blocking attribution is limited.";
pub(super) const TEMPORAL_SUSPECT_SHIFT_WARNING: &str = "Temporal segments show different primary suspects; inspect temporal_segments before acting on the global suspect.";
pub(super) const TEMPORAL_P95_SHIFT_WARNING: &str =
    "Temporal segments show a large p95 latency shift between early and late requests.";
pub(super) const TEMPORAL_OVERLAP_ATTRIBUTION_WARNING: &str = "Segment windows overlap under concurrent requests; timestamp-filtered runtime/in-flight attribution is approximate.";
pub(super) const TEMPORAL_WALL_CLOCK_FALLBACK_WARNING: &str = "Temporal segment used wall-clock timestamp fallback; attribution is approximate for artifacts without complete run-relative timing.";

#[derive(Debug, Clone, Copy)]
enum SegmentWindow {
    RunRelative { start: u64, finish: u64 },
    Unix { start: u64, finish: u64 },
}

fn request_temporal_sort_key(request: &RequestEvent) -> (u64, &str) {
    (
        request
            .started_at_run_us
            .unwrap_or(request.started_at_unix_ms),
        request.request_id.as_str(),
    )
}

fn segment_run_relative_window(segment: &[RequestEvent]) -> Option<SegmentWindow> {
    if !segment
        .iter()
        .all(|request| request.started_at_run_us.is_some() && request.finished_at_run_us.is_some())
    {
        return None;
    }

    let start = segment
        .iter()
        .filter_map(|request| request.started_at_run_us)
        .min()?;
    let finish = segment
        .iter()
        .filter_map(|request| request.finished_at_run_us)
        .max()?;
    Some(SegmentWindow::RunRelative { start, finish })
}

fn segment_unix_window(segment: &[RequestEvent]) -> Option<SegmentWindow> {
    let start = segment
        .iter()
        .map(|request| request.started_at_unix_ms)
        .min()?;
    let finish = segment
        .iter()
        .map(|request| request.finished_at_unix_ms)
        .max()?;
    Some(SegmentWindow::Unix { start, finish })
}

fn filter_segment_runtime_and_inflight(
    run: &Run,
    request_ids: &[String],
    window: SegmentWindow,
) -> Run {
    let mut filtered = route::filtered_run_for_route(run, request_ids);
    match window {
        SegmentWindow::RunRelative { start, finish } => {
            filtered.runtime_snapshots = run
                .runtime_snapshots
                .iter()
                .filter(|snapshot| {
                    snapshot
                        .at_run_us
                        .is_some_and(|at_run_us| at_run_us >= start && at_run_us <= finish)
                })
                .cloned()
                .collect();
            filtered.inflight = run
                .inflight
                .iter()
                .filter(|snapshot| {
                    snapshot
                        .at_run_us
                        .is_some_and(|at_run_us| at_run_us >= start && at_run_us <= finish)
                })
                .cloned()
                .collect();
        }
        SegmentWindow::Unix { start, finish } => {
            filtered.runtime_snapshots = run
                .runtime_snapshots
                .iter()
                .filter(|snapshot| snapshot.at_unix_ms >= start && snapshot.at_unix_ms <= finish)
                .cloned()
                .collect();
            filtered.inflight = run
                .inflight
                .iter()
                .filter(|snapshot| snapshot.at_unix_ms >= start && snapshot.at_unix_ms <= finish)
                .cloned()
                .collect();
        }
    }
    filtered
}

pub(super) fn temporal_segments(
    run: &Run,
    global_warnings: &mut Vec<String>,
    options: &AnalyzeOptions,
) -> Vec<TemporalSegment> {
    if run.requests.len() < options.temporal.min_request_count {
        return vec![];
    }
    let mut requests = run.requests.clone();
    requests.sort_by(|a, b| request_temporal_sort_key(a).cmp(&request_temporal_sort_key(b)));
    let split = requests.len() / 2;
    let (early, late) = requests.split_at(split);
    if early.len() < options.temporal.min_segment_request_count
        || late.len() < options.temporal.min_segment_request_count
    {
        return vec![];
    }

    let mut early_seg = build_temporal_segment("early", early, run, options);
    let mut late_seg = build_temporal_segment("late", late, run, options);
    let p95_shift =
        has_material_p95_shift(early_seg.p95_latency_us, late_seg.p95_latency_us, options);
    let queue_move = has_material_share_shift(
        early_seg.p95_queue_share_permille,
        late_seg.p95_queue_share_permille,
        options,
    );
    let service_move = has_material_share_shift(
        early_seg.p95_service_share_permille,
        late_seg.p95_service_share_permille,
        options,
    );
    let movement = TemporalMovement {
        p95_shift,
        queue_move,
        service_move,
    };
    let suspect_shift = has_material_suspect_shift(&early_seg, &late_seg, movement, options);
    let material = has_material_temporal_signal(suspect_shift, movement, options);
    if !material {
        return vec![];
    }
    if options.temporal.emit_on_suspect_shift && suspect_shift {
        global_warnings.push(TEMPORAL_SUSPECT_SHIFT_WARNING.to_string());
    }
    if p95_shift {
        global_warnings.push(TEMPORAL_P95_SHIFT_WARNING.to_string());
    }
    apply_temporal_overlap_attribution_warning(&mut early_seg, &mut late_seg);
    vec![early_seg, late_seg]
}

fn build_temporal_segment(
    name: &str,
    segment: &[RequestEvent],
    run: &Run,
    options: &AnalyzeOptions,
) -> TemporalSegment {
    let ids: Vec<String> = segment
        .iter()
        .map(|request| request.request_id.clone())
        .collect();
    let report_window = segment_unix_window(segment);
    let filter_window = segment_run_relative_window(segment).or(report_window);
    let used_wall_clock_fallback =
        !matches!(filter_window, Some(SegmentWindow::RunRelative { .. }));
    let mut analyzed = match filter_window {
        Some(window) => analyze_run_internal(
            &filter_segment_runtime_and_inflight(run, &ids, window),
            options,
        ),
        None => analyze_run_internal(&route::filtered_run_for_route(run, &ids), options),
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
    if used_wall_clock_fallback {
        analyzed
            .warnings
            .push(TEMPORAL_WALL_CLOCK_FALLBACK_WARNING.to_string());
    }
    let (start, finish) = match report_window {
        Some(SegmentWindow::Unix { start, finish }) => (Some(start), Some(finish)),
        Some(SegmentWindow::RunRelative { .. }) | None => (None, None),
    };
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
}

fn has_material_share_shift(
    left: Option<u64>,
    right: Option<u64>,
    options: &AnalyzeOptions,
) -> bool {
    matches!((left, right), (Some(a), Some(b)) if a.abs_diff(b) >= options.temporal.share_shift_permille)
}

fn has_runtime_sparse_temporal_evidence(early: &TemporalSegment, late: &TemporalSegment) -> bool {
    early.evidence_quality.runtime_snapshots != SignalCoverageStatus::Present
        || early.evidence_quality.inflight_snapshots != SignalCoverageStatus::Present
        || late.evidence_quality.runtime_snapshots != SignalCoverageStatus::Present
        || late.evidence_quality.inflight_snapshots != SignalCoverageStatus::Present
}

#[derive(Clone, Copy)]
struct TemporalMovement {
    p95_shift: bool,
    queue_move: bool,
    service_move: bool,
}

fn is_runtime_dependent_suspect_shift(early: &TemporalSegment, late: &TemporalSegment) -> bool {
    matches!(
        (&early.primary_suspect.kind, &late.primary_suspect.kind),
        (
            DiagnosisKind::ExecutorPressureSuspected | DiagnosisKind::BlockingPoolPressure,
            _
        ) | (
            _,
            DiagnosisKind::ExecutorPressureSuspected | DiagnosisKind::BlockingPoolPressure
        )
    )
}

fn has_material_suspect_shift(
    early: &TemporalSegment,
    late: &TemporalSegment,
    movement: TemporalMovement,
    options: &AnalyzeOptions,
) -> bool {
    let suspect_shift_raw = early.primary_suspect.kind != late.primary_suspect.kind;
    let runtime_sparse = has_runtime_sparse_temporal_evidence(early, late);
    let runtime_dependent_shift = is_runtime_dependent_suspect_shift(early, late);
    suspect_shift_raw
        && (!options
            .temporal
            .suppress_runtime_sparse_suspect_shift_without_supporting_movement
            || !runtime_sparse
            || !runtime_dependent_shift
            || movement.p95_shift
            || movement.queue_move
            || movement.service_move)
}

fn has_material_temporal_signal(
    suspect_shift: bool,
    movement: TemporalMovement,
    options: &AnalyzeOptions,
) -> bool {
    (options.temporal.emit_on_suspect_shift && suspect_shift)
        || movement.p95_shift
        || movement.queue_move
        || movement.service_move
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

pub(super) fn has_material_p95_shift(
    left: Option<u64>,
    right: Option<u64>,
    options: &AnalyzeOptions,
) -> bool {
    let (Some(a), Some(b)) = (left, right) else {
        return false;
    };
    let lower = a.min(b);
    let higher = a.max(b);
    if lower == 0 {
        return false;
    }
    higher.saturating_mul(options.temporal.p95_shift_ratio_denominator)
        >= lower.saturating_mul(options.temporal.p95_shift_ratio_numerator)
}
