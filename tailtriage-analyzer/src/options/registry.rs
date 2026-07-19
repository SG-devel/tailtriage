use super::{AnalyzeConfigError, AnalyzeOptionDescriptor, AnalyzeOptions};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OptionValue {
    U64(u64),
    Usize(usize),
    U8(u8),
    Bool(bool),
    StringList(Vec<String>),
}

impl OptionValue {
    pub(crate) fn summary_value(&self) -> String {
        match self {
            Self::U64(v) => v.to_string(),
            Self::Usize(v) => v.to_string(),
            Self::U8(v) => v.to_string(),
            Self::Bool(v) => v.to_string(),
            Self::StringList(v) => v.join(","),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ValueKind {
    U64,
    Usize,
    U8,
    Bool,
    StringList,
}

impl ValueKind {
    pub(crate) const fn type_name(self) -> &'static str {
        match self {
            Self::U64 => "u64",
            Self::Usize => "usize",
            Self::U8 => "u8",
            Self::Bool => "bool",
            Self::StringList => "Vec<String>",
        }
    }

    const fn expected(self) -> &'static str {
        match self {
            Self::U64 => "base-10 unsigned integer (u64)",
            Self::Usize => "base-10 unsigned integer (usize)",
            Self::U8 => "base-10 unsigned integer in range 0..=255 (u8)",
            Self::Bool => "'true' or 'false'",
            Self::StringList => "comma-separated non-empty entries (Vec<String>)",
        }
    }

    fn parse(self, path: &'static str, value: &str) -> Result<OptionValue, AnalyzeConfigError> {
        match self {
            Self::U64 => parse_num(path, value, self.expected()).map(OptionValue::U64),
            Self::Usize => parse_num(path, value, self.expected()).map(OptionValue::Usize),
            Self::U8 => parse_num(path, value, self.expected()).map(OptionValue::U8),
            Self::Bool => match value {
                "true" => Ok(OptionValue::Bool(true)),
                "false" => Ok(OptionValue::Bool(false)),
                _ => Err(invalid_value(path, value, self.expected())),
            },
            Self::StringList => {
                let mut out = Vec::new();
                for entry in value.split(',') {
                    let trimmed = entry.trim();
                    if trimmed.is_empty() {
                        return Err(invalid_value(path, value, self.expected()));
                    }
                    out.push(trimmed.to_string());
                }
                Ok(OptionValue::StringList(out))
            }
        }
    }
}

pub(crate) struct OptionSpec {
    pub(crate) path: &'static str,
    pub(crate) category: &'static str,
    pub(crate) kind: ValueKind,
    pub(crate) get: fn(&AnalyzeOptions) -> OptionValue,
    pub(crate) set: fn(&mut AnalyzeOptions, OptionValue),
    pub(crate) description: &'static str,
    pub(crate) increasing: Option<&'static str>,
    pub(crate) decreasing: Option<&'static str>,
    pub(crate) format_default: fn() -> String,
}

impl OptionSpec {
    pub(crate) fn descriptor(&self) -> AnalyzeOptionDescriptor {
        AnalyzeOptionDescriptor::new(
            self.path,
            Box::leak((self.format_default)().into_boxed_str()),
            self.kind.type_name(),
            self.category,
            self.description,
            self.increasing,
            self.decreasing,
        )
    }

    pub(crate) fn parse_and_set(
        &self,
        options: &mut AnalyzeOptions,
        value: &str,
    ) -> Result<(), AnalyzeConfigError> {
        let value = self.kind.parse(self.path, value)?;
        (self.set)(options, value);
        Ok(())
    }

