use super::{
    AnalyzeConfigError, AnalyzeConfigOverrideSummary, AnalyzeOptionDescriptor, AnalyzeOptions,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum OptionValue {
    U64(u64),
    Usize(usize),
    U8(u8),
    Bool(bool),
    StringList(Vec<String>),
}

impl OptionValue {
    fn kind(&self) -> ValueKind {
        match self {
            Self::U64(_) => ValueKind::U64,
            Self::Usize(_) => ValueKind::Usize,
            Self::U8(_) => ValueKind::U8,
            Self::Bool(_) => ValueKind::Bool,
            Self::StringList(_) => ValueKind::StringList,
        }
    }

    fn display_value(&self) -> String {
        match self {
            Self::U64(v) => v.to_string(),
            Self::Usize(v) => v.to_string(),
            Self::U8(v) => v.to_string(),
            Self::Bool(v) => v.to_string(),
            Self::StringList(values) => values.join(","),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ValueKind {
    U64,
    Usize,
    U8,
    Bool,
    StringList,
}

impl ValueKind {
    const fn type_name(self) -> &'static str {
        match self {
            Self::U64 => "u64",
            Self::Usize => "usize",
            Self::U8 => "u8",
            Self::Bool => "bool",
            Self::StringList => "Vec<String>",
        }
    }

    fn parse_cli(self, path: &'static str, value: &str) -> Result<OptionValue, AnalyzeConfigError> {
        match self {
            Self::U64 => {
                parse_num(path, value, "base-10 unsigned integer (u64)").map(OptionValue::U64)
            }
            Self::Usize => {
                parse_num(path, value, "base-10 unsigned integer (usize)").map(OptionValue::Usize)
            }
            Self::U8 => parse_num(
                path,
                value,
                "base-10 unsigned integer in range 0..=255 (u8)",
            )
            .map(OptionValue::U8),
            Self::Bool => match value {
                "true" => Ok(OptionValue::Bool(true)),
                "false" => Ok(OptionValue::Bool(false)),
                _ => Err(AnalyzeConfigError::InvalidOverrideValue {
                    path,
                    value: value.to_string(),
                    expected: "'true' or 'false'",
                }),
            },
            Self::StringList => parse_string_list(path, value).map(OptionValue::StringList),
        }
    }
}

pub(super) struct OptionEntry {
    pub(super) path: &'static str,
    kind: ValueKind,
    affects: &'static str,
    description: &'static str,
    increasing: Option<&'static str>,
    decreasing: Option<&'static str>,
    get: fn(&AnalyzeOptions) -> OptionValue,
    set: fn(&mut AnalyzeOptions, OptionValue),
    descriptor_default: &'static str,
}

impl OptionEntry {
    pub(super) fn descriptor(&self) -> AnalyzeOptionDescriptor {
        AnalyzeOptionDescriptor::new(
            self.path,
            self.default_display(),
            self.kind.type_name(),
            self.affects,
            self.description,
            self.increasing,
            self.decreasing,
        )
    }

    pub(super) fn parse_cli(&self, value: &str) -> Result<OptionValue, AnalyzeConfigError> {
        self.kind.parse_cli(self.path, value)
    }

    pub(super) fn apply(&self, options: &mut AnalyzeOptions, value: OptionValue) {
        debug_assert_eq!(value.kind(), self.kind);
        (self.set)(options, value);
    }

    fn read(&self, options: &AnalyzeOptions) -> OptionValue {
        (self.get)(options)
    }

    fn default_display(&self) -> &'static str {
        self.descriptor_default
    }
}

macro_rules! option_entries {
    ($($path:literal, $kind:ident, $default_display:literal, $affects:literal, $description:literal, $increasing:expr, $decreasing:expr, $get:expr, $set:expr;)*) => {
        pub(super) const OPTION_ENTRIES: &[OptionEntry] = &[
            $(OptionEntry { path: $path, kind: ValueKind::$kind, descriptor_default: $default_display, affects: $affects, description: $description, increasing: $increasing, decreasing: $decreasing, get: $get, set: $set },)*
        ];
    };
}

option_entries! {
"queueing.trigger_permille", U64, "300", "queue suspect trigger", "Minimum p95 queue share (permille) required before queue saturation becomes a ranked suspect.", Some("makes queue-saturation suspects harder to trigger"), Some("makes queue-saturation suspects easier to trigger"), |o| OptionValue::U64(o.queueing.trigger_permille), |o, v| if let OptionValue::U64(v) = v { o.queueing.trigger_permille = v };
"blocking.min_nonzero_samples_for_signal", Usize, "2", "blocking signal eligibility", "Minimum non-zero blocking queue samples required before considering blocking pressure evidence.", Some("requires more samples before blocking signal can appear"), Some("requires fewer samples before blocking signal can appear"), |o| OptionValue::Usize(o.blocking.min_nonzero_samples_for_signal), |o, v| if let OptionValue::Usize(v) = v { o.blocking.min_nonzero_samples_for_signal = v };
"blocking.strong_p95_threshold", U64, "12", "blocking suspect strength", "Blocking queue-depth p95 threshold used for strong blocking-pressure evidence.", Some("requires stronger p95 pressure"), Some("accepts weaker p95 pressure"), |o| OptionValue::U64(o.blocking.strong_p95_threshold), |o, v| if let OptionValue::U64(v) = v { o.blocking.strong_p95_threshold = v };
"blocking.strong_peak_threshold", U64, "20", "blocking suspect strength", "Blocking queue-depth peak threshold used for strong blocking-pressure evidence.", Some("requires stronger peak pressure"), Some("accepts weaker peak pressure"), |o| OptionValue::U64(o.blocking.strong_peak_threshold), |o, v| if let OptionValue::U64(v) = v { o.blocking.strong_peak_threshold = v };
"blocking.strong_nonzero_share_permille", U64, "700", "blocking suspect strength", "Minimum share of non-zero blocking samples (permille) for strong blocking-pressure evidence.", Some("requires a higher non-zero share"), Some("accepts a lower non-zero share"), |o| OptionValue::U64(o.blocking.strong_nonzero_share_permille), |o, v| if let OptionValue::U64(v) = v { o.blocking.strong_nonzero_share_permille = v };
"blocking.strong_min_samples", Usize, "30", "blocking suspect strength", "Minimum blocking sample count needed before applying strong blocking-pressure thresholds.", Some("requires more samples for strong classification"), Some("requires fewer samples for strong classification"), |o| OptionValue::Usize(o.blocking.strong_min_samples), |o, v| if let OptionValue::Usize(v) = v { o.blocking.strong_min_samples = v };
"executor.min_global_queue_p95_for_signal", U64, "1", "executor signal eligibility", "Minimum runtime global-queue p95 required before executor-pressure evidence is considered.", Some("requires higher runtime queue pressure"), Some("allows lower runtime queue pressure"), |o| OptionValue::U64(o.executor.min_global_queue_p95_for_signal), |o, v| if let OptionValue::U64(v) = v { o.executor.min_global_queue_p95_for_signal = v };
"downstream.min_stage_samples", Usize, "3", "downstream stage eligibility", "Minimum captured samples per stage before downstream dominance is considered.", Some("requires more stage samples"), Some("requires fewer stage samples"), |o| OptionValue::Usize(o.downstream.min_stage_samples), |o, v| if let OptionValue::Usize(v) = v { o.downstream.min_stage_samples = v };
"downstream.blocking_correlated_stage_patterns", StringList, "[\"spawn_blocking\", \"blocking_path\", \"blocking\"]", "downstream vs blocking interpretation", "Stage-name patterns used to spot downstream stages that likely correlate with blocking work.", None, None, |o| OptionValue::StringList(o.downstream.blocking_correlated_stage_patterns.clone()), |o, v| if let OptionValue::StringList(v) = v { o.downstream.blocking_correlated_stage_patterns = v };
"downstream.blocking_correlation_score_margin", U8, "2", "downstream vs blocking interpretation", "Minimum score gap used when distinguishing downstream-stage and blocking-correlated evidence.", Some("requires a wider score gap"), Some("allows a narrower score gap"), |o| OptionValue::U8(o.downstream.blocking_correlation_score_margin), |o, v| if let OptionValue::U8(v) = v { o.downstream.blocking_correlation_score_margin = v };
"confidence.medium_score_threshold", U8, "65", "confidence bucket thresholds", "Minimum suspect score treated as medium confidence.", Some("raises medium-confidence bar"), Some("lowers medium-confidence bar"), |o| OptionValue::U8(o.confidence.medium_score_threshold), |o, v| if let OptionValue::U8(v) = v { o.confidence.medium_score_threshold = v };
"confidence.high_score_threshold", U8, "85", "confidence bucket thresholds", "Minimum suspect score treated as high confidence.", Some("raises high-confidence bar"), Some("lowers high-confidence bar"), |o| OptionValue::U8(o.confidence.high_score_threshold), |o, v| if let OptionValue::U8(v) = v { o.confidence.high_score_threshold = v };
"confidence.ambiguity_min_score", U8, "60", "ambiguity warning", "Minimum score for top suspects before ambiguity checks can trigger.", Some("requires stronger top scores before ambiguity warning"), Some("allows ambiguity warning with lower scores"), |o| OptionValue::U8(o.confidence.ambiguity_min_score), |o, v| if let OptionValue::U8(v) = v { o.confidence.ambiguity_min_score = v };
"confidence.ambiguity_score_gap", U8, "4", "ambiguity warning", "Maximum score gap between top suspects to emit ambiguity warning.", Some("allows wider near-tie gaps"), Some("requires tighter near-tie gaps"), |o| OptionValue::U8(o.confidence.ambiguity_score_gap), |o, v| if let OptionValue::U8(v) = v { o.confidence.ambiguity_score_gap = v };
"evidence.low_completed_request_threshold", Usize, "20", "evidence quality warnings", "Completed-request threshold below which low-sample warnings and conservative confidence limits apply.", Some("requires more completed requests to avoid low-sample warnings"), Some("requires fewer completed requests to avoid low-sample warnings"), |o| OptionValue::Usize(o.evidence.low_completed_request_threshold), |o, v| if let OptionValue::Usize(v) = v { o.evidence.low_completed_request_threshold = v };
"route.min_request_count", Usize, "3", "route breakdown eligibility", "Minimum per-route completed request count required for route breakdown inclusion.", Some("filters out more low-volume routes"), Some("includes more low-volume routes"), |o| OptionValue::Usize(o.route.min_request_count), |o, v| if let OptionValue::Usize(v) = v { o.route.min_request_count = v };
"route.breakdown_limit", Usize, "10", "route breakdown output size", "Maximum number of route breakdown entries emitted in one report.", Some("allows more route entries"), Some("allows fewer route entries"), |o| OptionValue::Usize(o.route.breakdown_limit), |o, v| if let OptionValue::Usize(v) = v { o.route.breakdown_limit = v };
"route.emit_on_divergent_suspects", Bool, "true", "route divergence warning", "Whether to emit a global warning when route-level primary suspects diverge.", None, None, |o| OptionValue::Bool(o.route.emit_on_divergent_suspects), |o, v| if let OptionValue::Bool(v) = v { o.route.emit_on_divergent_suspects = v };
"route.slowest_to_fastest_p95_ratio_numerator", U64, "3", "route divergence detection", "Numerator for the slowest-to-fastest route p95 ratio threshold.", Some("requires larger slowest/fastest disparity"), Some("requires smaller slowest/fastest disparity"), |o| OptionValue::U64(o.route.slowest_to_fastest_p95_ratio_numerator), |o, v| if let OptionValue::U64(v) = v { o.route.slowest_to_fastest_p95_ratio_numerator = v };
"route.slowest_to_fastest_p95_ratio_denominator", U64, "2", "route divergence detection", "Denominator for the slowest-to-fastest route p95 ratio threshold.", Some("requires smaller slowest/fastest disparity"), Some("requires larger slowest/fastest disparity"), |o| OptionValue::U64(o.route.slowest_to_fastest_p95_ratio_denominator), |o, v| if let OptionValue::U64(v) = v { o.route.slowest_to_fastest_p95_ratio_denominator = v };
"route.slowest_to_global_p95_ratio_numerator", U64, "5", "route divergence detection", "Numerator for the slowest-route to global p95 ratio threshold.", Some("requires larger slowest/global disparity"), Some("requires smaller slowest/global disparity"), |o| OptionValue::U64(o.route.slowest_to_global_p95_ratio_numerator), |o, v| if let OptionValue::U64(v) = v { o.route.slowest_to_global_p95_ratio_numerator = v };
"route.slowest_to_global_p95_ratio_denominator", U64, "4", "route divergence detection", "Denominator for the slowest-route to global p95 ratio threshold.", Some("requires smaller slowest/global disparity"), Some("requires larger slowest/global disparity"), |o| OptionValue::U64(o.route.slowest_to_global_p95_ratio_denominator), |o, v| if let OptionValue::U64(v) = v { o.route.slowest_to_global_p95_ratio_denominator = v };
"temporal.min_request_count", Usize, "20", "temporal segmentation eligibility", "Minimum completed requests required before temporal early/late segmentation is considered.", Some("requires more requests before temporal analysis"), Some("requires fewer requests before temporal analysis"), |o| OptionValue::Usize(o.temporal.min_request_count), |o, v| if let OptionValue::Usize(v) = v { o.temporal.min_request_count = v };
"temporal.min_segment_request_count", Usize, "8", "temporal segmentation eligibility", "Minimum requests required in each temporal segment.", Some("requires larger per-segment sample size"), Some("allows smaller per-segment sample size"), |o| OptionValue::Usize(o.temporal.min_segment_request_count), |o, v| if let OptionValue::Usize(v) = v { o.temporal.min_segment_request_count = v };
"temporal.share_shift_permille", U64, "200", "temporal shift detection", "Minimum queue/service share shift (permille) to flag temporal movement.", Some("requires larger share movement"), Some("allows smaller share movement"), |o| OptionValue::U64(o.temporal.share_shift_permille), |o, v| if let OptionValue::U64(v) = v { o.temporal.share_shift_permille = v };
"temporal.p95_shift_ratio_numerator", U64, "3", "temporal shift detection", "Numerator for temporal p95 ratio shift threshold.", Some("requires larger p95 movement"), Some("requires smaller p95 movement"), |o| OptionValue::U64(o.temporal.p95_shift_ratio_numerator), |o, v| if let OptionValue::U64(v) = v { o.temporal.p95_shift_ratio_numerator = v };
"temporal.p95_shift_ratio_denominator", U64, "2", "temporal shift detection", "Denominator for temporal p95 ratio shift threshold.", Some("requires smaller p95 movement"), Some("requires larger p95 movement"), |o| OptionValue::U64(o.temporal.p95_shift_ratio_denominator), |o, v| if let OptionValue::U64(v) = v { o.temporal.p95_shift_ratio_denominator = v };
"temporal.emit_on_suspect_shift", Bool, "true", "temporal suspect-shift warning", "Whether temporal suspect-shift warnings are emitted when shifts are detected.", None, None, |o| OptionValue::Bool(o.temporal.emit_on_suspect_shift), |o, v| if let OptionValue::Bool(v) = v { o.temporal.emit_on_suspect_shift = v };
"temporal.suppress_runtime_sparse_suspect_shift_without_supporting_movement", Bool, "true", "temporal warning suppression", "Whether to suppress runtime-sparse temporal suspect-shift warnings when supporting movement is absent.", None, None, |o| OptionValue::Bool(o.temporal.suppress_runtime_sparse_suspect_shift_without_supporting_movement), |o, v| if let OptionValue::Bool(v) = v { o.temporal.suppress_runtime_sparse_suspect_shift_without_supporting_movement = v };
}

pub(super) fn find_entry(path: &str) -> Option<&'static OptionEntry> {
    OPTION_ENTRIES.iter().find(|entry| entry.path == path)
}

pub(super) fn apply_typed_path(
    options: &mut AnalyzeOptions,
    path: &str,
    value: OptionValue,
) -> Result<(), AnalyzeConfigError> {
    let Some(entry) = find_entry(path) else {
        return Err(AnalyzeConfigError::UnknownOverridePath {
            path: path.to_string(),
            suggestion: suggest_path(path),
        });
    };
    entry.apply(options, value);
    Ok(())
}

pub(super) fn non_default_overrides(options: &AnalyzeOptions) -> Vec<AnalyzeConfigOverrideSummary> {
    let defaults = AnalyzeOptions::default();
    let mut out: Vec<_> = OPTION_ENTRIES
        .iter()
        .filter_map(|entry| {
            let current = entry.read(options);
            (current != entry.read(&defaults)).then(|| AnalyzeConfigOverrideSummary {
                path: entry.path.to_string(),
                value: current.display_value(),
            })
        })
        .collect();
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

pub(super) fn suggest_path(path: &str) -> Option<&'static str> {
    OPTION_ENTRIES
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
        .map_err(|_| AnalyzeConfigError::InvalidOverrideValue {
            path,
            value: value.to_string(),
            expected,
        })
}

fn parse_string_list(path: &'static str, value: &str) -> Result<Vec<String>, AnalyzeConfigError> {
    let mut out = Vec::new();
    for entry in value.split(',') {
        let trimmed = entry.trim();
        if trimmed.is_empty() {
            return Err(AnalyzeConfigError::InvalidOverrideValue {
                path,
                value: value.to_string(),
                expected: "comma-separated non-empty entries (Vec<String>)",
            });
        }
        out.push(trimmed.to_string());
    }
    Ok(out)
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
