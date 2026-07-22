use std::collections::{BTreeMap, HashMap};

use tailtriage_core::Run;

use crate::{
    attribution::{attributed_elapsed_duration, AttributionInput},
    percentile,
};

#[derive(Debug, Clone, Copy)]
pub(super) struct RequestContext {
    pub(super) latency_us: u64,
    pub(super) is_tail: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct StageSummary {
    pub(super) stage: String,
    pub(super) request_samples: usize,
    pub(super) p95_attributed_latency_us: u64,
    pub(super) cumulative_attributed_latency_us: u64,
    pub(super) cumulative_share_permille: u64,
    pub(super) tail_attributed_latency_us: u64,
    pub(super) tail_share_permille: u64,
}

pub(super) fn request_contexts(
    run: &Run,
    p95_request_latency_us: u64,
) -> (HashMap<&str, RequestContext>, u64, u64) {
    let mut contexts = HashMap::with_capacity(run.requests.len());
    let mut total_request_latency_us = 0_u64;
    let mut tail_request_latency_us = 0_u64;

    for request in &run.requests {
        let is_tail = request.latency_us >= p95_request_latency_us;
        total_request_latency_us = total_request_latency_us.saturating_add(request.latency_us);
        if is_tail {
            tail_request_latency_us = tail_request_latency_us.saturating_add(request.latency_us);
        }
        contexts.insert(
            request.request_id.as_str(),
            RequestContext {
                latency_us: request.latency_us,
                is_tail,
            },
        );
    }

    (contexts, total_request_latency_us, tail_request_latency_us)
}

pub(super) fn stage_summaries(run: &Run, p95_request_latency_us: u64) -> Vec<StageSummary> {
    let (requests, total_request_latency_us, tail_request_latency_us) =
        request_contexts(run, p95_request_latency_us);
    let mut groups: BTreeMap<&str, HashMap<&str, Vec<AttributionInput>>> = BTreeMap::new();

    for stage in &run.stages {
        if requests.contains_key(stage.request_id.as_str()) {
            groups
                .entry(stage.stage.as_str())
                .or_default()
                .entry(stage.request_id.as_str())
                .or_default()
                .push(AttributionInput {
                    interval: stage.started_at_run_us.zip(stage.finished_at_run_us),
                    duration_us: stage.latency_us,
                });
        } else {
            debug_assert!(
                false,
                "normalized stage events should reference completed requests"
            );
        }
    }

    let mut summaries = Vec::with_capacity(groups.len());
    for (stage_name, by_request) in groups {
        let mut per_request = Vec::with_capacity(by_request.len());
        let mut cumulative = 0_u64;
        let mut tail_attributed = 0_u64;

        for (request_id, inputs) in by_request {
            let Some(context) = requests.get(request_id) else {
                debug_assert!(false, "stage group request must have context");
                continue;
            };
            let attributed = attributed_elapsed_duration(&inputs, context.latency_us);
            per_request.push(attributed.duration_us);
            cumulative = cumulative.saturating_add(attributed.duration_us);
            if context.is_tail {
                tail_attributed = tail_attributed.saturating_add(attributed.duration_us);
            }
        }

        let cumulative_share = cumulative
            .saturating_mul(1000)
            .checked_div(total_request_latency_us)
            .unwrap_or(0);
        let tail_share = tail_attributed
            .saturating_mul(1000)
            .checked_div(tail_request_latency_us)
            .unwrap_or(0);

        summaries.push(StageSummary {
            stage: stage_name.to_string(),
            request_samples: per_request.len(),
            p95_attributed_latency_us: percentile(&per_request, 95, 100).unwrap_or(0),
            cumulative_attributed_latency_us: cumulative,
            cumulative_share_permille: cumulative_share,
            tail_attributed_latency_us: tail_attributed,
            tail_share_permille: tail_share,
        });
    }

    summaries
}

#[cfg(test)]
mod tests {
    use tailtriage_core::{
        CaptureMode, EffectiveCoreConfig, RequestEvent, Run, RunMetadata, StageEvent,
        SCHEMA_VERSION,
    };

    use super::stage_summaries;

    fn run_with_requests(ids: &[&str], latency_us: u64) -> Run {
        Run {
            schema_version: SCHEMA_VERSION,
            metadata: RunMetadata {
                run_id: "run".into(),
                service_name: "svc".into(),
                service_version: None,
                started_at_unix_ms: 1,
                finalized_at_unix_ms: Some(2),
                mode: CaptureMode::Light,
                effective_core_config: Some(EffectiveCoreConfig {
                    mode: CaptureMode::Light,
                    capture_limits: CaptureMode::Light.core_defaults(),
                    strict_lifecycle: false,
                }),
                effective_tokio_sampler_config: None,
                host: None,
                pid: Some(1),
                lifecycle_warnings: Vec::new(),
                unfinished_requests: tailtriage_core::UnfinishedRequests::default(),
                run_end_reason: None,
            },
            requests: ids
                .iter()
                .map(|id| RequestEvent {
                    request_id: (*id).into(),
                    route: "/".into(),
                    kind: None,
                    started_at_unix_ms: 1,
                    started_at_run_us: Some(0),
                    finished_at_unix_ms: 2,
                    finished_at_run_us: Some(latency_us),
                    latency_us,
                    outcome: "ok".into(),
                })
                .collect(),
            stages: Vec::new(),
            queues: Vec::new(),
            inflight: Vec::new(),
            runtime_snapshots: Vec::new(),
            truncation: tailtriage_core::TruncationSummary::default(),
        }
    }