    pub(crate) fn set_value(&self, options: &mut AnalyzeOptions, value: OptionValue) {
        (self.set)(options, value);
    }
}

macro_rules! value_accessors {
    ($get_fn:ident, $set_fn:ident, StringList, $group:ident.$field:ident) => {
        fn $get_fn(options: &AnalyzeOptions) -> OptionValue {
            OptionValue::StringList(options.$group.$field.clone())
        }
        fn $set_fn(options: &mut AnalyzeOptions, value: OptionValue) {
            let OptionValue::StringList(value) = value else {
                unreachable!("registry kind/setter mismatch")
            };
            options.$group.$field = value;
        }
    };
    ($get_fn:ident, $set_fn:ident, $variant:ident, $group:ident.$field:ident) => {
        fn $get_fn(options: &AnalyzeOptions) -> OptionValue {
            OptionValue::$variant(options.$group.$field)
        }
        fn $set_fn(options: &mut AnalyzeOptions, value: OptionValue) {
            let OptionValue::$variant(value) = value else {
                unreachable!("registry kind/setter mismatch")
            };
            options.$group.$field = value;
        }
    };
}

macro_rules! def_default {
    ($name:ident, $value:expr) => {
        fn $name() -> String {
            $value.to_string()
        }
    };
}

value_accessors!(
    get_queueing_trigger_permille,
    set_queueing_trigger_permille,
    U64,
    queueing.trigger_permille
);
value_accessors!(
    get_blocking_min_nonzero_samples_for_signal,
    set_blocking_min_nonzero_samples_for_signal,
    Usize,
    blocking.min_nonzero_samples_for_signal
);
value_accessors!(
    get_blocking_strong_p95_threshold,
    set_blocking_strong_p95_threshold,
    U64,
    blocking.strong_p95_threshold
);
value_accessors!(
    get_blocking_strong_peak_threshold,
    set_blocking_strong_peak_threshold,
    U64,
    blocking.strong_peak_threshold
);
value_accessors!(
    get_blocking_strong_nonzero_share_permille,
    set_blocking_strong_nonzero_share_permille,
    U64,
    blocking.strong_nonzero_share_permille
);
value_accessors!(
    get_blocking_strong_min_samples,
    set_blocking_strong_min_samples,
    Usize,
    blocking.strong_min_samples
);
value_accessors!(
    get_executor_min_global_queue_p95_for_signal,
    set_executor_min_global_queue_p95_for_signal,
    U64,
    executor.min_global_queue_p95_for_signal
);
value_accessors!(
    get_downstream_min_stage_samples,
    set_downstream_min_stage_samples,
    Usize,
    downstream.min_stage_samples
);
value_accessors!(
    get_downstream_blocking_correlated_stage_patterns,
    set_downstream_blocking_correlated_stage_patterns,
    StringList,
    downstream.blocking_correlated_stage_patterns
);
value_accessors!(
    get_downstream_blocking_correlation_score_margin,
    set_downstream_blocking_correlation_score_margin,
    U8,
    downstream.blocking_correlation_score_margin
);
value_accessors!(
    get_confidence_medium_score_threshold,
    set_confidence_medium_score_threshold,
    U8,
    confidence.medium_score_threshold
);
value_accessors!(
    get_confidence_high_score_threshold,
    set_confidence_high_score_threshold,
    U8,
    confidence.high_score_threshold
);
value_accessors!(
    get_confidence_ambiguity_min_score,
    set_confidence_ambiguity_min_score,
    U8,
    confidence.ambiguity_min_score
);
value_accessors!(
    get_confidence_ambiguity_score_gap,
    set_confidence_ambiguity_score_gap,
    U8,
    confidence.ambiguity_score_gap
);
value_accessors!(
    get_evidence_low_completed_request_threshold,
    set_evidence_low_completed_request_threshold,
    Usize,
    evidence.low_completed_request_threshold
);
value_accessors!(
    get_route_min_request_count,
    set_route_min_request_count,
    Usize,
    route.min_request_count
);
value_accessors!(
    get_route_breakdown_limit,
    set_route_breakdown_limit,
    Usize,
    route.breakdown_limit
);
value_accessors!(
    get_route_emit_on_divergent_suspects,
    set_route_emit_on_divergent_suspects,
    Bool,
    route.emit_on_divergent_suspects
);
value_accessors!(
    get_route_slowest_to_fastest_p95_ratio_numerator,
    set_route_slowest_to_fastest_p95_ratio_numerator,
    U64,
    route.slowest_to_fastest_p95_ratio_numerator
);
value_accessors!(
    get_route_slowest_to_fastest_p95_ratio_denominator,
    set_route_slowest_to_fastest_p95_ratio_denominator,
    U64,
    route.slowest_to_fastest_p95_ratio_denominator
);
value_accessors!(
    get_route_slowest_to_global_p95_ratio_numerator,
    set_route_slowest_to_global_p95_ratio_numerator,
    U64,
    route.slowest_to_global_p95_ratio_numerator
);
value_accessors!(
    get_route_slowest_to_global_p95_ratio_denominator,
    set_route_slowest_to_global_p95_ratio_denominator,
    U64,
    route.slowest_to_global_p95_ratio_denominator
);
value_accessors!(
    get_temporal_min_request_count,
    set_temporal_min_request_count,
    Usize,
    temporal.min_request_count
);
value_accessors!(
    get_temporal_min_segment_request_count,
    set_temporal_min_segment_request_count,
    Usize,
    temporal.min_segment_request_count
);
value_accessors!(
    get_temporal_share_shift_permille,
    set_temporal_share_shift_permille,
    U64,
    temporal.share_shift_permille
);
value_accessors!(
    get_temporal_p95_shift_ratio_numerator,
    set_temporal_p95_shift_ratio_numerator,
    U64,
    temporal.p95_shift_ratio_numerator
);
value_accessors!(
    get_temporal_p95_shift_ratio_denominator,
    set_temporal_p95_shift_ratio_denominator,
    U64,
    temporal.p95_shift_ratio_denominator
);
value_accessors!(
    get_temporal_emit_on_suspect_shift,
    set_temporal_emit_on_suspect_shift,
    Bool,
    temporal.emit_on_suspect_shift
);
value_accessors!(
    get_temporal_suppress_runtime_sparse_suspect_shift_without_supporting_movement,
    set_temporal_suppress_runtime_sparse_suspect_shift_without_supporting_movement,
    Bool,
    temporal.suppress_runtime_sparse_suspect_shift_without_supporting_movement
);

def_default!(d300, "300");
def_default!(d2, "2");
def_default!(d12, "12");
def_default!(d20, "20");
def_default!(d700, "700");
def_default!(d30, "30");
def_default!(d1, "1");
def_default!(d3, "3");
def_default!(
    dpatterns,
    "[\"spawn_blocking\", \"blocking_path\", \"blocking\"]"
);
def_default!(d65, "65");
def_default!(d85, "85");
def_default!(d60, "60");
def_default!(d4, "4");
def_default!(d10, "10");
def_default!(dtrue, "true");
def_default!(d5, "5");
def_default!(d8, "8");
def_default!(d200, "200");

macro_rules! spec {
    ($path:literal, $cat:literal, $kind:ident, $get:ident, $set:ident, $desc:literal, $inc:expr, $dec:expr, $def:ident) => {
        OptionSpec {
            path: $path,
            category: $cat,
            kind: ValueKind::$kind,
            get: $get,
            set: $set,
            description: $desc,
            increasing: $inc,
            decreasing: $dec,
            format_default: $def,
        }
    };
}

pub(crate) const OPTION_SPECS: &[OptionSpec] = &[
    spec!("queueing.trigger_permille", "queue suspect trigger", U64, get_queueing_trigger_permille, set_queueing_trigger_permille, "Minimum p95 queue share (permille) required before queue saturation becomes a ranked suspect.", Some("makes queue-saturation suspects harder to trigger"), Some("makes queue-saturation suspects easier to trigger"), d300),
    spec!("blocking.min_nonzero_samples_for_signal", "blocking signal eligibility", Usize, get_blocking_min_nonzero_samples_for_signal, set_blocking_min_nonzero_samples_for_signal, "Minimum non-zero blocking queue samples required before considering blocking pressure evidence.", Some("requires more samples before blocking signal can appear"), Some("requires fewer samples before blocking signal can appear"), d2),
    spec!("blocking.strong_p95_threshold", "blocking suspect strength", U64, get_blocking_strong_p95_threshold, set_blocking_strong_p95_threshold, "Blocking queue-depth p95 threshold used for strong blocking-pressure evidence.", Some("requires stronger p95 pressure"), Some("accepts weaker p95 pressure"), d12),
    spec!("blocking.strong_peak_threshold", "blocking suspect strength", U64, get_blocking_strong_peak_threshold, set_blocking_strong_peak_threshold, "Blocking queue-depth peak threshold used for strong blocking-pressure evidence.", Some("requires stronger peak pressure"), Some("accepts weaker peak pressure"), d20),
    spec!("blocking.strong_nonzero_share_permille", "blocking suspect strength", U64, get_blocking_strong_nonzero_share_permille, set_blocking_strong_nonzero_share_permille, "Minimum share of non-zero blocking samples (permille) for strong blocking-pressure evidence.", Some("requires a higher non-zero share"), Some("accepts a lower non-zero share"), d700),
    spec!("blocking.strong_min_samples", "blocking suspect strength", Usize, get_blocking_strong_min_samples, set_blocking_strong_min_samples, "Minimum blocking sample count needed before applying strong blocking-pressure thresholds.", Some("requires more samples for strong classification"), Some("requires fewer samples for strong classification"), d30),
    spec!("executor.min_global_queue_p95_for_signal", "executor signal eligibility", U64, get_executor_min_global_queue_p95_for_signal, set_executor_min_global_queue_p95_for_signal, "Minimum runtime global-queue p95 required before executor-pressure evidence is considered.", Some("requires higher runtime queue pressure"), Some("allows lower runtime queue pressure"), d1),
    spec!("downstream.min_stage_samples", "downstream stage eligibility", Usize, get_downstream_min_stage_samples, set_downstream_min_stage_samples, "Minimum captured samples per stage before downstream dominance is considered.", Some("requires more stage samples"), Some("requires fewer stage samples"), d3),
    spec!("downstream.blocking_correlated_stage_patterns", "downstream vs blocking interpretation", StringList, get_downstream_blocking_correlated_stage_patterns, set_downstream_blocking_correlated_stage_patterns, "Stage-name patterns used to spot downstream stages that likely correlate with blocking work.", None, None, dpatterns),
    spec!("downstream.blocking_correlation_score_margin", "downstream vs blocking interpretation", U8, get_downstream_blocking_correlation_score_margin, set_downstream_blocking_correlation_score_margin, "Minimum score gap used when distinguishing downstream-stage and blocking-correlated evidence.", Some("requires a wider score gap"), Some("allows a narrower score gap"), d2),
    spec!("confidence.medium_score_threshold", "confidence bucket thresholds", U8, get_confidence_medium_score_threshold, set_confidence_medium_score_threshold, "Minimum suspect score treated as medium confidence.", Some("raises medium-confidence bar"), Some("lowers medium-confidence bar"), d65),
    spec!("confidence.high_score_threshold", "confidence bucket thresholds", U8, get_confidence_high_score_threshold, set_confidence_high_score_threshold, "Minimum suspect score treated as high confidence.", Some("raises high-confidence bar"), Some("lowers high-confidence bar"), d85),
    spec!("confidence.ambiguity_min_score", "ambiguity warning", U8, get_confidence_ambiguity_min_score, set_confidence_ambiguity_min_score, "Minimum score for top suspects before ambiguity checks can trigger.", Some("requires stronger top scores before ambiguity warning"), Some("allows ambiguity warning with lower scores"), d60),
    spec!("confidence.ambiguity_score_gap", "ambiguity warning", U8, get_confidence_ambiguity_score_gap, set_confidence_ambiguity_score_gap, "Maximum score gap between top suspects to emit ambiguity warning.", Some("allows wider near-tie gaps"), Some("requires tighter near-tie gaps"), d4),
    spec!("evidence.low_completed_request_threshold", "evidence quality warnings", Usize, get_evidence_low_completed_request_threshold, set_evidence_low_completed_request_threshold, "Completed-request threshold below which low-sample warnings and conservative confidence limits apply.", Some("requires more completed requests to avoid low-sample warnings"), Some("requires fewer completed requests to avoid low-sample warnings"), d20),
    spec!("route.min_request_count", "route breakdown eligibility", Usize, get_route_min_request_count, set_route_min_request_count, "Minimum per-route completed request count required for route breakdown inclusion.", Some("filters out more low-volume routes"), Some("includes more low-volume routes"), d3),
    spec!("route.breakdown_limit", "route breakdown output size", Usize, get_route_breakdown_limit, set_route_breakdown_limit, "Maximum number of route breakdown entries emitted in one report.", Some("allows more route entries"), Some("allows fewer route entries"), d10),
    spec!("route.emit_on_divergent_suspects", "route divergence warning", Bool, get_route_emit_on_divergent_suspects, set_route_emit_on_divergent_suspects, "Whether to emit a global warning when route-level primary suspects diverge.", None, None, dtrue),
    spec!("route.slowest_to_fastest_p95_ratio_numerator", "route divergence detection", U64, get_route_slowest_to_fastest_p95_ratio_numerator, set_route_slowest_to_fastest_p95_ratio_numerator, "Numerator for the slowest-to-fastest route p95 ratio threshold.", Some("requires larger slowest/fastest disparity"), Some("requires smaller slowest/fastest disparity"), d3),
    spec!("route.slowest_to_fastest_p95_ratio_denominator", "route divergence detection", U64, get_route_slowest_to_fastest_p95_ratio_denominator, set_route_slowest_to_fastest_p95_ratio_denominator, "Denominator for the slowest-to-fastest route p95 ratio threshold.", Some("requires smaller slowest/fastest disparity"), Some("requires larger slowest/fastest disparity"), d2),
    spec!("route.slowest_to_global_p95_ratio_numerator", "route divergence detection", U64, get_route_slowest_to_global_p95_ratio_numerator, set_route_slowest_to_global_p95_ratio_numerator, "Numerator for the slowest-route to global p95 ratio threshold.", Some("requires larger slowest/global disparity"), Some("requires smaller slowest/global disparity"), d5),
    spec!("route.slowest_to_global_p95_ratio_denominator", "route divergence detection", U64, get_route_slowest_to_global_p95_ratio_denominator, set_route_slowest_to_global_p95_ratio_denominator, "Denominator for the slowest-route to global p95 ratio threshold.", Some("requires smaller slowest/global disparity"), Some("requires larger slowest/global disparity"), d4),
    spec!("temporal.min_request_count", "temporal segmentation eligibility", Usize, get_temporal_min_request_count, set_temporal_min_request_count, "Minimum completed requests required before temporal early/late segmentation is considered.", Some("requires more requests before temporal analysis"), Some("requires fewer requests before temporal analysis"), d20),
    spec!("temporal.min_segment_request_count", "temporal segmentation eligibility", Usize, get_temporal_min_segment_request_count, set_temporal_min_segment_request_count, "Minimum requests required in each temporal segment.", Some("requires larger per-segment sample size"), Some("allows smaller per-segment sample size"), d8),
    spec!("temporal.share_shift_permille", "temporal shift detection", U64, get_temporal_share_shift_permille, set_temporal_share_shift_permille, "Minimum queue/service share shift (permille) to flag temporal movement.", Some("requires larger share movement"), Some("allows smaller share movement"), d200),
    spec!("temporal.p95_shift_ratio_numerator", "temporal shift detection", U64, get_temporal_p95_shift_ratio_numerator, set_temporal_p95_shift_ratio_numerator, "Numerator for temporal p95 ratio shift threshold.", Some("requires larger p95 movement"), Some("requires smaller p95 movement"), d3),
    spec!("temporal.p95_shift_ratio_denominator", "temporal shift detection", U64, get_temporal_p95_shift_ratio_denominator, set_temporal_p95_shift_ratio_denominator, "Denominator for temporal p95 ratio shift threshold.", Some("requires smaller p95 movement"), Some("requires larger p95 movement"), d2),
    spec!("temporal.emit_on_suspect_shift", "temporal suspect-shift warning", Bool, get_temporal_emit_on_suspect_shift, set_temporal_emit_on_suspect_shift, "Whether temporal suspect-shift warnings are emitted when shifts are detected.", None, None, dtrue),
    spec!("temporal.suppress_runtime_sparse_suspect_shift_without_supporting_movement", "temporal warning suppression", Bool, get_temporal_suppress_runtime_sparse_suspect_shift_without_supporting_movement, set_temporal_suppress_runtime_sparse_suspect_shift_without_supporting_movement, "Whether to suppress runtime-sparse temporal suspect-shift warnings when supporting movement is absent.", None, None, dtrue),
];

pub(crate) fn find_spec(path: &str) -> Option<&'static OptionSpec> {
    OPTION_SPECS.iter().find(|spec| spec.path == path)
}

