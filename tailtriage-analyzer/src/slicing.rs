use std::collections::HashSet;

use tailtriage_core::Run;

use super::{
    analyze_run_internal, scoring::WorkerEvidenceStatus, AnalyzeOptions, EvidenceQuality, Report,
    RouteBreakdown, Suspect, TemporalSegment,
};

#[derive(Clone, Copy)]
pub(super) enum GlobalEvidencePolicy {
    Exclude,
    Window(SampleWindow),
}

#[derive(Clone, Copy)]
pub(super) struct SampleWindow {
    pub(super) unix_start: u64,
    pub(super) unix_finish: u64,
    pub(super) run_relative: Option<(u64, u64)>,
}

pub(super) struct ScopedAnalysis {
    pub(super) report: ScopedReportProjection,
    pub(super) used_unix_fallback: bool,
}

pub(super) struct ScopedReportProjection {
    pub(super) request_count: usize,
    pub(super) p50_latency_us: Option<u64>,
    pub(super) p95_latency_us: Option<u64>,
    pub(super) p99_latency_us: Option<u64>,
    pub(super) p95_queue_share_permille: Option<u64>,
    pub(super) p95_service_share_permille: Option<u64>,
    pub(super) evidence_quality: EvidenceQuality,
    pub(super) primary_suspect: Suspect,
    pub(super) secondary_suspects: Vec<Suspect>,
    pub(super) warnings: Vec<String>,
}

impl From<Report> for ScopedReportProjection {
    fn from(report: Report) -> Self {
        Self {
            request_count: report.request_count,
            p50_latency_us: report.p50_latency_us,
            p95_latency_us: report.p95_latency_us,
            p99_latency_us: report.p99_latency_us,
            p95_queue_share_permille: report.p95_queue_share_permille,
            p95_service_share_permille: report.p95_service_share_permille,
            evidence_quality: report.evidence_quality,
            primary_suspect: report.primary_suspect,
            secondary_suspects: report.secondary_suspects,
            warnings: report.warnings,
        }
    }
}

impl ScopedReportProjection {
    pub(super) fn into_route_breakdown(self, route: String) -> RouteBreakdown {
        RouteBreakdown {
            route,
            request_count: self.request_count,
            p50_latency_us: self.p50_latency_us,
            p95_latency_us: self.p95_latency_us,
            p99_latency_us: self.p99_latency_us,
            p95_queue_share_permille: self.p95_queue_share_permille,
            p95_service_share_permille: self.p95_service_share_permille,
            evidence_quality: self.evidence_quality,
            primary_suspect: self.primary_suspect,
            secondary_suspects: self.secondary_suspects,
            warnings: self.warnings,
        }
    }

    pub(super) fn into_temporal_segment(
        self,
        name: String,
        started_at_unix_ms: Option<u64>,
        finished_at_unix_ms: Option<u64>,
    ) -> TemporalSegment {
        TemporalSegment {
            name,
            request_count: self.request_count,
            started_at_unix_ms,
            finished_at_unix_ms,
            p50_latency_us: self.p50_latency_us,
            p95_latency_us: self.p95_latency_us,
            p99_latency_us: self.p99_latency_us,
            p95_queue_share_permille: self.p95_queue_share_permille,
            p95_service_share_permille: self.p95_service_share_permille,
            evidence_quality: self.evidence_quality,
            primary_suspect: self.primary_suspect,
            secondary_suspects: self.secondary_suspects,
            warnings: self.warnings,
        }
    }
}

pub(super) fn analyze_slice(
    source: &Run,
    request_ids: &[String],
    global_evidence: GlobalEvidencePolicy,
    worker_status: Option<WorkerEvidenceStatus>,
    options: &AnalyzeOptions,
) -> ScopedAnalysis {
    let sliced = slice_run(source, request_ids, global_evidence);
    ScopedAnalysis {
        report: analyze_run_internal(&sliced.run, worker_status, options).into(),
        used_unix_fallback: sliced.used_unix_fallback,
    }
}

