use tailtriage_core::Run;

use crate::{
    partial_evidence::{EvidenceBasis, PartialEvidenceProfile, ScoredSuspect},
    percentile, runtime_metric_series, stage_attribution, AnalyzeOptions, DiagnosisKind,
    InflightTrend, Suspect,
};

const SAMPLE_QUALITY_HIGH_SAMPLE_COUNT: usize = 100;
const SAMPLE_QUALITY_MEDIUM_SAMPLE_COUNT: usize = 40;
const SAMPLE_QUALITY_LOW_SAMPLE_COUNT: usize = 20;
const SAMPLE_QUALITY_MIN_NONZERO_SAMPLE_COUNT: usize = 8;

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
    inflight_trend: Option<&InflightTrend>,
    options: &AnalyzeOptions,
) -> Option<ScoredSuspect> {
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
    match (completed, observed) {
        (Some(c), Some(o)) if o.suspect.score > c.suspect.score => Some(o),
        (Some(c), _) => Some(c),
        (None, Some(o)) => Some(o),
        (None, None) => None,
    }
}

fn queue_candidate(
    run: &Run,
    queue_shares: &[u64],
    completed_only: bool,
    completed_queue_p95_permille: Option<u64>,
    inflight_trend: Option<&InflightTrend>,
    options: &AnalyzeOptions,
) -> Option<ScoredSuspect> {
    let p95_queue_share_permille = percentile(queue_shares, 95, 100)?;
    if p95_queue_share_permille < options.queueing.trigger_permille {
        return None;
    }
    let queue_depths = run
        .queues
        .iter()
        .filter(|q| !completed_only || q.completed)
        .filter_map(|q| q.depth_at_start)
        .collect::<Vec<_>>();
    let max_depth = max_or_zero(&queue_depths);
    let growth_bonus = inflight_trend
        .filter(|t| t.growth_delta > 0)
        .map_or(0, |_| 5);
    let depth_bonus = (max_depth.min(40) * 2) / 3;
    let base = score_from_permille(22, p95_queue_share_permille, 14);
    let clean_extreme = p95_queue_share_permille >= 985
        && max_depth >= 12
        && queue_shares.len() >= 20
        && inflight_trend.is_some_and(|t| t.growth_delta > 0);
    let score = cap_unless_clean_evidence(
        base + depth_bonus + growth_bonus + u64::from(score_sample_quality(queue_shares.len())),
        clean_extreme,
        95,
    );
    let profile = PartialEvidenceProfile::from_run(run);
    let mut evidence = if completed_only {
        vec![format!(
            "Queue wait at p95 consumes {}.{}% of request time.",
            p95_queue_share_permille / 10,
            p95_queue_share_permille % 10
        )]
    } else {
        let mut e = Vec::new();
        if let Some(completed_p95) = completed_queue_p95_permille {
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
            profile.queues.partial
        ));
        e
    };
    if max_depth > 0 {
        evidence.push(format!("Observed queue depth sample up to {max_depth}."));
    }
    if let Some(trend) = inflight_trend.filter(|trend| trend.growth_delta > 0) {
        evidence.push(format!(
            "In-flight gauge '{}' grew by {} over the run window (p95={}, peak={}).",
            trend.gauge, trend.growth_delta, trend.p95_count, trend.peak_count
        ));
    }
    Some(ScoredSuspect {
        suspect: suspect(
            DiagnosisKind::ApplicationQueueSaturation,
            score,
            evidence,
            vec![
                "Inspect queue admission limits and producer burst patterns.".to_string(),
                "Compare queue wait distribution before and after increasing worker parallelism."
                    .to_string(),
            ],
            options,
        ),
        basis: if completed_only {
            EvidenceBasis::Completed
        } else {
            EvidenceBasis::ObservedLowerBound
        },
    })
}

#[cfg(test)]
pub(super) fn queue_candidate_for_test(
    run: &Run,
    queue_shares: &[u64],
    completed_only: bool,
    completed_queue_p95_permille: Option<u64>,
    options: &AnalyzeOptions,
) -> Option<ScoredSuspect> {
    queue_candidate(
        run,
        queue_shares,
        completed_only,
        completed_queue_p95_permille,
        None,
        options,
    )
}

#[derive(Clone, Copy)]
struct BlockingSignal {
    p95: u64,
    peak: u64,
    nonzero: usize,
    samples: usize,
    nz_share_permille: u64,
}

fn blocking_signal(run: &Run, options: &AnalyzeOptions) -> Option<BlockingSignal> {
    let depths = runtime_metric_series(&run.runtime_snapshots, |s| s.blocking_queue_depth);
    let p95 = percentile(&depths, 95, 100)?;
    let nonzero = nonzero_sample_count(&depths);
    if p95 == 0 && nonzero < options.blocking.min_nonzero_samples_for_signal {
        return None;
    }
    let peak = max_or_zero(&depths);
    let nz_share_permille = if depths.is_empty() {
        0
    } else {
        nonzero as u64 * 1000 / depths.len() as u64
    };
    Some(BlockingSignal {
        p95,
        peak,
        nonzero,
        samples: depths.len(),
        nz_share_permille,
    })
}