pub(crate) fn valid_override_paths() -> Vec<&'static str> {
    OPTION_SPECS.iter().map(|spec| spec.path).collect()
}

pub(crate) fn apply_path(
    options: &mut AnalyzeOptions,
    path: &str,
    value: &str,
) -> Result<(), AnalyzeConfigError> {
    let Some(spec) = find_spec(path) else {
        return Err(AnalyzeConfigError::UnknownOverridePath {
            path: path.to_string(),
            suggestion: suggest_path(path),
        });
    };
    spec.parse_and_set(options, value)
}

pub(crate) fn suggest_path(path: &str) -> Option<&'static str> {
    OPTION_SPECS
        .iter()
        .map(|candidate| (candidate.path, edit_distance(path, candidate.path)))
        .min_by_key(|(_, d)| *d)
        .and_then(|(c, d)| (d <= 3).then_some(c))
}

fn parse_num<T: std::str::FromStr>(
    path: &'static str,
    value: &str,
    expected: &'static str,
) -> Result<T, AnalyzeConfigError> {
    value
        .parse()
        .map_err(|_| invalid_value(path, value, expected))
}

fn invalid_value(path: &'static str, value: &str, expected: &'static str) -> AnalyzeConfigError {
    AnalyzeConfigError::InvalidOverrideValue {
        path,
        value: value.to_string(),
        expected,
    }
}

fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut curr = vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        curr[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            curr[j + 1] = (prev[j + 1] + 1).min(curr[j] + 1).min(prev[j] + cost);
        }
        prev.clone_from(&curr);
    }
    prev[b.len()]
}
