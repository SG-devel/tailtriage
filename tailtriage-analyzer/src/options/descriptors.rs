#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalyzeOptionDescriptor {
    pub path: &'static str,
    pub default_value: &'static str,
    pub value_type: &'static str,
    pub affects: &'static str,
    pub description: &'static str,
    pub increasing: Option<&'static str>,
    pub decreasing: Option<&'static str>,
}

#[must_use]
pub fn analyze_option_descriptors() -> Vec<AnalyzeOptionDescriptor> {
    vec![
        d("queueing.trigger_permille","300","u64","queue suspect trigger","Minimum p95 queue share (permille) before queueing suspect can trigger."),
        d("blocking.min_nonzero_samples_for_signal","2","usize","blocking signal eligibility","Minimum nonzero blocking-depth samples needed for blocking signal eligibility."),
        d("blocking.strong_p95_threshold","12","u64","blocking strong-signal classification","Blocking depth p95 threshold used when classifying stronger blocking signals."),
        d("blocking.strong_peak_threshold","20","u64","blocking strong-signal classification","Blocking depth peak threshold used when classifying stronger blocking signals."),
        d("blocking.strong_nonzero_share_permille","700","u64","blocking strong-signal classification","Minimum nonzero blocking-depth share (permille) for stronger blocking signal classification."),
        d("blocking.strong_min_samples","30","usize","blocking strong-signal classification","Minimum runtime sample count for stronger blocking signal classification."),
        d("executor.min_global_queue_p95_for_signal","1","u64","executor suspect trigger","Minimum global queue depth p95 needed before executor suspect can trigger."),
        d("downstream.min_stage_samples","3","usize","downstream-stage eligibility","Minimum stage sample count required before a stage can be considered for downstream dominance."),
        d("downstream.blocking_correlated_stage_patterns","[\"spawn_blocking\",\"blocking_path\",\"blocking\"]","Vec<String>","blocking/downstream tie-break context","Case-insensitive substrings used to recognize blocking-correlated stage names."),
        d("downstream.blocking_correlation_score_margin","2","u8","blocking/downstream tie-break context","Score margin used when comparing downstream and blocking-correlated evidence."),
        d("confidence.medium_score_threshold","65","u8","confidence bucket mapping","Score threshold for Medium confidence."),
        d("confidence.high_score_threshold","85","u8","confidence bucket mapping","Score threshold for High confidence."),
        d("confidence.ambiguity_min_score","60","u8","ambiguity warning gating","Minimum score for suspects to participate in ambiguity warnings."),
        d("confidence.ambiguity_score_gap","4","u8","ambiguity warning gating","Maximum top-score gap for ambiguity warnings."),
        d("evidence.low_completed_request_threshold","20","usize","low-evidence warning","Completed request count below which low-sample warnings are emitted."),
        d("route.min_request_count","3","usize","route-breakdown eligibility","Minimum completed requests per route to include route breakdowns."),
        d("route.breakdown_limit","10","usize","route-breakdown output sizing","Maximum number of emitted route breakdown entries."),
        d("route.emit_on_divergent_suspects","true","bool","route-breakdown emission","Emit route breakdowns when routes disagree on the primary suspect."),
        d("route.slowest_to_fastest_p95_ratio_numerator","3","u64","route-breakdown emission","Numerator for the slowest-vs-fastest route p95 ratio threshold."),
        d("route.slowest_to_fastest_p95_ratio_denominator","2","u64","route-breakdown emission","Denominator for the slowest-vs-fastest route p95 ratio threshold."),
        d("route.slowest_to_global_p95_ratio_numerator","5","u64","route-breakdown emission","Numerator for the slowest-route-vs-global p95 ratio threshold."),
        d("route.slowest_to_global_p95_ratio_denominator","4","u64","route-breakdown emission","Denominator for the slowest-route-vs-global p95 ratio threshold."),
        d("temporal.min_request_count","20","usize","temporal eligibility","Minimum run request count required before temporal segmentation is attempted."),
        d("temporal.min_segment_request_count","8","usize","temporal eligibility","Minimum request count required in each temporal segment."),
        d("temporal.share_shift_permille","200","u64","temporal movement detection","Minimum queue/service share movement (permille) considered material across segments."),
        d("temporal.p95_shift_ratio_numerator","3","u64","temporal movement detection","Numerator for material p95 shift ratio across segments."),
        d("temporal.p95_shift_ratio_denominator","2","u64","temporal movement detection","Denominator for material p95 shift ratio across segments."),
        d("temporal.emit_on_suspect_shift","true","bool","temporal emission","Emit temporal segments when early and late primary suspects differ."),
        d("temporal.suppress_runtime_sparse_suspect_shift_without_supporting_movement","true","bool","temporal suppression behavior","Suppress runtime-dependent suspect shifts when runtime evidence is sparse and no supporting movement is present."),
    ]
}

fn d(
    path: &'static str,
    default_value: &'static str,
    value_type: &'static str,
    affects: &'static str,
    description: &'static str,
) -> AnalyzeOptionDescriptor {
    AnalyzeOptionDescriptor {
        path,
        default_value,
        value_type,
        affects,
        description,
        increasing: None,
        decreasing: None,
    }
}
