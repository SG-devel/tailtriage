use std::collections::BTreeMap;

use tailtriage_core::Run;

use crate::{
    percentile, request_time_shares, runtime_metric_series, DiagnosisKind, InflightTrend, Suspect,
    QUEUE_SHARE_TRIGGER_PERMILLE,
};

const DOWNSTREAM_MIN_STAGE_SAMPLES: usize = 3;
const SAMPLE_QUALITY_HIGH_SAMPLE_COUNT: usize = 100;
const SAMPLE_QUALITY_MEDIUM_SAMPLE_COUNT: usize = 40;
const SAMPLE_QUALITY_LOW_SAMPLE_COUNT: usize = 20;
const SAMPLE_QUALITY_MIN_NONZERO_SAMPLE_COUNT: usize = 8;

pub(super) fn queue_saturation_suspect(
    run: &Run,
    inflight_trend: Option<&InflightTrend>,
) -> Option<Suspect> {
    let (queue_shares, _) = request_time_shares(run);
    let p95_queue_share_permille = percentile(&queue_shares, 95, 100)?;
    if p95_queue_share_permille < QUEUE_SHARE_TRIGGER_PERMILLE {
        return None;
    }
    let queue_depths = run
        .queues
        .iter()
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
    let mut evidence = vec![format!(
        "Queue wait at p95 consumes {}.{}% of request time.",
        p95_queue_share_permille / 10,
        p95_queue_share_permille % 10
    )];
    if max_depth > 0 {
        evidence.push(format!("Observed queue depth sample up to {max_depth}."));
    }
    if let Some(trend) = inflight_trend.filter(|trend| trend.growth_delta > 0) {
        evidence.push(format!(
            "In-flight gauge '{}' grew by {} over the run window (p95={}, peak={}).",
            trend.gauge, trend.growth_delta, trend.p95_count, trend.peak_count
        ));
    }
    Some(Suspect::new(
        DiagnosisKind::ApplicationQueueSaturation,
        score,
        evidence,
        vec![
            "Inspect queue admission limits and producer burst patterns.".to_string(),
            "Compare queue wait distribution before and after increasing worker parallelism."
                .to_string(),
        ],
    ))
}

#[derive(Clone, Copy)]
struct BlockingSignal {
    p95: u64,
    peak: u64,
    nonzero: usize,
    samples: usize,
    nz_share_permille: u64,
}

