//! Canonical `Run` integrity inspection and normalization.
#![allow(clippy::too_many_lines, clippy::too_many_arguments)]

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::{InFlightSnapshot, QueueEvent, RequestEvent, Run, StageEvent, SCHEMA_VERSION};

/// Shared tolerance for comparing duration fields with complete run-relative intervals.
pub const RUN_RELATIVE_DURATION_TOLERANCE_US: u64 = 2_000;

/// Severity of a run validation issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RunValidationSeverity {
    /// Non-fatal validation limitation.
    Warning,
    /// Fatal generic integrity violation.
    Error,
}

/// Stable canonical issue code for generic run validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RunValidationIssueCode {
    /// Run schema version is not supported.
    UnsupportedSchemaVersion,
    /// A required string field is blank.
    EmptyRequiredField,
    /// An end timestamp or offset is before its start.
    InvertedInterval,
    /// Exactly one run-relative interval endpoint is present.
    PartialRunRelativeInterval,
    /// Duration differs from complete run-relative interval beyond tolerance.
    DurationMismatch,
    /// Completed `request_id` is duplicated among otherwise valid requests.
    DuplicateCompletedRequestId,
    /// Child references a duplicated parent `request_id`.
    AmbiguousParentRequestId,
    /// Child references no completed valid parent request.
    OrphanRequestScopedEvent,
    /// Child references a parent excluded by validation.
    ParentRequestExcluded,
    /// Complete precise child interval lies outside its parent request.
    ChildIntervalOutsideRequest,
    /// Run-relative interval is absent so precise validation is unavailable.
    PreciseIntervalValidationUnavailable,
}

impl RunValidationIssueCode {
    /// Stable snake-case label for this issue code.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UnsupportedSchemaVersion => "unsupported_schema_version",
            Self::EmptyRequiredField => "empty_required_field",
            Self::InvertedInterval => "inverted_interval",
            Self::PartialRunRelativeInterval => "partial_run_relative_interval",
            Self::DurationMismatch => "duration_mismatch",
            Self::DuplicateCompletedRequestId => "duplicate_completed_request_id",
            Self::AmbiguousParentRequestId => "ambiguous_parent_request_id",
            Self::OrphanRequestScopedEvent => "orphan_request_scoped_event",
            Self::ParentRequestExcluded => "parent_request_excluded",
            Self::ChildIntervalOutsideRequest => "child_interval_outside_request",
            Self::PreciseIntervalValidationUnavailable => "precise_interval_validation_unavailable",
        }
    }
}

/// Section containing an event or metadata issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RunSection {
    /// Run metadata.
    Metadata,
    /// Completed request events.
    Requests,
    /// Request stage events.
    Stages,
    /// Request queue events.
    Queues,
    /// In-flight snapshots.
    Inflight,
    /// Runtime metric snapshots.
    RuntimeSnapshots,
}

/// Deterministic location for a validation issue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunValidationLocation {
    /// Section containing the issue.
    pub section: RunSection,
    /// Input index within the section, or None for metadata.
    pub index: Option<usize>,
    /// Field associated with the issue, when applicable.
    pub field: Option<&'static str>,
}

/// One validation issue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunValidationIssue {
    /// Stable issue code.
    pub code: RunValidationIssueCode,
    /// Issue severity.
    pub severity: RunValidationSeverity,
    /// Deterministic issue location.
    pub location: RunValidationLocation,
    /// Human-readable explanation.
    pub message: String,
}

/// Deterministic validation report.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RunValidationReport {
    /// Deterministically ordered issues.
    pub issues: Vec<RunValidationIssue>,
}

/// Retention/exclusion outcome for one input event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunEventDispositionKind {
    /// Event was retained in normalized output.
    Retained {
        /// Output index in the normalized section.
        output_index: usize,
    },
    /// Event was excluded from normalized output.
    Excluded {
        /// Unique enum-ordered issue codes responsible for exclusion.
        issue_codes: Vec<RunValidationIssueCode>,
    },
}