fn strong_blocking_signal(signal: BlockingSignal, options: &AnalyzeOptions) -> bool {
    signal.p95 >= options.blocking.strong_p95_threshold
        && signal.peak >= options.blocking.strong_peak_threshold
        && signal.nz_share_permille >= options.blocking.strong_nonzero_share_permille
        && signal.samples >= options.blocking.strong_min_samples
}

pub(super) fn stage_correlates_with_blocking_pool(stage: &str, options: &AnalyzeOptions) -> bool {
    let lower = stage.to_ascii_lowercase();
    options
        .downstream
        .blocking_correlated_stage_patterns
        .iter()
        .any(|p| lower.contains(&p.trim().to_ascii_lowercase()))
}

pub(super) fn blocking_pressure_suspect(run: &Run, options: &AnalyzeOptions) -> Option<Suspect> {
    let signal = blocking_signal(run, options)?;
    let clean_extreme = signal.p95 >= 16 && signal.peak >= 24 && signal.nz_share_permille >= 900;
    let score = cap_unless_clean_evidence(
        32 + signal.p95.min(24)
            + (signal.peak.min(24) / 2)
            + (signal.nz_share_permille / 80)
            + u64::from(score_sample_quality(signal.samples)),
        clean_extreme,
        94,
    );
    Some(suspect(
        DiagnosisKind::BlockingPoolPressure,
        score,
        vec![format!(
            "Blocking queue depth p95 is {}, peak is {}, with {}/{} nonzero samples.",
            signal.p95, signal.peak, signal.nonzero, signal.samples
        )],
        vec![
            "Audit blocking sections and move avoidable synchronous work out of hot paths."
                .to_string(),
            "Inspect spawn_blocking callsites for long-running CPU or I/O work.".to_string(),
        ],
        options,
    ))
}

pub(super) fn executor_pressure_suspect(
    run: &Run,
    inflight_trend: Option<&InflightTrend>,
    options: &AnalyzeOptions,
) -> Option<Suspect> {
    let global = runtime_metric_series(&run.runtime_snapshots, |s| s.global_queue_depth);
    let p95_global = percentile(&global, 95, 100)?;
    if p95_global < options.executor.min_global_queue_p95_for_signal {
        return None;
    }
    let local = runtime_metric_series(&run.runtime_snapshots, |s| s.local_queue_depth);
    let alive = runtime_metric_series(&run.runtime_snapshots, |s| s.alive_tasks);
    let growth_bonus = inflight_trend
        .filter(|t| t.growth_delta > 0)
        .map_or(0, |_| 4);
    let clean_extreme = p95_global >= 140 && global.len() >= 30;
    let score = cap_unless_clean_evidence(
        34 + (p95_global.min(150) / 4)
            + (percentile(&local, 95, 100).unwrap_or(0).min(60) / 6)
            + (percentile(&alive, 95, 100).unwrap_or(0).min(400) / 40)
            + growth_bonus
            + u64::from(score_sample_quality(global.len())),
        clean_extreme,
        94,
    );
    let mut evidence = vec![format!(
        "Runtime global queue depth p95 is {p95_global}, suggesting scheduler contention."
    )];
    if let Some(lp95) = percentile(&local, 95, 100) {
        evidence.push(format!("Runtime local queue depth p95 is {lp95}."));
    }
    if let Some(ap95) = percentile(&alive, 95, 100) {
        evidence.push(format!("Runtime alive_tasks p95 is {ap95}."));
    }
    Some(suspect(
        DiagnosisKind::ExecutorPressureSuspected,
        score,
        evidence,
        vec![
            "Check for long polls without yielding and uneven task fan-out.".to_string(),
            "Compare with per-stage timings to isolate overloaded async stages.".to_string(),
        ],
        options,
    ))
}

#[derive(Clone)]
struct StageCandidate {
    basis: EvidenceBasis,
    stage: String,
    samples: usize,
    p95: u64,
    cumulative: u64,
    cum_share: u64,
    tail_share: u64,
    partial_events: usize,
    score: u8,
}

fn downstream_stage_candidates(
    run: &Run,
    p95_req: u64,
    options: &AnalyzeOptions,
) -> Vec<StageCandidate> {
    let mut cands = Vec::new();
    for summary in stage_attribution::dual_stage_summaries(run, p95_req) {
        let samples = summary.request_samples;
        if samples < options.downstream.min_stage_samples {
            continue;
        }
        let clean_extreme = summary.tail_share_permille >= 960
            && summary.cumulative_share_permille >= 920
            && samples >= 20;
        let score = cap_unless_clean_evidence(
            score_from_permille(24, summary.tail_share_permille, 11)
                + (summary.cumulative_share_permille / 35)
                + u64::from(score_sample_quality(samples)),
            clean_extreme,
            95,
        );
        cands.push(StageCandidate {
            basis: summary.basis,
            stage: summary.stage,
            samples,
            p95: summary.p95_attributed_latency_us,
            cumulative: summary.cumulative_attributed_latency_us,
            cum_share: summary.cumulative_share_permille,
            tail_share: summary.tail_share_permille,
            partial_events: summary.partial_event_count,
            score,
        });
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
                c.basis,
                c.stage,
                c.samples,
                c.p95,
                c.cumulative,
                c.cum_share,
                c.tail_share,
                c.score,
            )
        })
        .collect()
}