struct SlicedRun {
    run: Run,
    used_unix_fallback: bool,
}

fn slice_run(
    source: &Run,
    request_ids: &[String],
    global_evidence: GlobalEvidencePolicy,
) -> SlicedRun {
    let request_ids: HashSet<&str> = request_ids.iter().map(String::as_str).collect();
    let mut run = source.clone();
    run.requests
        .retain(|request| request_ids.contains(request.request_id.as_str()));
    run.stages
        .retain(|stage| request_ids.contains(stage.request_id.as_str()));
    run.queues
        .retain(|queue| request_ids.contains(queue.request_id.as_str()));

    let used_unix_fallback = match global_evidence {
        GlobalEvidencePolicy::Exclude => {
            run.runtime_snapshots.clear();
            run.inflight.clear();
            false
        }
        GlobalEvidencePolicy::Window(window) => {
            let mut used_unix_fallback = false;
            run.runtime_snapshots.retain(|sample| {
                let (retained, used_fallback) =
                    retain_sample(sample.at_unix_ms, sample.at_run_us, window);
                used_unix_fallback |= used_fallback;
                retained
            });
            run.inflight.retain(|sample| {
                let (retained, used_fallback) =
                    retain_sample(sample.at_unix_ms, sample.at_run_us, window);
                used_unix_fallback |= used_fallback;
                retained
            });
            used_unix_fallback
        }
    };

    SlicedRun {
        run,
        used_unix_fallback,
    }
}

fn retain_sample(at_unix_ms: u64, at_run_us: Option<u64>, window: SampleWindow) -> (bool, bool) {
    if let (Some((start, finish)), Some(at)) = (window.run_relative, at_run_us) {
        return (at >= start && at <= finish, false);
    }

    let retained = at_unix_ms >= window.unix_start && at_unix_ms <= window.unix_finish;
    let used_unix_fallback = retained && window.run_relative.is_some() && at_run_us.is_none();
    (retained, used_unix_fallback)
}

#[cfg(test)]
mod tests {
    use super::{slice_run, GlobalEvidencePolicy, SampleWindow, ScopedReportProjection};
    use crate::{analyze_run_internal, AnalyzeOptions};
    use tailtriage_core::Run;

    fn fixture(contents: &str) -> Run {
        serde_json::from_str(contents).expect("fixture should deserialize")
    }

    #[test]
    fn shared_slicer_preserves_request_scoped_source_order() {
        let downstream = fixture(include_str!("../tests/fixtures/downstream_stage.json"));
        let queue = fixture(include_str!("../tests/fixtures/queue_saturation.json"));
        let mut source = downstream.clone();

        source.requests = (0..4)
            .map(|index| {
                let mut event = downstream.requests[index % downstream.requests.len()].clone();
                event.request_id = ["selected-b", "other-a", "selected-a", "other-b"][index].into();
                event
            })
            .collect();
        source.stages = (0..4)
            .map(|index| {
                let mut event = downstream.stages[index % downstream.stages.len()].clone();
                event.request_id = ["other-b", "selected-a", "other-a", "selected-b"][index].into();
                event
            })
            .collect();
        source.queues = (0..4)
            .map(|index| {
                let mut event = queue.queues[index % queue.queues.len()].clone();
                event.request_id = ["selected-a", "other-a", "selected-b", "other-b"][index].into();
                event
            })
            .collect();
        source.runtime_snapshots = queue.runtime_snapshots;
        source.inflight = queue.inflight;
        source.metadata.run_id = "non-default-slicing-run".into();
        source.metadata.lifecycle_warnings = vec!["preserve lifecycle warning".into()];
        source.truncation.dropped_requests = 7;

        let expected_metadata = source.metadata.clone();
        let expected_truncation = source.truncation.clone();
        let selected = vec!["selected-a".to_string(), "selected-b".to_string()];
        let sliced = slice_run(&source, &selected, GlobalEvidencePolicy::Exclude);

        assert_eq!(
            sliced
                .run
                .requests
                .iter()
                .map(|e| e.request_id.as_str())
                .collect::<Vec<_>>(),
            ["selected-b", "selected-a"]
        );
        assert_eq!(
            sliced
                .run
                .stages
                .iter()
                .map(|e| e.request_id.as_str())
                .collect::<Vec<_>>(),
            ["selected-a", "selected-b"]
        );
        assert_eq!(
            sliced
                .run
                .queues
                .iter()
                .map(|e| e.request_id.as_str())
                .collect::<Vec<_>>(),
            ["selected-a", "selected-b"]
        );
        assert!(sliced.run.runtime_snapshots.is_empty());
        assert!(sliced.run.inflight.is_empty());
        assert!(!sliced.used_unix_fallback);
        assert_eq!(sliced.run.metadata, expected_metadata);
        assert_eq!(sliced.run.truncation, expected_truncation);
        assert_eq!(sliced.run.schema_version, source.schema_version);
    }