/// Disposition for one input event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunEventDisposition {
    /// Input section.
    pub section: RunSection,
    /// Input index within the section.
    pub input_index: usize,
    /// Retained or excluded outcome.
    pub disposition: RunEventDispositionKind,
}

/// Permissively normalized run plus issues and per-input event dispositions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedRun {
    /// Normalized run artifact.
    pub run: Run,
    /// Validation report observed while normalizing.
    pub report: RunValidationReport,
    /// Per-input event dispositions.
    pub dispositions: Vec<RunEventDisposition>,
}

/// Strict validation error exposing the full deterministic report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunValidationError {
    report: RunValidationReport,
}

impl RunValidationError {
    /// Returns the deterministic report that failed strict validation.
    #[must_use]
    pub const fn report(&self) -> &RunValidationReport {
        &self.report
    }
}
impl fmt::Display for RunValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "strict run validation failed with {} error issue(s)",
            self.report
                .issues
                .iter()
                .filter(|i| i.severity == RunValidationSeverity::Error)
                .count()
        )
    }
}
impl std::error::Error for RunValidationError {}

/// Inspects a run for generic integrity issues without changing it.
#[must_use]
pub fn inspect_run(run: &Run) -> RunValidationReport {
    normalize_inner(run, false).report
}

/// Strictly validates a run and rejects every error-level issue.
///
/// # Errors
///
/// Returns [`RunValidationError`] when the inspected report contains any error-level issue.
pub fn validate_run_strict(run: &Run) -> Result<(), RunValidationError> {
    let report = inspect_run(run);
    if report
        .issues
        .iter()
        .any(|i| i.severity == RunValidationSeverity::Error)
    {
        Err(RunValidationError { report })
    } else {
        Ok(())
    }
}

/// Returns a deterministic permissive normalized run.
#[must_use]
pub fn normalize_run_permissive(run: &Run) -> NormalizedRun {
    normalize_inner(run, true)
}

/// Audience for canonical run validation summaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunValidationSummaryAudience {
    /// Analyzer-facing warnings include unchanged legacy precision limitations.
    Analyzer,
    /// Durable lifecycle warnings include only output-changing findings.
    Lifecycle,
}

/// Returns bounded deterministic analyzer-facing validation warning summaries.
#[must_use]
pub fn summarize_run_validation(normalized: &NormalizedRun) -> Vec<String> {
    summarize_normalized_run(normalized, RunValidationSummaryAudience::Analyzer)
}

/// Returns bounded deterministic validation summaries for durable lifecycle
/// warnings: only issues that changed canonical output or cleared invalid
/// optional precision are included.
#[must_use]
pub fn summarize_run_validation_lifecycle(normalized: &NormalizedRun) -> Vec<String> {
    summarize_normalized_run(normalized, RunValidationSummaryAudience::Lifecycle)
}

