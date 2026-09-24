use std::collections::HashSet;

use tailtriage_core::Run;

use crate::{
    candidate::SupportedCandidate,
    partial_evidence::{EvidenceBasis, PartialEvidenceProfile},
    percentile, runtime_metric_series, stage_attribution, AnalyzeOptions, DiagnosisKind,
    InflightCandidate, InflightOrdering, Suspect,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WorkerEvidenceStatus {
    HistoricalAbsent,
    Complete {
        worker_count: u32,
        local_complete: bool,
    },
    Partial,
    Inconsistent,
    InvalidZero,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ExecutorConfidenceLimitation {
    AmbiguousWorkers(WorkerEvidenceStatus),
    MissingLocalDepth,
}

#[derive(Debug, Clone, Copy)]
struct QueueMeasurement {
    basis: EvidenceBasis,
    p95_share_permille: u64,
    max_depth_at_start: u64,
    sample_count: usize,
    completed_p95_share_permille: Option<u64>,
    partial_event_count: usize,
    inflight_growth: bool,
}

#[derive(Debug, Clone, Copy)]
struct BlockingMeasurement {
    p95_queue_depth: u64,
    peak_queue_depth: u64,
    nonzero_sample_count: usize,
    usable_sample_count: usize,
    nonzero_share_permille: u64,
}

#[derive(Debug, Clone, Copy)]
struct ExecutorMeasurement {
    worker_status: WorkerEvidenceStatus,
    p95_global_queue_depth: u64,
    p95_local_queue_depth: Option<u64>,
    p95_alive_tasks: Option<u64>,
    global_sample_count: usize,
    normalized_p95_milli: Option<u64>,
    normalized_sample_count: usize,
    missing_local_lower_bound: bool,
    inflight_growth: bool,
}

#[derive(Debug, Clone)]
struct DownstreamMeasurement {
    basis: EvidenceBasis,
    stage: String,
    request_sample_count: usize,
    p95_attributed_latency_us: u64,
    cumulative_attributed_latency_us: u64,
    cumulative_share_permille: u64,
    tail_share_permille: u64,
    partial_event_count: usize,
}

pub(super) fn classify_worker_evidence(run: &Run) -> Option<WorkerEvidenceStatus> {
    let relevant = run
        .runtime_snapshots
        .iter()
        .filter(|snapshot| snapshot.global_queue_depth.is_some())
        .collect::<Vec<_>>();
    if relevant.is_empty() {
        return None;
    }
    if relevant
        .iter()
        .any(|snapshot| snapshot.worker_count == Some(0))
    {
        return Some(WorkerEvidenceStatus::InvalidZero);
    }
    let mut counts = relevant
        .iter()
        .filter_map(|snapshot| snapshot.worker_count)
        .collect::<Vec<_>>();
    counts.sort_unstable();
    counts.dedup();
    Some(match counts.as_slice() {
        [] => WorkerEvidenceStatus::HistoricalAbsent,
        [_] if relevant
            .iter()
            .any(|snapshot| snapshot.worker_count.is_none()) =>
        {
            WorkerEvidenceStatus::Partial
        }
        [worker_count] => WorkerEvidenceStatus::Complete {
            worker_count: *worker_count,
            local_complete: relevant
                .iter()
                .all(|snapshot| snapshot.local_queue_depth.is_some()),
        },
        _ => WorkerEvidenceStatus::Inconsistent,
    })
}

fn queue_per_worker_milli(global: u64, local: Option<u64>, workers: u32) -> u64 {
    let runnable = u128::from(global) + u128::from(local.unwrap_or(0));
    u64::try_from(((runnable * 1000) / u128::from(workers)).min(u128::from(u64::MAX)))
        .expect("normalized queue value is clamped to u64")
}

fn normalized_queue_contribution(p95: u64) -> u64 {
    match p95 {
        0..=499 => 0,
        500..=999 => 5,
        1000..=1999 => 15,
        2000..=3999 => 25,
        4000..=7999 => 40,
        _ => 55,
    }
}

fn suspect(
    kind: DiagnosisKind,
    score: u8,
    evidence: Vec<String>,
    next_checks: Vec<String>,
    options: &AnalyzeOptions,
) -> Suspect {
    Suspect {
        kind,
        score,
        confidence: crate::Confidence::from_score_with_options(score, options),
        evidence,
        next_checks,
        confidence_notes: Vec::new(),
    }
}

pub(super) fn queue_saturation_suspect(
    run: &Run,
    completed_queue_shares: &[u64],
    observed_queue_shares: &[u64],
    inflight_trend: Option<&InflightCandidate>,
    options: &AnalyzeOptions,
) -> Option<SupportedCandidate> {
    let completed_p95 = percentile(completed_queue_shares, 95, 100);
    let completed = queue_candidate(
        run,
        completed_queue_shares,
        true,
        completed_p95,
        inflight_trend,
        options,
    );
    let observed = queue_candidate(
        run,
        observed_queue_shares,
        false,
        completed_p95,
        inflight_trend,
        options,
    );
    QueueRepresentations {
        completed,
        observed_lower_bound: observed,
    }
    .select(options)
}

fn confidence_rank(confidence: crate::Confidence) -> u8 {
    match confidence {
        crate::Confidence::Low => 1,
        crate::Confidence::Medium => 2,
        crate::Confidence::High => 3,
    }
}

fn representation_confidence(
    score: u8,
    support: usize,
    basis: EvidenceBasis,
    options: &AnalyzeOptions,
) -> crate::Confidence {
    let mut confidence = crate::Confidence::from_score_with_options(score, options);
    confidence = confidence.min(crate::confidence::maturity_cap(support));
    if basis == EvidenceBasis::ObservedLowerBound {
        confidence = confidence.min(crate::Confidence::Medium);
    }
    confidence
}

fn representation_order(
    a: &SupportedCandidate,
    b: &SupportedCandidate,
    options: &AnalyzeOptions,
) -> std::cmp::Ordering {
    confidence_rank(representation_confidence(
        a.suspect.score,
        a.relevant_support,
        a.basis,
        options,
    ))
    .cmp(&confidence_rank(representation_confidence(
        b.suspect.score,
        b.relevant_support,
        b.basis,
        options,
    )))
    .then_with(|| a.relevant_support.cmp(&b.relevant_support))
    .then_with(|| a.suspect.score.cmp(&b.suspect.score))
    .then_with(|| (a.basis == EvidenceBasis::Completed).cmp(&(b.basis == EvidenceBasis::Completed)))
}

struct QueueRepresentations {
    completed: Option<SupportedCandidate>,
    observed_lower_bound: Option<SupportedCandidate>,
}

impl QueueRepresentations {
    fn select(self, options: &AnalyzeOptions) -> Option<SupportedCandidate> {
        match (self.completed, self.observed_lower_bound) {
            (Some(c), Some(o)) if representation_order(&o, &c, options).is_gt() => Some(o),
            (Some(c), _) => Some(c),
            (None, Some(o)) => Some(o),
            (None, None) => None,
        }
    }
}

fn queue_candidate(
    run: &Run,
    queue_shares: &[u64],
    completed_only: bool,
    completed_queue_p95_permille: Option<u64>,
    inflight_trend: Option<&InflightCandidate>,
    options: &AnalyzeOptions,
) -> Option<SupportedCandidate> {
    let measurement = eligible_queue_measurement(
        run,
        queue_shares,
        completed_only,
        completed_queue_p95_permille,
        inflight_trend,
        options,
    )?;
    Some(queue_magnitude(measurement, inflight_trend, options))
}

fn eligible_queue_measurement(
    run: &Run,
    queue_shares: &[u64],
    completed_only: bool,
    completed_queue_p95_permille: Option<u64>,
    inflight_trend: Option<&InflightCandidate>,
    options: &AnalyzeOptions,
) -> Option<QueueMeasurement> {
    let measurement = queue_measurement(
        run,
        queue_shares,
        completed_only,
        completed_queue_p95_permille,
        inflight_trend,
    )?;
    (measurement.p95_share_permille >= options.queueing.trigger_permille).then_some(measurement)
}

fn queue_magnitude(
    measurement: QueueMeasurement,
    inflight_trend: Option<&InflightCandidate>,
    options: &AnalyzeOptions,
) -> SupportedCandidate {
    let p95_queue_share_permille = measurement.p95_share_permille;
    let max_depth = measurement.max_depth_at_start;
    let growth_bonus = if measurement.inflight_growth { 5 } else { 0 };
    let depth_bonus = (max_depth.min(40) * 2) / 3;
    let base = score_from_permille(22, p95_queue_share_permille, 14);
    let clean_extreme = p95_queue_share_permille >= 985
        && max_depth >= 12
        && measurement.sample_count >= 20
        && measurement.inflight_growth;
    let score = cap_unless_clean_evidence(base + depth_bonus + growth_bonus, clean_extreme, 95);
    let mut evidence = if measurement.basis == EvidenceBasis::Completed {
        vec![format!(
            "Queue wait at p95 consumes {}.{}% of request time.",
            p95_queue_share_permille / 10,
            p95_queue_share_permille % 10
        )]
    } else {
        let mut e = Vec::new();
        if let Some(completed_p95) = measurement.completed_p95_share_permille {
            e.push(format!(
                "Completed-only queue wait at p95 is {}.{}% of request time.",
                completed_p95 / 10,
                completed_p95 % 10
            ));
        }
        e.push(format!(
            "Observed queue-wait lower bound at p95 is {}.{}% of request time and includes {} partial queue event(s).",
            p95_queue_share_permille / 10,
            p95_queue_share_permille % 10,
            measurement.partial_event_count
        ));
        e
    };
    if max_depth > 0 {
        evidence.push(format!("Observed queue depth sample up to {max_depth}."));
    }
    if let Some(trend) = inflight_trend.filter(|trend| trend.known_positive_growth()) {
        evidence.push(inflight_growth_evidence(trend));
    }
    SupportedCandidate {
        suspect: suspect(
            DiagnosisKind::ApplicationQueuePressure,
            score,
            evidence,
            vec![
                "Inspect queue admission limits and producer burst patterns.".to_string(),
                "Compare queue wait distribution before and after increasing worker parallelism."
                    .to_string(),
            ],
            options,
        ),
        basis: measurement.basis,
        executor_limitation: None,
        relevant_support: measurement.sample_count,
    }
}

fn queue_measurement(
    run: &Run,
    queue_shares: &[u64],
    completed_only: bool,
    completed_p95_share_permille: Option<u64>,
    inflight_trend: Option<&InflightCandidate>,
) -> Option<QueueMeasurement> {
    let profile = PartialEvidenceProfile::from_run(run);
    let depths = run
        .queues
        .iter()
        .filter(|queue| !completed_only || queue.completed)
        .filter_map(|queue| queue.depth_at_start)
        .collect::<Vec<_>>();
    Some(QueueMeasurement {
        basis: if completed_only {
            EvidenceBasis::Completed
        } else {
            EvidenceBasis::ObservedLowerBound
        },
        p95_share_permille: percentile(queue_shares, 95, 100)?,
        max_depth_at_start: max_or_zero(&depths),
        sample_count: {
            let completed_requests = run
                .requests
                .iter()
                .filter(|request| request.latency_us > 0)
                .map(|request| request.request_id.as_str())
                .collect::<HashSet<_>>();
            run.queues
                .iter()
                .filter(|queue| !completed_only || queue.completed)
                .filter(|queue| completed_requests.contains(queue.request_id.as_str()))
                .map(|queue| queue.request_id.as_str())
                .collect::<HashSet<_>>()
                .len()
        },
        completed_p95_share_permille,
        partial_event_count: profile.queues.partial,
        inflight_growth: inflight_trend.is_some_and(InflightCandidate::known_positive_growth),
    })
}

#[cfg(test)]
pub(super) fn queue_candidate_for_test(
    run: &Run,
    queue_shares: &[u64],
    completed_only: bool,
    completed_queue_p95_permille: Option<u64>,
    options: &AnalyzeOptions,
) -> Option<SupportedCandidate> {
    queue_candidate(
        run,
        queue_shares,
        completed_only,
        completed_queue_p95_permille,
        None,
        options,
    )
}

fn blocking_measurement(run: &Run) -> Option<BlockingMeasurement> {
    let depths = runtime_metric_series(&run.runtime_snapshots, |s| s.blocking_queue_depth);
    let p95 = percentile(&depths, 95, 100)?;
    let nonzero = nonzero_sample_count(&depths);
    let peak = max_or_zero(&depths);
    let nz_share_permille = if depths.is_empty() {
        0
    } else {
        nonzero as u64 * 1000 / depths.len() as u64
    };
    Some(BlockingMeasurement {
        p95_queue_depth: p95,
        peak_queue_depth: peak,
        nonzero_sample_count: nonzero,
        usable_sample_count: depths.len(),
        nonzero_share_permille: nz_share_permille,
    })
}

fn eligible_blocking_measurement(
    run: &Run,
    options: &AnalyzeOptions,
) -> Option<BlockingMeasurement> {
    let measurement = blocking_measurement(run)?;
    (measurement.p95_queue_depth > 0
        || measurement.nonzero_sample_count >= options.blocking.min_nonzero_samples_for_signal)
        .then_some(measurement)
}

fn strong_blocking_signal(signal: BlockingMeasurement, options: &AnalyzeOptions) -> bool {
    signal.p95_queue_depth >= options.blocking.strong_p95_threshold
        && signal.peak_queue_depth >= options.blocking.strong_peak_threshold
        && signal.nonzero_share_permille >= options.blocking.strong_nonzero_share_permille
        && signal.usable_sample_count >= options.blocking.strong_min_samples
}

pub(super) fn stage_correlates_with_blocking_pool(stage: &str, options: &AnalyzeOptions) -> bool {
    let lower = stage.to_ascii_lowercase();
    options
        .downstream
        .blocking_correlated_stage_patterns
        .iter()
        .any(|p| lower.contains(&p.trim().to_ascii_lowercase()))
}

pub(super) fn blocking_pressure_suspect(
    run: &Run,
    options: &AnalyzeOptions,
) -> Option<SupportedCandidate> {
    let signal = eligible_blocking_measurement(run, options)?;
    let clean_extreme = signal.p95_queue_depth >= 16
        && signal.peak_queue_depth >= 24
        && signal.nonzero_share_permille >= 900;
    let score = cap_unless_clean_evidence(
        32 + signal.p95_queue_depth.min(24)
            + (signal.peak_queue_depth.min(24) / 2)
            + (signal.nonzero_share_permille / 80),
        clean_extreme,
        94,
    );
    Some(SupportedCandidate {
        suspect: suspect(
            DiagnosisKind::BlockingPoolPressure,
            score,
            vec![format!(
                "Blocking queue depth p95 is {}, peak is {}, with {}/{} nonzero samples.",
                signal.p95_queue_depth,
                signal.peak_queue_depth,
                signal.nonzero_sample_count,
                signal.usable_sample_count
            )],
            vec![
                "Audit blocking sections and move avoidable synchronous work out of hot paths."
                    .to_string(),
                "Inspect spawn_blocking callsites for long-running CPU or I/O work.".to_string(),
            ],
            options,
        ),
        basis: EvidenceBasis::Completed,
        executor_limitation: None,
        relevant_support: signal.usable_sample_count,
    })
}

#[allow(clippy::too_many_lines)]
pub(super) fn executor_pressure_suspect(
    run: &Run,
    worker_status: Option<WorkerEvidenceStatus>,
    inflight_trend: Option<&InflightCandidate>,
    options: &AnalyzeOptions,
) -> Option<(Suspect, Option<ExecutorConfidenceLimitation>, usize)> {
    let measurement = eligible_executor_measurement(run, worker_status?, inflight_trend, options)?;
    let p95_global = measurement.p95_global_queue_depth;
    let growth_bonus = if measurement.inflight_growth { 4 } else { 0 };
    let legacy_score = || {
        let clean_extreme = p95_global >= 140 && measurement.global_sample_count >= 30;
        cap_unless_clean_evidence(
            34 + (p95_global.min(150) / 4)
                + (measurement.p95_local_queue_depth.unwrap_or(0).min(60) / 6)
                + (measurement.p95_alive_tasks.unwrap_or(0).min(400) / 40)
                + growth_bonus,
            clean_extreme,
            94,
        )
    };
    let mut evidence = vec![format!("Runtime global queue depth p95 is {p95_global}.")];
    if let Some(lp95) = measurement.p95_local_queue_depth {
        evidence.push(format!("Runtime local queue depth p95 is {lp95}."));
    }
    if let Some(ap95) = measurement.p95_alive_tasks {
        evidence.push(format!("Runtime alive_tasks p95 is {ap95}."));
    }
    if let Some(trend) = inflight_trend.filter(|trend| trend.known_positive_growth()) {
        evidence.push(inflight_growth_evidence(trend));
    }
    let (score, limitation) = match measurement.worker_status {
        WorkerEvidenceStatus::Complete { worker_count, .. } => {
            let p95 = measurement.normalized_p95_milli?;
            debug_assert!(
                p95 >= options
                    .executor
                    .min_runnable_queue_per_worker_p95_milli_for_signal
            );
            let contribution = normalized_queue_contribution(p95);
            evidence.push(format!(
                "Runnable queue depth p95 is {p95} milli-tasks per worker across {} samples with worker_count={worker_count}.",
                measurement.normalized_sample_count
            ));
            if measurement.missing_local_lower_bound {
                evidence.push("Runnable queue normalization is a lower bound because missing local queue depths were treated as zero.".to_string());
            }
            (
                clamp_score(34 + contribution + growth_bonus),
                measurement
                    .missing_local_lower_bound
                    .then_some(ExecutorConfidenceLimitation::MissingLocalDepth),
            )
        }
        WorkerEvidenceStatus::HistoricalAbsent => {
            debug_assert!(p95_global >= options.executor.min_global_queue_p95_for_signal);
            evidence[0] = format!(
                "Runtime global queue depth p95 is {p95_global}, suggesting scheduler contention."
            );
            evidence.push("Worker normalization was unavailable because this historical artifact has no worker-count evidence; legacy absolute-depth scoring was used.".to_string());
            (legacy_score(), None)
        }
        status @ (WorkerEvidenceStatus::Partial
        | WorkerEvidenceStatus::Inconsistent
        | WorkerEvidenceStatus::InvalidZero) => {
            debug_assert!(p95_global >= options.executor.min_global_queue_p95_for_signal);
            evidence[0] = format!(
                "Runtime global queue depth p95 is {p95_global}, suggesting scheduler contention."
            );
            evidence.push(format!("Worker-count evidence is {status:?}; legacy absolute-depth scoring was used without inferring a worker count."));
            (
                legacy_score(),
                Some(ExecutorConfidenceLimitation::AmbiguousWorkers(status)),
            )
        }
    };
    Some((
        suspect(
            DiagnosisKind::ExecutorPressure,
            score,
            evidence,
            vec![
                "Check for long polls without yielding and uneven task fan-out.".to_string(),
                "Compare with per-stage timings to isolate overloaded async stages.".to_string(),
            ],
            options,
        ),
        limitation,
        match measurement.worker_status {
            WorkerEvidenceStatus::Complete { .. } => measurement.normalized_sample_count,
            _ => measurement.global_sample_count,
        },
    ))
}

fn eligible_executor_measurement(
    run: &Run,
    worker_status: WorkerEvidenceStatus,
    inflight_trend: Option<&InflightCandidate>,
    options: &AnalyzeOptions,
) -> Option<ExecutorMeasurement> {
    let measurement = executor_measurement(run, worker_status, inflight_trend)?;
    let eligible = match measurement.worker_status {
        WorkerEvidenceStatus::Complete { .. } => {
            measurement.normalized_p95_milli.is_some_and(|p95| {
                p95 >= options
                    .executor
                    .min_runnable_queue_per_worker_p95_milli_for_signal
            })
        }
        WorkerEvidenceStatus::HistoricalAbsent
        | WorkerEvidenceStatus::Partial
        | WorkerEvidenceStatus::Inconsistent
        | WorkerEvidenceStatus::InvalidZero => {
            measurement.p95_global_queue_depth >= options.executor.min_global_queue_p95_for_signal
        }
    };
    eligible.then_some(measurement)
}

fn executor_measurement(
    run: &Run,
    worker_status: WorkerEvidenceStatus,
    inflight_trend: Option<&InflightCandidate>,
) -> Option<ExecutorMeasurement> {
    let global = runtime_metric_series(&run.runtime_snapshots, |s| s.global_queue_depth);
    let p95_global = percentile(&global, 95, 100)?;
    let local = runtime_metric_series(&run.runtime_snapshots, |s| s.local_queue_depth);
    let alive = runtime_metric_series(&run.runtime_snapshots, |s| s.alive_tasks);
    let (normalized_p95_milli, normalized_sample_count, missing_local_lower_bound) =
        match worker_status {
            WorkerEvidenceStatus::Complete {
                worker_count,
                local_complete,
            } => {
                let normalized = run
                    .runtime_snapshots
                    .iter()
                    .filter_map(|snapshot| {
                        Some(queue_per_worker_milli(
                            snapshot.global_queue_depth?,
                            snapshot.local_queue_depth,
                            worker_count,
                        ))
                    })
                    .collect::<Vec<_>>();
                (
                    percentile(&normalized, 95, 100),
                    normalized.len(),
                    !local_complete,
                )
            }
            _ => (None, 0, false),
        };
    Some(ExecutorMeasurement {
        worker_status,
        p95_global_queue_depth: p95_global,
        p95_local_queue_depth: percentile(&local, 95, 100),
        p95_alive_tasks: percentile(&alive, 95, 100),
        global_sample_count: global.len(),
        normalized_p95_milli,
        normalized_sample_count,
        missing_local_lower_bound,
        inflight_growth: inflight_trend.is_some_and(InflightCandidate::known_positive_growth),
    })
}

fn inflight_growth_evidence(candidate: &InflightCandidate) -> String {
    let trend = &candidate.trend;
    let growth_delta = trend
        .growth_delta
        .expect("in-flight growth evidence requires known positive growth");
    match (candidate.ordering, trend.growth_per_sec_milli) {
        (_, Some(rate)) => format!(
            "In-flight gauge '{}' latest active episode grew by {} across {} samples (p95={}, peak={}, run-relative rate={} milli-counts/sec).",
            trend.gauge, growth_delta, trend.sample_count, trend.p95_count, trend.peak_count, rate
        ),
        (InflightOrdering::UnixFallback, None) => format!(
            "In-flight gauge '{}' latest active episode grew by {} across {} samples (p95={}, peak={}); ordering used Unix-ms fallback and no precise growth rate was derived.",
            trend.gauge, growth_delta, trend.sample_count, trend.p95_count, trend.peak_count
        ),
        (InflightOrdering::RunRelative, None) => format!(
            "In-flight gauge '{}' latest active episode grew by {} across {} samples (p95={}, peak={}); precise run-relative growth rate is unavailable.",
            trend.gauge, growth_delta, trend.sample_count, trend.p95_count, trend.peak_count
        ),
    }
}

#[derive(Clone)]
struct StageCandidate {
    measurement: DownstreamMeasurement,
    score: u8,
}

struct DownstreamRepresentations(Vec<StageCandidate>);

impl DownstreamRepresentations {
    fn select(self, options: &AnalyzeOptions) -> Option<StageCandidate> {
        self.0.into_iter().max_by(|a, b| {
            confidence_rank(representation_confidence(
                a.score,
                a.measurement.request_sample_count,
                a.measurement.basis,
                options,
            ))
            .cmp(&confidence_rank(representation_confidence(
                b.score,
                b.measurement.request_sample_count,
                b.measurement.basis,
                options,
            )))
            .then_with(|| {
                a.measurement
                    .request_sample_count
                    .cmp(&b.measurement.request_sample_count)
            })
            .then_with(|| a.score.cmp(&b.score))
            .then_with(|| {
                (a.measurement.basis == EvidenceBasis::Completed)
                    .cmp(&(b.measurement.basis == EvidenceBasis::Completed))
            })
            .then_with(|| {
                a.measurement
                    .tail_share_permille
                    .cmp(&b.measurement.tail_share_permille)
            })
            .then_with(|| {
                a.measurement
                    .cumulative_share_permille
                    .cmp(&b.measurement.cumulative_share_permille)
            })
            .then_with(|| {
                (b.measurement.basis == EvidenceBasis::ObservedLowerBound)
                    .cmp(&(a.measurement.basis == EvidenceBasis::ObservedLowerBound))
            })
            .then_with(|| b.measurement.stage.cmp(&a.measurement.stage))
        })
    }
}

fn downstream_measurements(run: &Run, p95_req: u64) -> Vec<DownstreamMeasurement> {
    stage_attribution::dual_stage_summaries(run, p95_req)
        .into_iter()
        .map(|summary| DownstreamMeasurement {
            basis: summary.basis,
            stage: summary.stage,
            request_sample_count: summary.request_samples,
            p95_attributed_latency_us: summary.p95_attributed_latency_us,
            cumulative_attributed_latency_us: summary.cumulative_attributed_latency_us,
            cumulative_share_permille: summary.cumulative_share_permille,
            tail_share_permille: summary.tail_share_permille,
            partial_event_count: summary.partial_event_count,
        })
        .collect()
}

fn downstream_stage_candidates(
    run: &Run,
    p95_req: u64,
    options: &AnalyzeOptions,
) -> Vec<StageCandidate> {
    let mut cands = Vec::new();
    for measurement in downstream_measurements(run, p95_req) {
        let samples = measurement.request_sample_count;
        if samples < options.downstream.min_stage_samples || measurement.tail_share_permille < 300 {
            continue;
        }
        let clean_extreme = measurement.tail_share_permille >= 960
            && measurement.cumulative_share_permille >= 920
            && samples >= 20;
        let score = cap_unless_clean_evidence(
            score_from_permille(24, measurement.tail_share_permille, 11)
                + (measurement.cumulative_share_permille / 35),
            clean_extreme,
            95,
        );
        cands.push(StageCandidate { measurement, score });
    }
    cands
}

#[cfg(test)]
pub(super) type StageCandidateProjectionForTest =
    (EvidenceBasis, String, usize, u64, u64, u64, u64, u8);

#[cfg(test)]
pub(super) fn downstream_stage_candidates_for_test(
    run: &Run,
    p95_req: u64,
    options: &AnalyzeOptions,
) -> Vec<StageCandidateProjectionForTest> {
    downstream_stage_candidates(run, p95_req, options)
        .into_iter()
        .map(|c| {
            (
                c.measurement.basis,
                c.measurement.stage,
                c.measurement.request_sample_count,
                c.measurement.p95_attributed_latency_us,
                c.measurement.cumulative_attributed_latency_us,
                c.measurement.cumulative_share_permille,
                c.measurement.tail_share_permille,
                c.score,
            )
        })
        .collect()
}

pub(super) fn downstream_stage_suspect(
    run: &Run,
    options: &AnalyzeOptions,
) -> Option<SupportedCandidate> {
    let p95_req = percentile(
        &run.requests
            .iter()
            .map(|r| r.latency_us)
            .collect::<Vec<_>>(),
        95,
        100,
    )?;
    let blocking = eligible_blocking_measurement(run, options);
    let blocking_score = blocking.map(|signal| {
        let clean_extreme = signal.p95_queue_depth >= 16
            && signal.peak_queue_depth >= 24
            && signal.nonzero_share_permille >= 900;
        cap_unless_clean_evidence(
            32 + signal.p95_queue_depth.min(24)
                + (signal.peak_queue_depth.min(24) / 2)
                + (signal.nonzero_share_permille / 80),
            clean_extreme,
            94,
        )
    });
    let best = DownstreamRepresentations(downstream_stage_candidates(run, p95_req, options))
        .select(options)?;
    let (downstream_score, correlation_evidence) = apply_current_downstream_relation_policy(
        &best.measurement.stage,
        best.score,
        blocking,
        blocking_score,
        options,
    );
    let mut evidence = downstream_stage_evidence(&best);
    if let Some(extra) = correlation_evidence {
        evidence.push(extra);
    }
    Some(SupportedCandidate {
        suspect: suspect(
            DiagnosisKind::DownstreamStageDominance,
            downstream_score,
            evidence,
            vec![
                format!(
                    "Inspect downstream dependency behind stage '{}'.",
                    best.measurement.stage
                ),
                "Collect downstream service timings and retry behavior during tail windows."
                    .to_string(),
                "Review downstream SLO/error budget and align retry budget/backoff with it."
                    .to_string(),
            ],
            options,
        ),
        basis: best.measurement.basis,
        executor_limitation: None,
        relevant_support: best.measurement.request_sample_count,
    })
}

fn apply_current_downstream_relation_policy(
    stage: &str,
    downstream_score: u8,
    blocking: Option<BlockingMeasurement>,
    blocking_score: Option<u8>,
    options: &AnalyzeOptions,
) -> (u8, Option<String>) {
    if stage_correlates_with_blocking_pool(stage, options)
        && blocking.is_some_and(|signal| strong_blocking_signal(signal, options))
        && blocking_score.is_some()
    {
        let cap = blocking_score
            .unwrap_or(downstream_score)
            .saturating_sub(options.downstream.blocking_correlation_score_margin);
        return (
            downstream_score.min(cap),
            Some(format!(
                "Stage '{stage}' looks blocking-correlated; strong runtime blocking-queue evidence keeps blocking_pool_pressure prioritized."
            )),
        );
    }
    (downstream_score, None)
}

fn downstream_stage_evidence(best: &StageCandidate) -> Vec<String> {
    let measurement = &best.measurement;
    let mut evidence = if measurement.basis == EvidenceBasis::ObservedLowerBound {
        vec![format!(
            "Stage '{}' observed lower-bound p95 latency is {} us across {} samples and includes {} partial stage event(s).",
            measurement.stage,
            measurement.p95_attributed_latency_us,
            measurement.request_sample_count,
            measurement.partial_event_count
        )]
    } else {
        vec![format!(
            "Stage '{}' has p95 latency {} us across {} samples.",
            measurement.stage,
            measurement.p95_attributed_latency_us,
            measurement.request_sample_count
        )]
    };
    if measurement.basis == EvidenceBasis::ObservedLowerBound {
        evidence.extend(vec![
            format!(
                "Stage '{}' observed lower-bound cumulative latency is {} us ({} permille of request latency).",
                measurement.stage,
                measurement.cumulative_attributed_latency_us,
                measurement.cumulative_share_permille
            ),
            format!(
                "Stage '{}' observed lower-bound contribution is {} permille of tail request latency.",
                measurement.stage, measurement.tail_share_permille
            ),
        ]);
    } else {
        evidence.extend(vec![
            format!(
                "Stage '{}' cumulative latency is {} us ({} permille of request latency).",
                measurement.stage,
                measurement.cumulative_attributed_latency_us,
                measurement.cumulative_share_permille
            ),
            format!(
                "Stage '{}' contributes {} permille of tail request latency.",
                measurement.stage, measurement.tail_share_permille
            ),
        ]);
    }
    evidence
}

fn clamp_score(value: u64) -> u8 {
    u8::try_from(value.min(100)).unwrap_or(100)
}

fn nonzero_sample_count(values: &[u64]) -> usize {
    values.iter().filter(|&&v| v > 0).count()
}

fn max_or_zero(values: &[u64]) -> u64 {
    values.iter().copied().max().unwrap_or(0)
}

fn score_from_permille(base: u64, permille: u64, scale: u64) -> u64 {
    base + permille.min(1000) / scale
}

fn cap_unless_clean_evidence(score: u64, clean: bool, soft_cap: u8) -> u8 {
    if clean {
        clamp_score(score)
    } else {
        clamp_score(score.min(u64::from(soft_cap)))
    }
}

#[cfg(test)]
mod worker_normalized_tests {
    use super::*;

    fn run_with(counts: &[Option<u32>], locals: &[Option<u64>]) -> Run {
        let mut run: Run =
            serde_json::from_str(include_str!("../tests/fixtures/executor_pressure.json"))
                .expect("fixture");
        let template = run.runtime_snapshots[0].clone();
        run.runtime_snapshots = counts
            .iter()
            .enumerate()
            .map(|(index, count)| {
                let mut snapshot = template.clone();
                snapshot.global_queue_depth = Some(1);
                snapshot.worker_count = *count;
                snapshot.local_queue_depth = locals.get(index).copied().unwrap_or(Some(0));
                snapshot
            })
            .collect();
        run
    }

    // TT-TEST: support
    #[test]
    fn classifies_all_worker_evidence_statuses() {
        assert_eq!(
            classify_worker_evidence(&run_with(&[None, None], &[Some(0); 2])),
            Some(WorkerEvidenceStatus::HistoricalAbsent)
        );
        assert_eq!(
            classify_worker_evidence(&run_with(&[Some(4), Some(4)], &[Some(0), None])),
            Some(WorkerEvidenceStatus::Complete {
                worker_count: 4,
                local_complete: false
            })
        );
        assert_eq!(
            classify_worker_evidence(&run_with(&[Some(4), None], &[Some(0); 2])),
            Some(WorkerEvidenceStatus::Partial)
        );
        assert_eq!(
            classify_worker_evidence(&run_with(&[Some(2), Some(4)], &[Some(0); 2])),
            Some(WorkerEvidenceStatus::Inconsistent)
        );
        assert_eq!(
            classify_worker_evidence(&run_with(&[None, Some(0)], &[Some(0); 2])),
            Some(WorkerEvidenceStatus::InvalidZero)
        );
    }

    // TT-TEST: A02 primary
    #[test]
    fn normalized_contribution_boundaries_are_exact() {
        for (p95, expected) in [
            (499, 0),
            (500, 5),
            (999, 5),
            (1000, 15),
            (1999, 15),
            (2000, 25),
            (3999, 25),
            (4000, 40),
            (7999, 40),
            (8000, 55),
        ] {
            assert_eq!(normalized_queue_contribution(p95), expected, "p95={p95}");
        }
    }

    // TT-TEST: support
    #[test]
    fn normalization_floors_scales_and_clamps_with_u128_intermediates() {
        assert_eq!(queue_per_worker_milli(1, None, 3), 333);
        assert_eq!(queue_per_worker_milli(4, Some(4), 4), 2000);
        assert_eq!(queue_per_worker_milli(8, Some(8), 8), 2000);
        assert_eq!(
            queue_per_worker_milli(u64::MAX, Some(u64::MAX), 1),
            u64::MAX
        );
    }

    // TT-TEST: support
    #[test]
    fn percentile_combines_each_snapshots_global_and_local_depth_first() {
        let combined = [
            queue_per_worker_milli(10, Some(0), 2),
            queue_per_worker_milli(0, Some(10), 2),
        ];
        assert_eq!(percentile(&combined, 95, 100), Some(5000));
    }

    // TT-TEST: support
    #[test]
    fn normalized_pressure_is_monotonic_and_scale_invariant() {
        for workers in 1..=12 {
            let mut previous = 0;
            for runnable in 0..=64 {
                let current = queue_per_worker_milli(runnable, Some(0), workers);
                assert!(
                    current >= previous,
                    "runnable={runnable}, workers={workers}"
                );
                previous = current;
            }
        }

        for runnable in 0..=64 {
            let mut previous = u64::MAX;
            for workers in 1..=12 {
                let current = queue_per_worker_milli(runnable, Some(0), workers);
                assert!(
                    current <= previous,
                    "runnable={runnable}, workers={workers}"
                );
                previous = current;
            }
        }

        for runnable in 0..=32 {
            for workers in 1..=8 {
                for factor in 1..=4 {
                    assert_eq!(
                        queue_per_worker_milli(runnable, Some(0), workers),
                        queue_per_worker_milli(
                            runnable * u64::from(factor),
                            Some(0),
                            workers * factor,
                        ),
                        "runnable={runnable}, workers={workers}, factor={factor}"
                    );
                }
            }
        }
    }

    // TT-TEST: support
    #[test]
    fn normalized_contribution_is_monotonic() {
        let mut previous = 0;
        for p95 in 0..=10_000 {
            let current = normalized_queue_contribution(p95);
            assert!(current >= previous, "p95={p95}");
            previous = current;
        }
    }
}