pub(super) fn downstream_stage_suspect(
    run: &Run,
    options: &AnalyzeOptions,
) -> Option<ScoredSuspect> {
    let p95_req = percentile(
        &run.requests
            .iter()
            .map(|r| r.latency_us)
            .collect::<Vec<_>>(),
        95,
        100,
    )?;
    let blocking = blocking_signal(run, options);
    let blocking_score = blocking.map(|signal| {
        let clean_extreme =
            signal.p95 >= 16 && signal.peak >= 24 && signal.nz_share_permille >= 900;
        cap_unless_clean_evidence(
            32 + signal.p95.min(24)
                + (signal.peak.min(24) / 2)
                + (signal.nz_share_permille / 80)
                + u64::from(score_sample_quality(signal.samples)),
            clean_extreme,
            94,
        )
    });
    let best = downstream_stage_candidates(run, p95_req, options)
        .into_iter()
        .max_by(|a, b| {
            a.score
                .cmp(&b.score)
                .then_with(|| a.tail_share.cmp(&b.tail_share))
                .then_with(|| a.cum_share.cmp(&b.cum_share))
                .then_with(|| {
                    (b.basis == EvidenceBasis::ObservedLowerBound)
                        .cmp(&(a.basis == EvidenceBasis::ObservedLowerBound))
                })
                .then_with(|| b.stage.cmp(&a.stage))
        })?;
    let mut downstream_score = best.score;
    let mut correlation_evidence: Option<String> = None;
    if stage_correlates_with_blocking_pool(&best.stage, options)
        && blocking.is_some_and(|signal| strong_blocking_signal(signal, options))
        && blocking_score.is_some()
    {
        let cap = blocking_score
            .unwrap_or(downstream_score)
            .saturating_sub(options.downstream.blocking_correlation_score_margin);
        downstream_score = downstream_score.min(cap);
        correlation_evidence = Some(format!(
            "Stage '{}' looks blocking-correlated; strong runtime blocking-queue evidence keeps blocking_pool_pressure prioritized.",
            best.stage
        ));
    }
    let mut evidence = downstream_stage_evidence(&best);
    if let Some(extra) = correlation_evidence {
        evidence.push(extra);
    }
    Some(ScoredSuspect {
        suspect: suspect(
            DiagnosisKind::DownstreamStageDominates,
            downstream_score,
            evidence,
            vec![
                format!(
                    "Inspect downstream dependency behind stage '{}'.",
                    best.stage
                ),
                "Collect downstream service timings and retry behavior during tail windows."
                    .to_string(),
                "Review downstream SLO/error budget and align retry budget/backoff with it."
                    .to_string(),
            ],
            options,
        ),
        basis: best.basis,
    })
}

fn downstream_stage_evidence(best: &StageCandidate) -> Vec<String> {
    let mut evidence = if best.basis == EvidenceBasis::ObservedLowerBound {
        vec![format!(
            "Stage '{}' observed lower-bound p95 latency is {} us across {} samples and includes {} partial stage event(s).",
            best.stage, best.p95, best.samples, best.partial_events
        )]
    } else {
        vec![format!(
            "Stage '{}' has p95 latency {} us across {} samples.",
            best.stage, best.p95, best.samples
        )]
    };
    if best.basis == EvidenceBasis::ObservedLowerBound {
        evidence.extend(vec![
            format!(
                "Stage '{}' observed lower-bound cumulative latency is {} us ({} permille of request latency).",
                best.stage, best.cumulative, best.cum_share
            ),
            format!(
                "Stage '{}' observed lower-bound contribution is {} permille of tail request latency.",
                best.stage, best.tail_share
            ),
        ]);
    } else {
        evidence.extend(vec![
            format!(
                "Stage '{}' cumulative latency is {} us ({} permille of request latency).",
                best.stage, best.cumulative, best.cum_share
            ),
            format!(
                "Stage '{}' contributes {} permille of tail request latency.",
                best.stage, best.tail_share
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

fn score_sample_quality(sample_count: usize) -> u8 {
    if sample_count >= SAMPLE_QUALITY_HIGH_SAMPLE_COUNT {
        8
    } else if sample_count >= SAMPLE_QUALITY_MEDIUM_SAMPLE_COUNT {
        5
    } else if sample_count >= SAMPLE_QUALITY_LOW_SAMPLE_COUNT {
        3
    } else {
        u8::from(sample_count >= SAMPLE_QUALITY_MIN_NONZERO_SAMPLE_COUNT)
    }
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