    #[test]
    fn scoped_projection_matches_internal_report_fields_exactly() {
        let source = fixture(include_str!("../tests/fixtures/queue_saturation.json"));
        let mut report = analyze_run_internal(
            &source,
            crate::scoring::classify_worker_evidence(&source),
            &AnalyzeOptions::default(),
        );
        report.request_count = 37;
        report.p50_latency_us = Some(101);
        report.p95_latency_us = Some(202);
        report.p99_latency_us = Some(303);
        report.p95_queue_share_permille = Some(404);
        report.p95_service_share_permille = Some(505);
        report.evidence_quality.request_count = 41;
        report.evidence_quality.queue_event_count = 42;
        report.evidence_quality.stage_event_count = 43;
        report.evidence_quality.runtime_snapshot_count = 44;
        report.evidence_quality.inflight_snapshot_count = 45;
        report.evidence_quality.limitations = vec!["quality-a".into(), "quality-b".into()];
        report.primary_suspect.score = 71;
        report.primary_suspect.evidence = vec!["primary-a".into(), "primary-b".into()];
        let mut secondary_a = report.primary_suspect.clone();
        secondary_a.score = 31;
        secondary_a.evidence = vec!["secondary-a".into()];
        let mut secondary_b = report.primary_suspect.clone();
        secondary_b.score = 32;
        secondary_b.evidence = vec!["secondary-b".into()];
        report.secondary_suspects = vec![secondary_a, secondary_b];
        report.warnings = vec!["warning-a".into(), "warning-b".into()];

        let route = ScopedReportProjection::from(report.clone())
            .into_route_breakdown("route-identity".into());
        assert_eq!(route.route, "route-identity");
        assert_eq!(route.request_count, report.request_count);
        assert_eq!(route.p50_latency_us, report.p50_latency_us);
        assert_eq!(route.p95_latency_us, report.p95_latency_us);
        assert_eq!(route.p99_latency_us, report.p99_latency_us);
        assert_eq!(
            route.p95_queue_share_permille,
            report.p95_queue_share_permille
        );
        assert_eq!(
            route.p95_service_share_permille,
            report.p95_service_share_permille
        );
        assert_eq!(route.evidence_quality, report.evidence_quality);
        assert_eq!(route.primary_suspect, report.primary_suspect);
        assert_eq!(route.secondary_suspects, report.secondary_suspects);
        assert_eq!(route.warnings, report.warnings);

        let temporal = ScopedReportProjection::from(report.clone()).into_temporal_segment(
            "temporal-identity".into(),
            Some(601),
            Some(602),
        );
        assert_eq!(temporal.name, "temporal-identity");
        assert_eq!(temporal.started_at_unix_ms, Some(601));
        assert_eq!(temporal.finished_at_unix_ms, Some(602));
        assert_eq!(temporal.request_count, report.request_count);
        assert_eq!(temporal.p50_latency_us, report.p50_latency_us);
        assert_eq!(temporal.p95_latency_us, report.p95_latency_us);
        assert_eq!(temporal.p99_latency_us, report.p99_latency_us);
        assert_eq!(
            temporal.p95_queue_share_permille,
            report.p95_queue_share_permille
        );
        assert_eq!(
            temporal.p95_service_share_permille,
            report.p95_service_share_permille
        );
        assert_eq!(temporal.evidence_quality, report.evidence_quality);
        assert_eq!(temporal.primary_suspect, report.primary_suspect);
        assert_eq!(temporal.secondary_suspects, report.secondary_suspects);
        assert_eq!(temporal.warnings, report.warnings);

        let route_json = serde_json::to_value(route).unwrap();
        let temporal_json = serde_json::to_value(temporal).unwrap();
        assert!(route_json.get("route_breakdowns").is_none());
        assert!(route_json.get("temporal_segments").is_none());
        assert!(temporal_json.get("route_breakdowns").is_none());
        assert!(temporal_json.get("temporal_segments").is_none());
    }