/// Returns bounded deterministic validation summaries for the selected audience.
#[must_use]
pub fn summarize_normalized_run(
    normalized: &NormalizedRun,
    audience: RunValidationSummaryAudience,
) -> Vec<String> {
    let mut groups =
        BTreeMap::<(RunValidationIssueCode, RunSection, SummaryAction), (usize, Vec<String>)>::new(
        );
    for issue in &normalized.report.issues {
        if audience == RunValidationSummaryAudience::Lifecycle
            && issue.code == RunValidationIssueCode::PreciseIntervalValidationUnavailable
        {
            continue;
        }
        let action = summary_action(normalized, issue);
        if audience == RunValidationSummaryAudience::Lifecycle && !action.changed_canonical_output()
        {
            continue;
        }
        let entry = groups
            .entry((issue.code, issue.location.section, action))
            .or_default();
        entry.0 += 1;
        if entry.1.len() < 5 {
            if let Some(index) = issue.location.index {
                entry.1.push(format!("index {index}"));
            } else if let Some(field) = issue.location.field {
                entry.1.push(field.to_string());
            }
        }
    }
    groups
        .into_iter()
        .map(|((code, section, action), (count, samples))| {
            let subject = if section == RunSection::Metadata {
                "finding(s)"
            } else {
                "event(s)"
            };
            let sample = if samples.is_empty() {
                String::new()
            } else {
                format!(" sample: {}.", samples.join(", "))
            };
            format!(
                "Run validation {}: {} {} {} {}{}",
                code.as_str(),
                count,
                section.as_str(),
                subject,
                action.as_str(),
                sample
            )
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum SummaryAction {
    EvidenceExcluded,
    OptionalOffsetsClearedDurationRetained,
    OptionalPrecisionUnavailableEvidenceUnchanged,
    MetadataFailedValidation,
}

impl SummaryAction {
    const fn as_str(self) -> &'static str {
        match self {
            Self::EvidenceExcluded => "were excluded from analysis.",
            Self::OptionalOffsetsClearedDurationRetained => {
                "had optional run-relative offsets cleared while authoritative duration evidence was retained."
            }
            Self::OptionalPrecisionUnavailableEvidenceUnchanged => {
                "lack optional run-relative precision; legacy duration evidence was retained unchanged."
            }
            Self::MetadataFailedValidation => "failed validation without removing event evidence.",
        }
    }

    const fn changed_canonical_output(self) -> bool {
        matches!(
            self,
            Self::EvidenceExcluded | Self::OptionalOffsetsClearedDurationRetained
        )
    }
}

fn summary_action(normalized: &NormalizedRun, issue: &RunValidationIssue) -> SummaryAction {
    if issue.location.section == RunSection::Metadata {
        return SummaryAction::MetadataFailedValidation;
    }
    if let Some(index) = issue.location.index {
        if normalized.dispositions.iter().any(|disposition| {
            disposition.section == issue.location.section
                && disposition.input_index == index
                && matches!(
                    disposition.disposition,
                    RunEventDispositionKind::Excluded { .. }
                )
        }) {
            return SummaryAction::EvidenceExcluded;
        }
    }
    match issue.code {
        RunValidationIssueCode::PartialRunRelativeInterval
        | RunValidationIssueCode::DurationMismatch => {
            SummaryAction::OptionalOffsetsClearedDurationRetained
        }
        RunValidationIssueCode::InvertedInterval if issue.location.field.is_none() => {
            SummaryAction::OptionalOffsetsClearedDurationRetained
        }
        RunValidationIssueCode::PreciseIntervalValidationUnavailable => {
            SummaryAction::OptionalPrecisionUnavailableEvidenceUnchanged
        }
        _ => SummaryAction::EvidenceExcluded,
    }
}

impl RunSection {
    /// Stable lowercase label for this section.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Metadata => "metadata",
            Self::Requests => "request",
            Self::Stages => "stage",
            Self::Queues => "queue",
            Self::Inflight => "in-flight",
            Self::RuntimeSnapshots => "runtime snapshot",
        }
    }
}

#[derive(Default)]
struct Ctx {
    issues: Vec<RunValidationIssue>,
}
impl Ctx {
    fn issue(
        &mut self,
        section: RunSection,
        index: Option<usize>,
        field: Option<&'static str>,
        code: RunValidationIssueCode,
        severity: RunValidationSeverity,
        message: impl Into<String>,
    ) {
        self.issues.push(RunValidationIssue {
            code,
            severity,
            location: RunValidationLocation {
                section,
                index,
                field,
            },
            message: message.into(),
        });
    }
    fn sorted_report(mut self) -> RunValidationReport {
        self.issues.sort_by(|a, b| {
            (
                a.location.section,
                a.location.index,
                a.code,
                a.location.field,
            )
                .cmp(&(
                    b.location.section,
                    b.location.index,
                    b.code,
                    b.location.field,
                ))
        });
        RunValidationReport {
            issues: self.issues,
        }
    }
}

