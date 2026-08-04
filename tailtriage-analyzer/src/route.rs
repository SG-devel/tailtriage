use std::collections::{BTreeMap, BTreeSet};

use tailtriage_core::Run;

use super::{AnalyzeOptions, Report, RouteBreakdown, ROUTE_RUNTIME_ATTRIBUTION_WARNING};
use crate::ratio::meets_ratio;
use crate::slicing::{analyze_slice, GlobalEvidencePolicy};

pub(super) struct RouteBreakdownContext {
    pub(super) breakdowns: Vec<RouteBreakdown>,
    pub(super) warn_on_divergence: bool,
}

pub(super) fn route_breakdowns(
    run: &Run,
    global: &Report,
    options: &AnalyzeOptions,
) -> RouteBreakdownContext {
    let mut ids_by_route: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for request in &run.requests {
        ids_by_route
            .entry(request.route.clone())
            .or_default()
            .push(request.request_id.clone());
    }
    let eligible: Vec<(String, Vec<String>)> = ids_by_route
        .into_iter()
        .filter(|(_, ids)| ids.len() >= options.route.min_request_count)
        .collect();
    if eligible.len() < 2 {
        return RouteBreakdownContext {
            breakdowns: vec![],
            warn_on_divergence: false,
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
        .filter(|count| *count < options.route.min_request_count)
        .count();

    let mut candidates = Vec::new();
    for (route, request_ids) in eligible {
        let mut analyzed = analyze_slice(
            run,
            run,
            &request_ids,
            GlobalEvidencePolicy::Exclude,
            options,
        )
        .report;
        analyzed
            .warnings
            .push(ROUTE_RUNTIME_ATTRIBUTION_WARNING.to_string());
        candidates.push(analyzed.into_route_breakdown(route));
    }
    if !should_emit_route_breakdowns(global, &candidates, options) {
        return RouteBreakdownContext {
            breakdowns: vec![],
            warn_on_divergence: false,
        };
    }
    let mut emitted = candidates;
    emitted.sort_by(|a, b| {
        b.p95_latency_us
            .cmp(&a.p95_latency_us)
            .then_with(|| b.request_count.cmp(&a.request_count))
            .then_with(|| a.route.cmp(&b.route))
    });
    emitted.truncate(options.route.breakdown_limit);
    let warn_on_divergence = options.route.emit_on_divergent_suspects && route_divergence(&emitted);
    if omitted_routes > 0 {
        let min_count = options.route.min_request_count;
        let note = format!(
            "Some routes are omitted from route_breakdowns because they have fewer than {min_count} completed requests."
        );
        for breakdown in &mut emitted {
            breakdown.warnings.push(note.clone());
        }
    }
    RouteBreakdownContext {
        breakdowns: emitted,
        warn_on_divergence,
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

fn should_emit_route_breakdowns(
    global: &Report,
    candidates: &[RouteBreakdown],
    options: &AnalyzeOptions,
) -> bool {
    if candidates.len() < 2 {
        return false;
    }
    if route_divergence(candidates) && options.route.emit_on_divergent_suspects {
        return true;
    }
    let p95s: Vec<u64> = candidates.iter().filter_map(|c| c.p95_latency_us).collect();
    if p95s.len() < 2 {
        return false;
    }
    let slowest = *p95s.iter().max().unwrap_or(&0);
    let fastest = *p95s.iter().min().unwrap_or(&0);
    has_material_route_p95_ratio(slowest, fastest, global.p95_latency_us, options)
}

fn has_material_route_p95_ratio(
    slowest: u64,
    fastest: u64,
    global_p95: Option<u64>,
    options: &AnalyzeOptions,
) -> bool {
    meets_ratio(
        slowest,
        fastest,
        options.route.slowest_to_fastest_p95_ratio_numerator,
        options.route.slowest_to_fastest_p95_ratio_denominator,
    ) || match global_p95 {
        Some(global_p95) => meets_ratio(
            slowest,
            global_p95,
            options.route.slowest_to_global_p95_ratio_numerator,
            options.route.slowest_to_global_p95_ratio_denominator,
        ),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::has_material_route_p95_ratio;
    use crate::AnalyzeOptions;

    #[test]
    fn slowest_to_fastest_ratio_uses_its_boundary_and_baseline() {
        let mut options = AnalyzeOptions::default();
        options.route.emit_on_divergent_suspects = false;
        options.route.slowest_to_fastest_p95_ratio_numerator = 3;
        options.route.slowest_to_fastest_p95_ratio_denominator = 2;
        options.route.slowest_to_global_p95_ratio_numerator = u64::MAX;
        options.route.slowest_to_global_p95_ratio_denominator = 1;

        assert!(has_material_route_p95_ratio(30, 20, Some(1), &options));
        assert!(!has_material_route_p95_ratio(29, 20, Some(1), &options));
        assert!(has_material_route_p95_ratio(31, 20, Some(1), &options));
    }

    #[test]
    fn slowest_to_global_ratio_uses_its_boundary_and_baseline() {
        let mut options = AnalyzeOptions::default();
        options.route.emit_on_divergent_suspects = false;
        options.route.slowest_to_fastest_p95_ratio_numerator = u64::MAX;
        options.route.slowest_to_fastest_p95_ratio_denominator = 1;
        options.route.slowest_to_global_p95_ratio_numerator = 3;
        options.route.slowest_to_global_p95_ratio_denominator = 2;

        assert!(has_material_route_p95_ratio(30, 1, Some(20), &options));
        assert!(!has_material_route_p95_ratio(29, 1, Some(20), &options));
        assert!(has_material_route_p95_ratio(31, 1, Some(20), &options));
    }
}