    #[test]
    fn temporal_slice_filters_global_samples_with_existing_clock_rules() {
        let mut source = fixture(include_str!("../tests/fixtures/queue_saturation.json"));
        let blocking = fixture(include_str!("../tests/fixtures/blocking_pressure.json"));
        let runtime_template = blocking.runtime_snapshots[0].clone();
        let inflight_template = source.inflight[0].clone();
        let selected = source
            .requests
            .iter()
            .map(|r| r.request_id.clone())
            .collect::<Vec<_>>();

        source.runtime_snapshots = [(9, None), (90, Some(9)), (999, None)]
            .into_iter()
            .map(|(unix, run)| {
                let mut sample = runtime_template.clone();
                sample.at_unix_ms = unix;
                sample.at_run_us = run;
                sample
            })
            .collect();
        source.inflight = [(8, None), (80, Some(8))]
            .into_iter()
            .map(|(unix, run)| {
                let mut sample = inflight_template.clone();
                sample.at_unix_ms = unix;
                sample.at_run_us = run;
                sample
            })
            .collect();
        let negative = slice_run(
            &source,
            &selected,
            GlobalEvidencePolicy::Window(SampleWindow {
                unix_start: 100,
                unix_finish: 200,
                run_relative: Some((10, 20)),
            }),
        );
        assert!(negative.run.runtime_snapshots.is_empty());
        assert!(negative.run.inflight.is_empty());
        assert!(!negative.used_unix_fallback);

        source.runtime_snapshots = [
            (999, Some(9)),
            (999, Some(10)),
            (150, None),
            (0, Some(15)),
            (0, Some(20)),
            (150, Some(21)),
        ]
        .into_iter()
        .map(|(unix, run)| {
            let mut sample = runtime_template.clone();
            sample.at_unix_ms = unix;
            sample.at_run_us = run;
            sample
        })
        .collect();
        source.inflight = [
            (999, Some(9)),
            (999, Some(10)),
            (151, None),
            (0, Some(15)),
            (0, Some(20)),
            (150, Some(21)),
        ]
        .into_iter()
        .map(|(unix, run)| {
            let mut sample = inflight_template.clone();
            sample.at_unix_ms = unix;
            sample.at_run_us = run;
            sample
        })
        .collect();

        let sliced = slice_run(
            &source,
            &selected,
            GlobalEvidencePolicy::Window(SampleWindow {
                unix_start: 100,
                unix_finish: 200,
                run_relative: Some((10, 20)),
            }),
        );

        assert_eq!(
            sliced
                .run
                .runtime_snapshots
                .iter()
                .map(|s| (s.at_unix_ms, s.at_run_us))
                .collect::<Vec<_>>(),
            [(999, Some(10)), (150, None), (0, Some(15)), (0, Some(20))]
        );
        assert_eq!(
            sliced
                .run
                .inflight
                .iter()
                .map(|s| (s.at_unix_ms, s.at_run_us))
                .collect::<Vec<_>>(),
            [(999, Some(10)), (151, None), (0, Some(15)), (0, Some(20))]
        );
        assert!(sliced.used_unix_fallback);
    }
}