    fn stage(req: &str, name: &str, start: Option<u64>, end: Option<u64>, dur: u64) -> StageEvent {
        StageEvent {
            request_id: req.into(),
            stage: name.into(),
            started_at_unix_ms: 1,
            started_at_run_us: start,
            finished_at_unix_ms: 1,
            finished_at_run_us: end,
            latency_us: dur,
            success: true,
        }
    }

    #[test]
    fn overlapping_same_name_stages_use_per_request_union() {
        let mut run = run_with_requests(&["a", "b", "c"], 100);
        run.stages = vec![
            stage("a", "db", Some(0), Some(60), 60),
            stage("a", "db", Some(40), Some(90), 50),
            stage("b", "db", Some(0), Some(20), 20),
            stage("c", "db", Some(0), Some(20), 20),
        ];
        let summary = stage_summaries(&run, 100).remove(0);
        assert_eq!(summary.request_samples, 3);
        assert_eq!(summary.p95_attributed_latency_us, 90);
        assert_eq!(summary.cumulative_attributed_latency_us, 130);
        assert_eq!(summary.cumulative_share_permille, 433);
        assert_eq!(summary.tail_attributed_latency_us, 130);
        assert_eq!(summary.tail_share_permille, 433);
    }

    #[test]
    fn disjoint_repeated_same_name_stages_remain_additive_per_request() {
        let mut run = run_with_requests(&["a", "b", "c"], 100);
        run.stages = vec![
            stage("a", "db", Some(0), Some(20), 20),
            stage("a", "db", Some(40), Some(70), 30),
            stage("b", "db", Some(0), Some(20), 20),
            stage("c", "db", Some(0), Some(10), 10),
        ];
        let summary = stage_summaries(&run, 100).remove(0);
        assert_eq!(summary.request_samples, 3);
        assert_eq!(summary.p95_attributed_latency_us, 50);
        assert_eq!(summary.cumulative_attributed_latency_us, 80);
        assert_eq!(summary.cumulative_share_permille, 266);
        assert_eq!(summary.tail_share_permille, 266);
    }

    #[test]
    fn approximate_fallback_uses_all_events_in_one_request_stage_group() {
        let mut run = run_with_requests(&["a", "b", "c"], 100);
        run.stages = vec![
            stage("a", "db", Some(0), Some(20), 20),
            stage("a", "db", None, None, 90),
            stage("b", "db", Some(0), Some(10), 10),
            stage("c", "db", Some(0), Some(10), 10),
        ];
        let summary = stage_summaries(&run, 100).remove(0);
        assert_eq!(summary.request_samples, 3);
        assert_eq!(summary.p95_attributed_latency_us, 100);
        assert_eq!(summary.cumulative_attributed_latency_us, 120);
        assert_eq!(summary.cumulative_share_permille, 400);
        assert_eq!(summary.tail_share_permille, 400);
    }

    #[test]
    fn interleaved_stage_request_groups_are_independent_and_deterministic() {
        let mut run = run_with_requests(&["a", "b", "c"], 100);
        run.stages = vec![
            stage("a", "db", Some(0), Some(20), 20),
            stage("b", "cache", Some(0), Some(30), 30),
            stage("c", "db", Some(0), Some(10), 10),
            stage("a", "cache", Some(0), Some(50), 50),
            stage("b", "db", Some(40), Some(80), 40),
            stage("b", "db", Some(70), Some(90), 20),
            stage("c", "cache", Some(0), Some(20), 20),
        ];
        let summaries = stage_summaries(&run, 100);
        assert_eq!(
            summaries
                .iter()
                .map(|s| s.stage.as_str())
                .collect::<Vec<_>>(),
            vec!["cache", "db"]
        );
        let cache = &summaries[0];
        assert_eq!(cache.request_samples, 3);
        assert_eq!(cache.p95_attributed_latency_us, 50);
        assert_eq!(cache.cumulative_attributed_latency_us, 100);
        assert_eq!(cache.cumulative_share_permille, 333);
        let db = &summaries[1];
        assert_eq!(db.request_samples, 3);
        assert_eq!(db.p95_attributed_latency_us, 50);
        assert_eq!(db.cumulative_attributed_latency_us, 80);
        assert_eq!(db.cumulative_share_permille, 266);
    }

    #[test]
    fn different_stage_names_remain_independent_when_nested() {
        let mut run = run_with_requests(&["a", "b", "c"], 100);
        for id in ["a", "b", "c"] {
            run.stages.push(stage(id, "outer", Some(0), Some(80), 80));
            run.stages.push(stage(id, "inner", Some(20), Some(40), 20));
        }
        let summaries = stage_summaries(&run, 100);
        assert_eq!(summaries[0].stage, "inner");
        assert_eq!(summaries[0].p95_attributed_latency_us, 20);
        assert_eq!(summaries[0].cumulative_attributed_latency_us, 60);
        assert_eq!(summaries[1].stage, "outer");
        assert_eq!(summaries[1].p95_attributed_latency_us, 80);
        assert_eq!(summaries[1].cumulative_attributed_latency_us, 240);
    }

    #[test]
    fn coarse_unix_timestamps_do_not_drive_stage_attribution() {
        let mut run = run_with_requests(&["a", "b", "c"], 100);
        run.stages = vec![
            stage("a", "db", Some(0), Some(20), 20),
            stage("a", "db", Some(40), Some(70), 30),
            stage("b", "db", Some(0), Some(20), 20),
            stage("c", "db", Some(0), Some(10), 10),
        ];
        for stage in &mut run.stages {
            stage.started_at_unix_ms = 42;
            stage.finished_at_unix_ms = 42;
        }
        let summary = stage_summaries(&run, 100).remove(0);
        assert_eq!(summary.p95_attributed_latency_us, 50);
        assert_eq!(summary.cumulative_attributed_latency_us, 80);
    }
}