fn blocking_signal(run: &Run) -> Option<BlockingSignal> {
    let depths = runtime_metric_series(&run.runtime_snapshots, |s| s.blocking_queue_depth);
    let p95 = percentile(&depths, 95, 100)?;
    let nonzero = nonzero_sample_count(&depths);
    if p95 == 0 && nonzero < 2 {
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

fn strong_blocking_signal(signal: BlockingSignal) -> bool {
    signal.p95 >= 12 && signal.peak >= 20 && signal.nz_share_permille >= 700 && signal.samples >= 30
}

pub(super) fn stage_correlates_with_blocking_pool(stage: &str) -> bool {
    let lower = stage.to_ascii_lowercase();
    lower.contains("spawn_blocking")
        || lower.contains("blocking_path")
        || lower.contains("blocking")
}

pub(super) fn blocking_pressure_suspect(run: &Run) -> Option<Suspect> {
    let signal = blocking_signal(run)?;
    let clean_extreme = signal.p95 >= 16 && signal.peak >= 24 && signal.nz_share_permille >= 900;
    let score = cap_unless_clean_evidence(
        32 + signal.p95.min(24)
            + (signal.peak.min(24) / 2)
            + (signal.nz_share_permille / 80)
            + u64::from(score_sample_quality(signal.samples)),
        clean_extreme,
        94,
    );
    Some(Suspect::new(
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
    ))
}

pub(super) fn executor_pressure_suspect(
    run: &Run,
    inflight_trend: Option<&InflightTrend>,
) -> Option<Suspect> {
    let global = runtime_metric_series(&run.runtime_snapshots, |s| s.global_queue_depth);
    let p95_global = percentile(&global, 95, 100)?;
    if p95_global == 0 {
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
    Some(Suspect::new(
        DiagnosisKind::ExecutorPressureSuspected,
        score,
        evidence,
        vec![
            "Check for long polls without yielding and uneven task fan-out.".to_string(),
            "Compare with per-stage timings to isolate overloaded async stages.".to_string(),
        ],
    ))
}

#[derive(Clone)]
struct StageCandidate {
    stage: String,
    samples: usize,
    p95: u64,
    cumulative: u64,
    cum_share: u64,
    tail_share: u64,
    score: u8,
}

fn downstream_stage_candidates(run: &Run, p95_req: u64, total_req: u64) -> Vec<StageCandidate> {
    let tail_ids: std::collections::HashMap<&str, u64> = run
        .requests
        .iter()
        .filter(|r| r.latency_us >= p95_req)
        .map(|r| (r.request_id.as_str(), r.latency_us))
        .collect();
    let tail_total = tail_ids.values().copied().fold(0_u64, u64::saturating_add);
    let mut by: BTreeMap<&str, Vec<&tailtriage_core::StageEvent>> = BTreeMap::new();
    for st in &run.stages {
        by.entry(st.stage.as_str()).or_default().push(st);
    }
    let mut cands = Vec::new();
    for (name, ss) in by {
        if ss.len() < DOWNSTREAM_MIN_STAGE_SAMPLES {
            continue;
        }
        let lats = ss.iter().map(|s| s.latency_us).collect::<Vec<_>>();
        let cum = lats.iter().copied().fold(0_u64, u64::saturating_add);
        let p95 = percentile(&lats, 95, 100).unwrap_or(0);
        let cum_share = cum.saturating_mul(1000).checked_div(total_req).unwrap_or(0);
        let tail_stage = ss
            .iter()
            .filter_map(|s| tail_ids.get(s.request_id.as_str()).map(|_| s.latency_us))
            .fold(0_u64, u64::saturating_add);
        let tail_share = if tail_total == 0 {
            0
        } else {
            tail_stage
                .saturating_mul(1000)
                .checked_div(tail_total)
                .unwrap_or(0)
        };
        let clean_extreme = tail_share >= 960 && cum_share >= 920 && ss.len() >= 20;
        let score = cap_unless_clean_evidence(
            score_from_permille(24, tail_share, 11)
                + (cum_share / 35)
                + u64::from(score_sample_quality(ss.len())),
            clean_extreme,
            95,
        );
        cands.push(StageCandidate {
            stage: name.to_string(),
            samples: ss.len(),
            p95,
            cumulative: cum,
            cum_share,
            tail_share,
            score,
        });
    }
    cands
}

pub(super) fn downstream_stage_suspect(run: &Run) -> Option<Suspect> {
    let p95_req = percentile(
        &run.requests
            .iter()
            .map(|r| r.latency_us)
            .collect::<Vec<_>>(),
        95,
        100,
    )?;
    let total_req = run
        .requests
        .iter()
        .map(|r| r.latency_us)
        .fold(0_u64, u64::saturating_add);
    let blocking = blocking_signal(run);
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
    let best = downstream_stage_candidates(run, p95_req, total_req)
        .into_iter()
        .max_by(|a, b| {
            a.score
                .cmp(&b.score)
                .then_with(|| a.tail_share.cmp(&b.tail_share))
                .then_with(|| a.cum_share.cmp(&b.cum_share))
                .then_with(|| b.stage.cmp(&a.stage))
        })?;
    let mut downstream_score = best.score;
    let mut correlation_evidence: Option<String> = None;
    if stage_correlates_with_blocking_pool(&best.stage)
        && blocking.is_some_and(strong_blocking_signal)
        && blocking_score.is_some()
    {
        let cap = blocking_score.unwrap_or(downstream_score).saturating_sub(2);
        downstream_score = downstream_score.min(cap);
        correlation_evidence = Some(format!(
            "Stage '{}' looks blocking-correlated; strong runtime blocking-queue evidence keeps blocking_pool_pressure prioritized.",
            best.stage
        ));
    }
    let mut evidence = vec![
        format!(
            "Stage '{}' has p95 latency {} us across {} samples.",
            best.stage, best.p95, best.samples
        ),
        format!(
            "Stage '{}' cumulative latency is {} us ({} permille of request latency).",
            best.stage, best.cumulative, best.cum_share
        ),
        format!(
            "Stage '{}' contributes {} permille of tail request latency.",
            best.stage, best.tail_share
        ),
    ];
    if let Some(extra) = correlation_evidence {
        evidence.push(extra);
    }
    Some(Suspect::new(
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
    ))
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