fn normalize_inner(run: &Run, produce_run: bool) -> NormalizedRun {
    let mut ctx = Ctx::default();
    if run.schema_version != SCHEMA_VERSION {
        ctx.issue(
            RunSection::Metadata,
            None,
            Some("schema_version"),
            RunValidationIssueCode::UnsupportedSchemaVersion,
            RunValidationSeverity::Error,
            "unsupported Run schema version",
        );
    }
    if run.metadata.run_id.trim().is_empty() {
        ctx.issue(
            RunSection::Metadata,
            None,
            Some("run_id"),
            RunValidationIssueCode::EmptyRequiredField,
            RunValidationSeverity::Error,
            "metadata.run_id must not be blank",
        );
    }
    if run.metadata.service_name.trim().is_empty() {
        ctx.issue(
            RunSection::Metadata,
            None,
            Some("service_name"),
            RunValidationIssueCode::EmptyRequiredField,
            RunValidationSeverity::Error,
            "metadata.service_name must not be blank",
        );
    }
    if run.metadata.finished_at_unix_ms < run.metadata.started_at_unix_ms {
        ctx.issue(
            RunSection::Metadata,
            None,
            Some("finished_at_unix_ms"),
            RunValidationIssueCode::InvertedInterval,
            RunValidationSeverity::Error,
            "metadata finished time is before started time",
        );
    }
    if let Some(f) = run.metadata.finalized_at_unix_ms {
        if f < run.metadata.finished_at_unix_ms {
            ctx.issue(
                RunSection::Metadata,
                None,
                Some("finalized_at_unix_ms"),
                RunValidationIssueCode::InvertedInterval,
                RunValidationSeverity::Error,
                "metadata finalization time is before finished time",
            );
        }
    }

    let mut request_invalid = vec![BTreeSet::new(); run.requests.len()];
    let mut request_precision_clear = vec![BTreeSet::new(); run.requests.len()];
    for (i, r) in run.requests.iter().enumerate() {
        inspect_request(
            &mut ctx,
            &mut request_invalid[i],
            &mut request_precision_clear[i],
            i,
            r,
        );
    }
    let mut counts = BTreeMap::<&str, usize>::new();
    for (i, r) in run.requests.iter().enumerate() {
        if request_invalid[i].is_empty() {
            *counts.entry(r.request_id.as_str()).or_default() += 1;
        }
    }
    let dup_ids = counts
        .iter()
        .filter(|(_, c)| **c > 1)
        .map(|(id, _)| *id)
        .collect::<BTreeSet<_>>();
    for (i, r) in run.requests.iter().enumerate() {
        if dup_ids.contains(r.request_id.as_str()) {
            request_invalid[i].insert(RunValidationIssueCode::DuplicateCompletedRequestId);
            ctx.issue(
                RunSection::Requests,
                Some(i),
                Some("request_id"),
                RunValidationIssueCode::DuplicateCompletedRequestId,
                RunValidationSeverity::Error,
                "duplicate completed request_id is ambiguous",
            );
        }
    }
    let normalized_requests_by_input = run
        .requests
        .iter()
        .enumerate()
        .map(|(i, r)| {
            if request_invalid[i].is_empty() {
                Some(clear_bad_request(r, &request_precision_clear[i]))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    let mut parent_states =
        build_parent_states(run, &request_invalid, &normalized_requests_by_input);
    for id in &dup_ids {
        parent_states.insert(*id, ParentState::AmbiguousRetained);
    }
    let retained_requests = normalized_requests_by_input
        .iter()
        .filter_map(Clone::clone)
        .collect::<Vec<_>>();
    let mut normalized = run.clone();
    if produce_run {
        normalized.requests = retained_requests;
    }
    let mut dispositions = Vec::new();
    add_disps(&mut dispositions, RunSection::Requests, &request_invalid);
    let mut stage_invalid = vec![BTreeSet::new(); run.stages.len()];
    let mut stage_precision_clear = vec![BTreeSet::new(); run.stages.len()];
    let mut stages = Vec::new();
    for (i, s) in run.stages.iter().enumerate() {
        inspect_stage(
            &mut ctx,
            &mut stage_invalid[i],
            &mut stage_precision_clear[i],
            i,
            s,
        );
        child_policy(
            &mut ctx,
            &mut stage_invalid[i],
            RunSection::Stages,
            i,
            s.request_id.as_str(),
            &parent_states,
            interval_stage(&clear_bad_stage(s, &stage_precision_clear[i])),
        );
        if stage_invalid[i].is_empty() {
            stages.push(clear_bad_stage(s, &stage_precision_clear[i]));
        }
    }
    if produce_run {
        normalized.stages = stages;
    }
    add_disps(&mut dispositions, RunSection::Stages, &stage_invalid);
    let mut queue_invalid = vec![BTreeSet::new(); run.queues.len()];
    let mut queue_precision_clear = vec![BTreeSet::new(); run.queues.len()];
    let mut queues = Vec::new();
    for (i, q) in run.queues.iter().enumerate() {
        inspect_queue(
            &mut ctx,
            &mut queue_invalid[i],
            &mut queue_precision_clear[i],
            i,
            q,
        );
        child_policy(
            &mut ctx,
            &mut queue_invalid[i],
            RunSection::Queues,
            i,
            q.request_id.as_str(),
            &parent_states,
            interval_queue(&clear_bad_queue(q, &queue_precision_clear[i])),
        );
        if queue_invalid[i].is_empty() {
            queues.push(clear_bad_queue(q, &queue_precision_clear[i]));
        }
    }
    if produce_run {
        normalized.queues = queues;
    }
    add_disps(&mut dispositions, RunSection::Queues, &queue_invalid);
    let mut inflight_invalid = vec![BTreeSet::new(); run.inflight.len()];
    let inflight = run
        .inflight
        .iter()
        .enumerate()
        .filter_map(|(i, s)| {
            if s.gauge.trim().is_empty() {
                inflight_invalid[i].insert(RunValidationIssueCode::EmptyRequiredField);
                ctx.issue(
                    RunSection::Inflight,
                    Some(i),
                    Some("gauge"),
                    RunValidationIssueCode::EmptyRequiredField,
                    RunValidationSeverity::Error,
                    "in-flight gauge must not be blank",
                );
                None
            } else {
                Some(s.clone())
            }
        })
        .collect();
    if produce_run {
        normalized.inflight = inflight;
    }
    add_disps(&mut dispositions, RunSection::Inflight, &inflight_invalid);
    let runtime_invalid = vec![BTreeSet::new(); run.runtime_snapshots.len()];
    add_disps(
        &mut dispositions,
        RunSection::RuntimeSnapshots,
        &runtime_invalid,
    );
    NormalizedRun {
        run: normalized,
        report: ctx.sorted_report(),
        dispositions,
    }
}

#[derive(Clone)]
enum ParentState {
    UniqueRetained {
        precise_interval: Option<(u64, u64)>,
    },
    AmbiguousRetained,
    ExcludedOnly,
}

fn build_parent_states<'a>(
    run: &'a Run,
    request_invalid: &[BTreeSet<RunValidationIssueCode>],
    normalized_requests_by_input: &[Option<RequestEvent>],
) -> BTreeMap<&'a str, ParentState> {
    let mut retained_by_id = BTreeMap::<&str, Vec<(usize, RequestEvent)>>::new();
    let mut excluded_ids = BTreeSet::<&str>::new();
    for (i, r) in run.requests.iter().enumerate() {
        if let Some(normalized) = &normalized_requests_by_input[i] {
            retained_by_id
                .entry(r.request_id.as_str())
                .or_default()
                .push((i, normalized.clone()));
        } else if !request_invalid[i].is_empty() {
            excluded_ids.insert(r.request_id.as_str());
        }
    }

    let mut states = BTreeMap::new();
    for (id, retained) in retained_by_id {
        if retained.len() > 1 {
            states.insert(id, ParentState::AmbiguousRetained);
        } else {
            let (_, request) = retained.into_iter().next().expect("one retained request");
            let precise_interval = interval(request.started_at_run_us, request.finished_at_run_us);
            states.insert(id, ParentState::UniqueRetained { precise_interval });
        }
    }
    for id in excluded_ids {
        states.entry(id).or_insert(ParentState::ExcludedOnly);
    }
    states
}

fn interval(start: Option<u64>, end: Option<u64>) -> Option<(u64, u64)> {
    Some((start?, end?))
}
fn interval_stage(s: &StageEvent) -> Option<(u64, u64)> {
    interval(s.started_at_run_us, s.finished_at_run_us)
}
fn interval_queue(q: &QueueEvent) -> Option<(u64, u64)> {
    interval(q.waited_from_run_us, q.waited_until_run_us)
}
fn child_policy(
    ctx: &mut Ctx,
    invalid: &mut BTreeSet<RunValidationIssueCode>,
    section: RunSection,
    i: usize,
    id: &str,
    parents: &BTreeMap<&str, ParentState>,
    child: Option<(u64, u64)>,
) {
    let Some(parent_state) = parents.get(id) else {
        invalid.insert(RunValidationIssueCode::OrphanRequestScopedEvent);
        ctx.issue(
            section,
            Some(i),
            Some("request_id"),
            RunValidationIssueCode::OrphanRequestScopedEvent,
            RunValidationSeverity::Error,
            "no retained parent request matches request_id",
        );
        return;
    };
    match parent_state {
        ParentState::AmbiguousRetained => {
            invalid.insert(RunValidationIssueCode::AmbiguousParentRequestId);
            ctx.issue(
                section,
                Some(i),
                Some("request_id"),
                RunValidationIssueCode::AmbiguousParentRequestId,
                RunValidationSeverity::Error,
                "parent `request_id` is duplicated",
            );
        }
        ParentState::ExcludedOnly => {
            invalid.insert(RunValidationIssueCode::ParentRequestExcluded);
            ctx.issue(
                section,
                Some(i),
                Some("request_id"),
                RunValidationIssueCode::ParentRequestExcluded,
                RunValidationSeverity::Error,
                "parent request was excluded",
            );
        }
        ParentState::UniqueRetained { precise_interval } => {
            if let (Some((ps, pe)), Some((cs, ce))) = (precise_interval, child) {
                if cs < *ps || ce > *pe {
                    invalid.insert(RunValidationIssueCode::ChildIntervalOutsideRequest);
                    ctx.issue(
                        section,
                        Some(i),
                        None,
                        RunValidationIssueCode::ChildIntervalOutsideRequest,
                        RunValidationSeverity::Error,
                        "child interval lies outside parent request interval",
                    );
                }
            }
        }
    }
}
fn add_disps(
    out: &mut Vec<RunEventDisposition>,
    section: RunSection,
    invalids: &[BTreeSet<RunValidationIssueCode>],
) {
    let mut output_index = 0;
    for (i, codes) in invalids.iter().enumerate() {
        let disposition = if codes.is_empty() {
            let retained = RunEventDispositionKind::Retained { output_index };
            output_index += 1;
            retained
        } else {
            RunEventDispositionKind::Excluded {
                issue_codes: codes.iter().copied().collect(),
            }
        };
        out.push(RunEventDisposition {
            section,
            input_index: i,
            disposition,
        });
    }
}

fn inspect_required(
    ctx: &mut Ctx,
    invalid: &mut BTreeSet<RunValidationIssueCode>,
    section: RunSection,
    i: usize,
    field: &'static str,
    v: &str,
) {
    if v.trim().is_empty() {
        invalid.insert(RunValidationIssueCode::EmptyRequiredField);
        ctx.issue(
            section,
            Some(i),
            Some(field),
            RunValidationIssueCode::EmptyRequiredField,
            RunValidationSeverity::Error,
            format!("{field} must not be blank"),
        );
    }
}
fn inspect_time(
    ctx: &mut Ctx,
    invalid: &mut BTreeSet<RunValidationIssueCode>,
    precision_clear: &mut BTreeSet<RunValidationIssueCode>,
    section: RunSection,
    i: usize,
    coarse_start: u64,
    coarse_end: u64,
    field: &'static str,
    start: Option<u64>,
    end: Option<u64>,
    duration: u64,
) {
    if coarse_end < coarse_start {
        invalid.insert(RunValidationIssueCode::InvertedInterval);
        ctx.issue(
            section,
            Some(i),
            Some(field),
            RunValidationIssueCode::InvertedInterval,
            RunValidationSeverity::Error,
            "wall-clock end is before start",
        );
    }
    match (start, end) {
        (None, None) => ctx.issue(
            section,
            Some(i),
            None,
            RunValidationIssueCode::PreciseIntervalValidationUnavailable,
            RunValidationSeverity::Warning,
            "run-relative interval is unavailable",
        ),
        (Some(_), None) | (None, Some(_)) => {
            precision_clear.insert(RunValidationIssueCode::PartialRunRelativeInterval);
            ctx.issue(
                section,
                Some(i),
                None,
                RunValidationIssueCode::PartialRunRelativeInterval,
                RunValidationSeverity::Error,
                "run-relative interval is partial",
            );
        }
        (Some(s), Some(e)) => {
            if e < s {
                precision_clear.insert(RunValidationIssueCode::InvertedInterval);
                ctx.issue(
                    section,
                    Some(i),
                    None,
                    RunValidationIssueCode::InvertedInterval,
                    RunValidationSeverity::Error,
                    "run-relative end is before start",
                );
            } else if duration.abs_diff(e - s) > RUN_RELATIVE_DURATION_TOLERANCE_US {
                precision_clear.insert(RunValidationIssueCode::DurationMismatch);
                ctx.issue(
                    section,
                    Some(i),
                    None,
                    RunValidationIssueCode::DurationMismatch,
                    RunValidationSeverity::Error,
                    "duration differs from run-relative interval beyond tolerance",
                );
            }
        }
    }
}
fn inspect_request(
    ctx: &mut Ctx,
    invalid: &mut BTreeSet<RunValidationIssueCode>,
    precision_clear: &mut BTreeSet<RunValidationIssueCode>,
    i: usize,
    r: &RequestEvent,
) {
    inspect_required(
        ctx,
        invalid,
        RunSection::Requests,
        i,
        "request_id",
        &r.request_id,
    );
    inspect_required(ctx, invalid, RunSection::Requests, i, "route", &r.route);
    inspect_required(ctx, invalid, RunSection::Requests, i, "outcome", &r.outcome);
    inspect_time(
        ctx,
        invalid,
        precision_clear,
        RunSection::Requests,
        i,
        r.started_at_unix_ms,
        r.finished_at_unix_ms,
        "finished_at_unix_ms",
        r.started_at_run_us,
        r.finished_at_run_us,
        r.latency_us,
    );
}
fn inspect_stage(
    ctx: &mut Ctx,
    invalid: &mut BTreeSet<RunValidationIssueCode>,
    precision_clear: &mut BTreeSet<RunValidationIssueCode>,
    i: usize,
    s: &StageEvent,
) {
    inspect_required(
        ctx,
        invalid,
        RunSection::Stages,
        i,
        "request_id",
        &s.request_id,
    );
    inspect_required(ctx, invalid, RunSection::Stages, i, "stage", &s.stage);
    inspect_time(
        ctx,
        invalid,
        precision_clear,
        RunSection::Stages,
        i,
        s.started_at_unix_ms,
        s.finished_at_unix_ms,
        "finished_at_unix_ms",
        s.started_at_run_us,
        s.finished_at_run_us,
        s.latency_us,
    );
}
fn inspect_queue(
    ctx: &mut Ctx,
    invalid: &mut BTreeSet<RunValidationIssueCode>,
    precision_clear: &mut BTreeSet<RunValidationIssueCode>,
    i: usize,
    q: &QueueEvent,
) {
    inspect_required(
        ctx,
        invalid,
        RunSection::Queues,
        i,
        "request_id",
        &q.request_id,
    );
    inspect_required(ctx, invalid, RunSection::Queues, i, "queue", &q.queue);
    inspect_time(
        ctx,
        invalid,
        precision_clear,
        RunSection::Queues,
        i,
        q.waited_from_unix_ms,
        q.waited_until_unix_ms,
        "waited_until_unix_ms",
        q.waited_from_run_us,
        q.waited_until_run_us,
        q.wait_us,
    );
}
fn clear_bad_request(
    r: &RequestEvent,
    precision_clear: &BTreeSet<RunValidationIssueCode>,
) -> RequestEvent {
    let mut r = r.clone();
    if !precision_clear.is_empty() {
        r.started_at_run_us = None;
        r.finished_at_run_us = None;
    }
    r
}
fn clear_bad_stage(
    s: &StageEvent,
    precision_clear: &BTreeSet<RunValidationIssueCode>,
) -> StageEvent {
    let mut s = s.clone();
    if !precision_clear.is_empty() {
        s.started_at_run_us = None;
        s.finished_at_run_us = None;
    }
    s
}
fn clear_bad_queue(
    q: &QueueEvent,
    precision_clear: &BTreeSet<RunValidationIssueCode>,
) -> QueueEvent {
    let mut q = q.clone();
    if !precision_clear.is_empty() {
        q.waited_from_run_us = None;
        q.waited_until_run_us = None;
    }
    q
}

/// Returns whether a request event is an impossible single event for push-time validation.
pub(crate) fn validate_request_shape(event: &RequestEvent) -> Result<(), (&'static str, String)> {
    if let (Some(start), Some(end)) = (event.started_at_run_us, event.finished_at_run_us) {
        if end < start {
            return Err(("finished_at_run_us", "must be >= started_at_run_us".into()));
        }
    }
    if event.request_id.trim().is_empty() {
        return Err(("request_id", "must not be empty".into()));
    }
    if event.route.trim().is_empty() {
        return Err(("route", "must not be empty".into()));
    }
    if event.finished_at_unix_ms < event.started_at_unix_ms {
        return Err((
            "finished_at_unix_ms",
            "must be >= started_at_unix_ms".into(),
        ));
    }
    if event.outcome.trim().is_empty() {
        return Err(("outcome", "must not be empty".into()));
    }
    Ok(())
}
/// Returns whether a stage event is an impossible single event for push-time validation.
pub(crate) fn validate_stage_shape(event: &StageEvent) -> Result<(), (&'static str, String)> {
    if let (Some(start), Some(end)) = (event.started_at_run_us, event.finished_at_run_us) {
        if end < start {
            return Err(("finished_at_run_us", "must be >= started_at_run_us".into()));
        }
    }
    if event.request_id.trim().is_empty() {
        return Err(("request_id", "must not be empty".into()));
    }
    if event.stage.trim().is_empty() {
        return Err(("stage", "must not be empty".into()));
    }
    if event.finished_at_unix_ms < event.started_at_unix_ms {
        return Err((
            "finished_at_unix_ms",
            "must be >= started_at_unix_ms".into(),
        ));
    }
    Ok(())
}
/// Returns whether a queue event is an impossible single event for push-time validation.
pub(crate) fn validate_queue_shape(event: &QueueEvent) -> Result<(), (&'static str, String)> {
    if let (Some(start), Some(end)) = (event.waited_from_run_us, event.waited_until_run_us) {
        if end < start {
            return Err((
                "waited_until_run_us",
                "must be >= waited_from_run_us".into(),
            ));
        }
    }
    if event.request_id.trim().is_empty() {
        return Err(("request_id", "must not be empty".into()));
    }
    if event.queue.trim().is_empty() {
        return Err(("queue", "must not be empty".into()));
    }
    if event.waited_until_unix_ms < event.waited_from_unix_ms {
        return Err((
            "waited_until_unix_ms",
            "must be >= waited_from_unix_ms".into(),
        ));
    }
    Ok(())
}
/// Returns whether an in-flight snapshot is an impossible single event for push-time validation.
pub(crate) fn validate_inflight_shape(
    snapshot: &InFlightSnapshot,
) -> Result<(), (&'static str, String)> {
    if snapshot.gauge.trim().is_empty() {
        return Err(("gauge", "must not be empty".into()));
    }
    Ok(())
}
