use std::collections::{BTreeMap, BTreeSet, HashSet};

use tailtriage_core::Run;

use super::{
    analyze_run_internal, Report, RouteBreakdown, ROUTE_BREAKDOWN_LIMIT, ROUTE_MIN_REQUEST_COUNT,
    ROUTE_RUNTIME_ATTRIBUTION_WARNING,
};

pub(super) struct RouteBreakdownContext {
    pub(super) breakdowns: Vec<RouteBreakdown>,
    pub(super) divergent: bool,
}

pub(super) fn route_breakdowns(run: &Run, global: &Report) -> RouteBreakdownContext {
    let mut ids_by_route: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for request in &run.requests {
        ids_by_route
            .entry(request.route.clone())
            .or_default()
            .push(request.request_id.clone());
    }
    let eligible: Vec<(String, Vec<String>)> = ids_by_route
        .into_iter()
        .filter(|(_, ids)| ids.len() >= ROUTE_MIN_REQUEST_COUNT)
        .collect();
    if eligible.len() < 2 {
        return RouteBreakdownContext {
            breakdowns: vec![],
            divergent: false,
        };
    }

    let omitted_routes = run
        .requests
        .iter()
        .fold(BTreeMap::<String, usize>::new(), |mut acc, request| {
            *acc.entry(request.route.clone()).or_default() += 1;
            acc
        })
        .into_values()
        .filter(|count| *count < ROUTE_MIN_REQUEST_COUNT)
        .count();

    let mut candidates = Vec::new();
    for (route, request_ids) in eligible {
        let filtered = filtered_run_for_route(run, &request_ids);
        let mut analyzed = analyze_run_internal(&filtered);
        analyzed
            .warnings
            .push(ROUTE_RUNTIME_ATTRIBUTION_WARNING.to_string());
        candidates.push(RouteBreakdown {
            route,
            request_count: analyzed.request_count,
            p50_latency_us: analyzed.p50_latency_us,
            p95_latency_us: analyzed.p95_latency_us,
            p99_latency_us: analyzed.p99_latency_us,
            p95_queue_share_permille: analyzed.p95_queue_share_permille,
            p95_service_share_permille: analyzed.p95_service_share_permille,
            evidence_quality: analyzed.evidence_quality,
            primary_suspect: analyzed.primary_suspect,
            secondary_suspects: analyzed.secondary_suspects,
            warnings: analyzed.warnings,
        });
    }
    if !should_emit_route_breakdowns(global, &candidates) {
        return RouteBreakdownContext {
            breakdowns: vec![],
            divergent: false,
        };
    }
    let mut emitted = candidates;
    emitted.sort_by(|a, b| {
        b.p95_latency_us
            .cmp(&a.p95_latency_us)
            .then_with(|| b.request_count.cmp(&a.request_count))
            .then_with(|| a.route.cmp(&b.route))
    });
    emitted.truncate(ROUTE_BREAKDOWN_LIMIT);
    let divergent = route_divergence(&emitted);
    if omitted_routes > 0 {
        let note = format!(
            "Some routes are omitted from route_breakdowns because they have fewer than {ROUTE_MIN_REQUEST_COUNT} completed requests."
        );
        for breakdown in &mut emitted {
            breakdown.warnings.push(note.clone());
        }
    }
    RouteBreakdownContext {
        breakdowns: emitted,
        divergent,
    }
}

fn route_divergence(candidates: &[RouteBreakdown]) -> bool {
    candidates
        .iter()
        .map(|c| c.primary_suspect.kind.as_str())
        .collect::<BTreeSet<_>>()
        .len()
        >= 2
}

fn should_emit_route_breakdowns(global: &Report, candidates: &[RouteBreakdown]) -> bool {
    if candidates.len() < 2 {
        return false;
    }
    if route_divergence(candidates) {
        return true;
    }
    let p95s: Vec<u64> = candidates.iter().filter_map(|c| c.p95_latency_us).collect();
    if p95s.len() < 2 {
        return false;
    }
    let slowest = *p95s.iter().max().unwrap_or(&0);
    let fastest = *p95s.iter().min().unwrap_or(&0);
    (fastest > 0 && slowest.saturating_mul(2) >= fastest.saturating_mul(3))
        || match global.p95_latency_us {
            Some(global_p95) if global_p95 > 0 => {
                slowest.saturating_mul(4) >= global_p95.saturating_mul(5)
            }
            _ => false,
        }
}

pub(super) fn filtered_run_for_route(run: &Run, request_ids: &[String]) -> Run {
    let request_ids: HashSet<&str> = request_ids.iter().map(String::as_str).collect();
    let mut filtered = run.clone();
    filtered.requests = run
        .requests
        .iter()
        .filter(|r| request_ids.contains(r.request_id.as_str()))
        .cloned()
        .collect();
    filtered.stages = run
        .stages
        .iter()
        .filter(|s| request_ids.contains(s.request_id.as_str()))
        .cloned()
        .collect();
    filtered.queues = run
        .queues
        .iter()
        .filter(|q| request_ids.contains(q.request_id.as_str()))
        .cloned()
        .collect();
    filtered.runtime_snapshots = Vec::new();
    filtered.inflight = Vec::new();
    filtered
}
