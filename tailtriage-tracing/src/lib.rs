#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

//! Tracing intake bridge types for tailtriage triage workflows.
//!
//! This crate provides semantic `tt.*` keys, typed [`SpanRecord`] intake,
//! and conversion to [`tailtriage_core::Run`] via [`run_from_span_records`].
//! The `jsonl` feature adds JSONL import APIs.
//! The `live` feature adds live in-memory recording APIs.
//! It does not implement OpenTelemetry/OTLP. It converts tracing evidence into
//! standard `tailtriage_core::Run` artifacts and does not introduce a
//! tracing-specific analyzer path; Run JSON and the existing analyzer remain the
//! center of the workflow.
//!
//! # Example
//!
//! ```
//! use tailtriage_tracing::{
//!     ImportOptions, SpanRecord, TT_KIND, TT_OUTCOME, TT_REQUEST_ID, TT_ROUTE,
//! };
//!
//! let record = SpanRecord::new("http.request", 1_700_000_000_000, 1_700_000_000_120)
//!     .field(TT_KIND, "request")
//!     .field(TT_REQUEST_ID, "req-42")
//!     .field(TT_ROUTE, "/checkout")
//!     .field(TT_OUTCOME, "ok");
//!
//! let options = ImportOptions::new("checkout-service").strict(false);
//! assert_eq!(record.name(), "http.request");
//! assert_eq!(options.service_name(), "checkout-service");
//! ```

mod convention;
mod error;
#[cfg(feature = "jsonl")]
mod jsonl;
#[cfg(feature = "live")]
mod recorder;
#[cfg(feature = "tokio")]
/// Optional Tokio runtime sampler coupling for tracing sessions.
pub mod tokio;
mod types;

use tailtriage_core::{
    normalize_run_permissive, summarize_run_validation, summarize_run_validation_lifecycle,
    validate_run_strict, EffectiveCoreConfig, NormalizedRun, QueueEvent, RequestEvent, Run,
    RunEventDispositionKind, RunMetadata, RunSection, RunValidationIssueCode,
    RunValidationSeverity, StageEvent, TruncationSummary, UnfinishedRequests,
};

pub use convention::{
    TT_DEPTH_AT_START, TT_KIND, TT_OUTCOME, TT_QUEUE, TT_REQUEST_ID, TT_ROUTE, TT_STAGE, TT_SUCCESS,
};
pub use error::ImportError;
#[cfg(feature = "jsonl")]
pub use jsonl::{
    import_jsonl_path, import_jsonl_path_with_mode, import_jsonl_reader,
    import_jsonl_reader_with_mode, JsonlParseMode,
};
#[cfg(feature = "live")]
pub use recorder::{
    RecorderLimits, TailtriageLayer, TracingIntakeSession, TracingIntakeSessionBuilder,
    TracingRecorder, TracingRecorderBuilder, DEFAULT_MAX_COMPLETED_CANDIDATE_SPANS,
    DEFAULT_MAX_OPEN_SPANS,
};
pub use types::{FieldValue, ImportOptions, ImportWarning, ImportedRun, SpanKind, SpanRecord};

/// Ensures a run is suitable for persisted Run JSON artifacts intended for CLI analysis.
///
/// # Errors
///
/// Returns [`ImportError::ZeroRequestArtifact`] when the run has no completed request events.
pub fn ensure_persistable_run_has_requests(run: &tailtriage_core::Run) -> Result<(), ImportError> {
    if run.requests.is_empty() {
        return Err(ImportError::ZeroRequestArtifact {
            guidance: persistable_zero_request_guidance(),
        });
    }
    Ok(())
}

#[allow(dead_code)]
pub(crate) fn ensure_persistable_run_with_warnings(
    run: &tailtriage_core::Run,
    warnings: &[ImportWarning],
) -> Result<(), ImportError> {
    if run.requests.is_empty() {
        let guidance = persistable_zero_request_guidance();
        let warning_messages = warnings
            .iter()
            .map(|w| w.message().to_owned())
            .collect::<Vec<_>>();
        if warning_messages.is_empty() {
            return Err(ImportError::ZeroRequestArtifact { guidance });
        }
        return Err(ImportError::ZeroRequestArtifactWithWarnings {
            guidance,
            warnings: warning_messages,
        });
    }
    ensure_persistable_run_has_requests(run)
}

pub(crate) fn persistable_zero_request_guidance() -> String {
    "tracing import produced zero request events; persisted Run JSON artifacts intended for tailtriage analyze require at least one completed tt.kind=\"request\" span with tt.request_id, tt.route, and explicit unix-ms timing fields (started_at_unix_ms/finished_at_unix_ms).".to_owned()
}

/// Converts in-memory tracing span records into a `tailtriage_core::Run`.
///
/// Spans without any `tt.*` fields are ignored silently. Spans with `tt.*`
/// fields but missing `tt.kind` are treated as malformed tailtriage input.
/// In non-strict mode, malformed `tt.*` spans are skipped and surfaced as
/// warnings. In strict mode, the first malformed `tt.*` span returns an
/// [`ImportError`].
/// # Errors
///
/// Returns [`ImportError::StrictViolation`] when `options.strict(true)` is set
/// and a tailtriage-tagged span is malformed or incomplete.
#[allow(clippy::too_many_lines, clippy::needless_pass_by_value)]
pub fn run_from_span_records<I>(
    spans: I,
    options: ImportOptions,
) -> Result<ImportedRun, ImportError>
where
    I: IntoIterator<Item = SpanRecord>,
{
    Ok(convert_span_records_with_provenance(spans, options)?.into_imported())
}

#[allow(clippy::too_many_lines, clippy::needless_pass_by_value)]
fn convert_span_records_with_provenance<I>(
    spans: I,
    options: ImportOptions,
) -> Result<ProvenanceImportedRun, ImportError>
where
    I: IntoIterator<Item = SpanRecord>,
{
    validate_service_name(options.service_name())?;
    let source_spans = spans
        .into_iter()
        .enumerate()
        .map(|(source_index, span)| SourceSpan { source_index, span })
        .collect::<Vec<_>>();
    let mut warnings = Vec::new();
    let mut parsed_requests = Vec::new();
    let mut parsed_stages = Vec::new();
    let mut parsed_queues = Vec::new();

    for source in &source_spans {
        let span = &source.span;
        let kind = match get_string_field_state(span, TT_KIND) {
            StringFieldState::Missing => {
                if span_has_tailtriage_field(span) {
                    strict_or_warn(
                        options.strict_mode(),
                        &mut warnings,
                        format!(
                            "missing required field '{TT_KIND}' in span '{}'",
                            span.name()
                        ),
                    )?;
                }
                continue;
            }
            StringFieldState::Value(kind) => {
                let Some(kind) = SpanKind::parse(kind) else {
                    strict_or_warn(
                        options.strict_mode(),
                        &mut warnings,
                        format!("unknown tt.kind '{kind}' in span '{}'", span.name()),
                    )?;
                    continue;
                };
                kind
            }
            StringFieldState::InvalidType => {
                strict_or_warn(
                    options.strict_mode(),
                    &mut warnings,
                    format!(
                        "invalid field '{TT_KIND}' in span '{}': expected string",
                        span.name()
                    ),
                )?;
                continue;
            }
        };

        if span.finished_at_unix_ms() < span.started_at_unix_ms() {
            strict_or_warn(
                options.strict_mode(),
                &mut warnings,
                format!(
                    "skipped span '{}' due to inverted timestamps: start={} finish={}",
                    span.name(),
                    span.started_at_unix_ms(),
                    span.finished_at_unix_ms()
                ),
            )?;
            continue;
        }

        match kind {
            SpanKind::Request => {
                let request_id =
                    required_string(span, TT_REQUEST_ID, options.strict_mode(), &mut warnings)?;
                let route = required_string(span, TT_ROUTE, options.strict_mode(), &mut warnings)?;
                if let (Some(request_id), Some(route)) = (request_id, route) {
                    let Some((outcome, outcome_defaulted)) =
                        parse_outcome(span, options.strict_mode(), &mut warnings)?
                    else {
                        continue;
                    };
                    let (started_at_run_us, finished_at_run_us) =
                        sanitized_run_relative_offsets(span);
                    parsed_requests.push(ParsedRequestEvent {
                        source_index: source.source_index,
                        event: RequestEvent {
                            request_id,
                            route,
                            kind: None,
                            started_at_unix_ms: span.started_at_unix_ms(),
                            started_at_run_us,
                            finished_at_unix_ms: span.finished_at_unix_ms(),
                            finished_at_run_us,
                            latency_us: elapsed_duration_us(
                                span,
                                started_at_run_us,
                                finished_at_run_us,
                            ),
                            outcome,
                        },
                        outcome_defaulted,
                    });
                }
            }
            SpanKind::Stage => {
                let request_id =
                    required_string(span, TT_REQUEST_ID, options.strict_mode(), &mut warnings)?;
                let stage = required_string(span, TT_STAGE, options.strict_mode(), &mut warnings)?;
                if let (Some(request_id), Some(stage)) = (request_id, stage) {
                    let success_field = parse_success(span, options.strict_mode(), &mut warnings)?;
                    let success = match success_field {
                        OptionalField::Missing => true,
                        OptionalField::Value(success) => success,
                        OptionalField::Invalid => continue,
                    };
                    let (started_at_run_us, finished_at_run_us) =
                        sanitized_run_relative_offsets(span);
                    parsed_stages.push(ParsedStageEvent {
                        source_index: source.source_index,
                        event: StageEvent {
                            request_id,
                            stage,
                            started_at_unix_ms: span.started_at_unix_ms(),
                            started_at_run_us,
                            finished_at_unix_ms: span.finished_at_unix_ms(),
                            finished_at_run_us,
                            latency_us: elapsed_duration_us(
                                span,
                                started_at_run_us,
                                finished_at_run_us,
                            ),
                            success,
                        },
                        success_defaulted: matches!(success_field, OptionalField::Missing),
                    });
                }
            }
            SpanKind::Queue => {
                let request_id =
                    required_string(span, TT_REQUEST_ID, options.strict_mode(), &mut warnings)?;
                let queue = required_string(span, TT_QUEUE, options.strict_mode(), &mut warnings)?;
                if let (Some(request_id), Some(queue)) = (request_id, queue) {
                    let depth_at_start =
                        match parse_depth_at_start(span, options.strict_mode(), &mut warnings)? {
                            OptionalField::Missing => None,
                            OptionalField::Value(depth) => Some(depth),
                            OptionalField::Invalid => continue,
                        };
                    let (waited_from_run_us, waited_until_run_us) =
                        sanitized_run_relative_offsets(span);
                    parsed_queues.push(ParsedQueueEvent {
                        source_index: source.source_index,
                        event: QueueEvent {
                            request_id,
                            queue,
                            waited_from_unix_ms: span.started_at_unix_ms(),
                            waited_from_run_us,
                            waited_until_unix_ms: span.finished_at_unix_ms(),
                            waited_until_run_us,
                            wait_us: elapsed_duration_us(
                                span,
                                waited_from_run_us,
                                waited_until_run_us,
                            ),
                            depth_at_start,
                        },
                    });
                }
            }
        }
    }
    let mode = options.mode_value();
    let capture_limits = options.resolved_capture_limits();

    let request_outcome_default_count = parsed_requests
        .iter()
        .take(capture_limits.max_requests)
        .filter(|request| request.outcome_defaulted)
        .count();
    if request_outcome_default_count > 0 {
        warnings.push(ImportWarning::new(format!("{request_outcome_default_count} request span(s) missing optional '{TT_OUTCOME}'; assumed 'ok'")));
    }
    let stage_success_default_count = parsed_stages
        .iter()
        .take(capture_limits.max_stages)
        .filter(|stage| stage.success_defaulted)
        .count();
    if stage_success_default_count > 0 {
        warnings.push(ImportWarning::new(format!("{stage_success_default_count} stage span(s) missing optional '{TT_SUCCESS}'; assumed true")));
    }

    let mut truncation = TruncationSummary::default();
    apply_retention_limit(
        &mut parsed_requests,
        capture_limits.max_requests,
        |dropped| truncation.dropped_requests = dropped,
    );
    apply_retention_limit(&mut parsed_stages, capture_limits.max_stages, |dropped| {
        truncation.dropped_stages = dropped;
    });
    apply_retention_limit(&mut parsed_queues, capture_limits.max_queues, |dropped| {
        truncation.dropped_queues = dropped;
    });
    truncation.limits_hit = truncation.dropped_requests > 0
        || truncation.dropped_stages > 0
        || truncation.dropped_queues > 0;

    let provenance =
        CandidateProvenance::from_candidates(&parsed_requests, &parsed_stages, &parsed_queues);
    let requests: Vec<RequestEvent> = parsed_requests
        .into_iter()
        .map(|request| request.event)
        .collect();
    let stages: Vec<StageEvent> = parsed_stages.into_iter().map(|stage| stage.event).collect();
    let queues: Vec<QueueEvent> = parsed_queues.into_iter().map(|queue| queue.event).collect();

    let (started_at_unix_ms, finished_at_unix_ms) =
        retained_event_time_bounds(&requests, &stages, &queues).unwrap_or_else(|| {
            let now = tailtriage_core::unix_time_ms();
            (now, now)
        });
    let explicit_run_id = options.run_id_ref().is_some();
    let run_id = options.run_id_ref().map_or_else(
        || format!("tracing-import-{started_at_unix_ms}-{finished_at_unix_ms}"),
        ToOwned::to_owned,
    );

    let candidate = Run {
        schema_version: tailtriage_core::SCHEMA_VERSION,
        metadata: RunMetadata {
            run_id,
            service_name: options.service_name().to_owned(),
            service_version: options.service_version_ref().map(ToOwned::to_owned),
            started_at_unix_ms,
            finished_at_unix_ms,
            finalized_at_unix_ms: Some(finished_at_unix_ms),
            mode,
            effective_core_config: Some(EffectiveCoreConfig {
                mode,
                capture_limits,
                strict_lifecycle: false,
            }),
            effective_tokio_sampler_config: None,
            host: None,
            pid: None,
            lifecycle_warnings: Vec::new(),
            unfinished_requests: UnfinishedRequests::default(),
            run_end_reason: None,
        },
        requests,
        stages,
        queues,
        inflight: Vec::new(),
        runtime_snapshots: Vec::new(),
        truncation,
    };

    if options.strict_mode() {
        validate_run_strict(&candidate).map_err(|err| strict_core_error(&err))?;
    }

    let normalized = normalize_run_permissive(&candidate);
    let source_outcomes = SourceOutcomes::from_normalized(&provenance, &normalized);
    let retained_sources = source_outcomes.retained_sources(&source_spans);
    let core_warnings = summarize_run_validation(&normalized);
    let lifecycle_warnings = summarize_run_validation_lifecycle(&normalized);
    let mut run = normalized.run.clone();
    refresh_normalized_metadata_bounds(&mut run, explicit_run_id);
    for warning in core_warnings {
        if !warnings
            .iter()
            .any(|existing| existing.message() == warning)
        {
            warnings.push(ImportWarning::new(warning));
        }
    }
    attach_durable_conversion_warnings(&mut run, &warnings);
    for warning in lifecycle_warnings {
        if !run.metadata.lifecycle_warnings.contains(&warning) {
            run.metadata.lifecycle_warnings.push(warning);
        }
    }

    Ok(ProvenanceImportedRun {
        imported: ImportedRun::new(run, warnings),
        normalized,
        candidate_provenance: provenance,
        source_outcomes,
        retained_sources,
    })
}

#[derive(Debug, Clone)]
struct SourceSpan {
    source_index: usize,
    span: SpanRecord,
}

#[derive(Debug)]
struct ProvenanceImportedRun {
    imported: ImportedRun,
    normalized: NormalizedRun,
    candidate_provenance: CandidateProvenance,
    source_outcomes: SourceOutcomes,
    retained_sources: Vec<SpanRecord>,
}

impl ProvenanceImportedRun {
    fn into_imported(self) -> ImportedRun {
        let _ = (
            &self.normalized,
            &self.candidate_provenance,
            &self.source_outcomes,
            &self.retained_sources,
        );
        self.imported
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CandidateSource {
    section: RunSection,
    input_index: usize,
    source_index: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CandidateProvenance {
    inputs: Vec<CandidateSource>,
}

impl CandidateProvenance {
    fn from_candidates(
        requests: &[ParsedRequestEvent],
        stages: &[ParsedStageEvent],
        queues: &[ParsedQueueEvent],
    ) -> Self {
        let request_inputs =
            requests
                .iter()
                .enumerate()
                .map(|(input_index, request)| CandidateSource {
                    section: RunSection::Requests,
                    input_index,
                    source_index: request.source_index,
                });
        let stage_inputs = stages
            .iter()
            .enumerate()
            .map(|(input_index, stage)| CandidateSource {
                section: RunSection::Stages,
                input_index,
                source_index: stage.source_index,
            });
        let queue_inputs = queues
            .iter()
            .enumerate()
            .map(|(input_index, queue)| CandidateSource {
                section: RunSection::Queues,
                input_index,
                source_index: queue.source_index,
            });
        Self {
            inputs: request_inputs
                .chain(stage_inputs)
                .chain(queue_inputs)
                .collect(),
        }
    }

    fn source_index(&self, section: RunSection, input_index: usize) -> Option<usize> {
        self.inputs
            .iter()
            .find(|input| input.section == section && input.input_index == input_index)
            .map(|input| input.source_index)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SourceOutcome {
    source_index: usize,
    section: RunSection,
    input_index: usize,
    outcome: SourceOutcomeKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SourceOutcomeKind {
    Retained {
        output_index: usize,
    },
    Excluded {
        issue_codes: Vec<RunValidationIssueCode>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SourceOutcomes {
    outcomes: Vec<SourceOutcome>,
}

impl SourceOutcomes {
    fn from_normalized(provenance: &CandidateProvenance, normalized: &NormalizedRun) -> Self {
        let mut outcomes = normalized
            .dispositions
            .iter()
            .filter(|disposition| {
                matches!(
                    disposition.section,
                    RunSection::Requests | RunSection::Stages | RunSection::Queues
                )
            })
            .map(|disposition| {
                let source_index = provenance
                    .source_index(disposition.section, disposition.input_index)
                    .expect("core disposition must join to one tracing candidate source");
                let outcome = match &disposition.disposition {
                    RunEventDispositionKind::Retained { output_index } => {
                        SourceOutcomeKind::Retained {
                            output_index: *output_index,
                        }
                    }
                    RunEventDispositionKind::Excluded { issue_codes } => {
                        SourceOutcomeKind::Excluded {
                            issue_codes: issue_codes.clone(),
                        }
                    }
                };
                SourceOutcome {
                    source_index,
                    section: disposition.section,
                    input_index: disposition.input_index,
                    outcome,
                }
            })
            .collect::<Vec<_>>();
        outcomes.sort_by_key(source_outcome_sort_key);
        Self { outcomes }
    }

    fn retained_sources(&self, source_spans: &[SourceSpan]) -> Vec<SpanRecord> {
        self.outcomes
            .iter()
            .filter(|outcome| matches!(outcome.outcome, SourceOutcomeKind::Retained { .. }))
            .map(|outcome| {
                source_spans
                    .iter()
                    .find(|source| source.source_index == outcome.source_index)
                    .expect("retained source index must select one original span")
                    .span
                    .clone()
            })
            .collect()
    }
}

fn source_outcome_sort_key(outcome: &SourceOutcome) -> (usize, u8, usize) {
    let section = match outcome.section {
        RunSection::Requests => 0,
        RunSection::Stages => 1,
        RunSection::Queues => 2,
        _ => 3,
    };
    (outcome.source_index, section, outcome.input_index)
}

fn apply_retention_limit<T>(items: &mut Vec<T>, max: usize, set_dropped: impl FnOnce(u64)) {
    let dropped = items.len().saturating_sub(max) as u64;
    if items.len() > max {
        items.truncate(max);
    }
    set_dropped(dropped);
}

fn strict_core_error(err: &tailtriage_core::RunValidationError) -> ImportError {
    let mut codes = err
        .report()
        .issues
        .iter()
        .filter(|issue| issue.severity == RunValidationSeverity::Error)
        .map(|issue| issue.code)
        .collect::<Vec<_>>();
    codes.sort_unstable();
    codes.dedup();
    let labels = codes
        .iter()
        .map(|code| code.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    ImportError::StrictViolation(format!(
        "core run validation failed with issue codes [{labels}]: {err}"
    ))
}

fn refresh_normalized_metadata_bounds(run: &mut Run, explicit_run_id: bool) {
    if let Some((started_at_unix_ms, finished_at_unix_ms)) =
        retained_event_time_bounds(&run.requests, &run.stages, &run.queues)
    {
        run.metadata.started_at_unix_ms = started_at_unix_ms;
        run.metadata.finished_at_unix_ms = finished_at_unix_ms;
        run.metadata.finalized_at_unix_ms = Some(finished_at_unix_ms);
        if !explicit_run_id {
            run.metadata.run_id =
                format!("tracing-import-{started_at_unix_ms}-{finished_at_unix_ms}");
        }
    }
}

struct ParsedStageEvent {
    source_index: usize,
    event: StageEvent,
    success_defaulted: bool,
}

struct ParsedRequestEvent {
    source_index: usize,
    event: RequestEvent,
    outcome_defaulted: bool,
}

struct ParsedQueueEvent {
    source_index: usize,
    event: QueueEvent,
}

fn validate_service_name(service_name: &str) -> Result<(), ImportError> {
    if service_name.trim().is_empty() {
        return Err(ImportError::EmptyServiceName);
    }
    Ok(())
}

fn retained_event_time_bounds(
    requests: &[RequestEvent],
    stages: &[StageEvent],
    queues: &[QueueEvent],
) -> Option<(u64, u64)> {
    let request_bounds = requests
        .iter()
        .map(|request| (request.started_at_unix_ms, request.finished_at_unix_ms));
    let stage_bounds = stages
        .iter()
        .map(|stage| (stage.started_at_unix_ms, stage.finished_at_unix_ms));
    let queue_bounds = queues
        .iter()
        .map(|queue| (queue.waited_from_unix_ms, queue.waited_until_unix_ms));
    request_bounds.chain(stage_bounds).chain(queue_bounds).fold(
        None,
        |acc: Option<(u64, u64)>, (start, finish)| {
            Some(match acc {
                Some((min_start, max_finish)) => (min_start.min(start), max_finish.max(finish)),
                None => (start, finish),
            })
        },
    )
}

enum StringFieldState<'a> {
    Missing,
    Value(&'a str),
    InvalidType,
}

enum OptionalField<T> {
    Missing,
    Value(T),
    Invalid,
}

fn get_string_field_state<'a>(span: &'a SpanRecord, key: &str) -> StringFieldState<'a> {
    match span.fields().get(key) {
        Some(FieldValue::String(value)) => StringFieldState::Value(value.as_str()),
        Some(_) => StringFieldState::InvalidType,
        None => StringFieldState::Missing,
    }
}

fn span_has_tailtriage_field(span: &SpanRecord) -> bool {
    span.fields().keys().any(|key| key.starts_with("tt."))
}

fn required_string(
    span: &SpanRecord,
    key: &'static str,
    strict: bool,
    warnings: &mut Vec<ImportWarning>,
) -> Result<Option<String>, ImportError> {
    match get_string_field_state(span, key) {
        StringFieldState::Value(value) => {
            if value.trim().is_empty() {
                strict_or_warn(
                    strict,
                    warnings,
                    format!(
                        "invalid field '{key}' in span '{}': required string cannot be empty or whitespace",
                        span.name()
                    ),
                )?;
                Ok(None)
            } else {
                Ok(Some(value.to_owned()))
            }
        }
        StringFieldState::Missing => {
            strict_or_warn(
                strict,
                warnings,
                format!("missing required field '{key}' in span '{}'", span.name()),
            )?;
            Ok(None)
        }
        StringFieldState::InvalidType => {
            strict_or_warn(
                strict,
                warnings,
                format!(
                    "invalid field '{key}' in span '{}': expected string",
                    span.name()
                ),
            )?;
            Ok(None)
        }
    }
}

#[cfg(feature = "live")]
pub(crate) fn duration_within_derived_tolerance(duration_us: u64, derived_us: u64) -> bool {
    duration_us.abs_diff(derived_us) <= tailtriage_core::RUN_RELATIVE_DURATION_TOLERANCE_US
}

#[cfg(feature = "live")]
pub(crate) fn duration_within_tolerance(
    duration_us: u64,
    started_at_unix_ms: u64,
    finished_at_unix_ms: u64,
) -> bool {
    let derived_us = timestamp_derived_duration_us(started_at_unix_ms, finished_at_unix_ms);
    duration_within_derived_tolerance(duration_us, derived_us)
}

fn timestamp_derived_duration_us(started_at_unix_ms: u64, finished_at_unix_ms: u64) -> u64 {
    finished_at_unix_ms
        .saturating_sub(started_at_unix_ms)
        .saturating_mul(1000)
}

fn sanitized_run_relative_offsets(span: &SpanRecord) -> (Option<u64>, Option<u64>) {
    (span.started_at_run_us_ref(), span.finished_at_run_us_ref())
}

fn run_relative_derived_duration_us(
    started_at_run_us: Option<u64>,
    finished_at_run_us: Option<u64>,
) -> Option<u64> {
    let started_at_run_us = started_at_run_us?;
    let finished_at_run_us = finished_at_run_us?;
    finished_at_run_us.checked_sub(started_at_run_us)
}

fn elapsed_duration_us(
    span: &SpanRecord,
    started_at_run_us: Option<u64>,
    finished_at_run_us: Option<u64>,
) -> u64 {
    if let Some(duration_us) = span.duration_us_ref() {
        return duration_us;
    }

    run_relative_derived_duration_us(started_at_run_us, finished_at_run_us).unwrap_or_else(|| {
        timestamp_derived_duration_us(span.started_at_unix_ms(), span.finished_at_unix_ms())
    })
}

fn is_durable_conversion_warning(message: &str) -> bool {
    message.starts_with("skipped ")
        || message.starts_with("missing required field")
        || message.starts_with("invalid field")
        || message.starts_with("unknown tt.kind")
        || message.contains("missing optional 'tt.outcome'; assumed 'ok'")
        || message.contains("missing optional 'tt.success'; assumed true")
}

fn attach_durable_conversion_warnings(run: &mut tailtriage_core::Run, warnings: &[ImportWarning]) {
    for warning in warnings {
        let message = warning.message();
        if is_durable_conversion_warning(message)
            && !run
                .metadata
                .lifecycle_warnings
                .iter()
                .any(|existing| existing == message)
        {
            run.metadata.lifecycle_warnings.push(message.to_owned());
        }
    }
}

#[cfg(test)]
mod persistable_tests {
    use super::ensure_persistable_run_has_requests;
    use crate::ImportError;
    use tailtriage_core::{MemorySink, RequestEvent, Tailtriage};

    #[test]
    fn ensure_persistable_run_has_requests_accepts_non_empty_runs() {
        let collector = Tailtriage::builder("svc")
            .sink(MemorySink::new())
            .build()
            .unwrap();
        let mut run = collector.snapshot();
        run.requests.push(RequestEvent {
            request_id: "r1".into(),
            route: "/".into(),
            kind: None,
            started_at_unix_ms: 1,
            started_at_run_us: None,
            finished_at_unix_ms: 2,
            finished_at_run_us: None,
            latency_us: 1_000,
            outcome: "ok".into(),
        });
        assert!(ensure_persistable_run_has_requests(&run).is_ok());
    }

    #[test]
    fn ensure_persistable_run_has_requests_rejects_empty_runs() {
        let run = Tailtriage::builder("svc")
            .sink(MemorySink::new())
            .build()
            .unwrap()
            .snapshot();
        let err = ensure_persistable_run_has_requests(&run).unwrap_err();
        assert!(matches!(err, ImportError::ZeroRequestArtifact { .. }));
    }
}

fn strict_or_warn(
    strict: bool,
    warnings: &mut Vec<ImportWarning>,
    message: String,
) -> Result<(), ImportError> {
    if strict {
        return Err(ImportError::StrictViolation(message));
    }
    warnings.push(ImportWarning::new(message));
    Ok(())
}

#[cfg(test)]
const RECOMMENDED_OUTCOME_LABELS: [&str; 5] = ["ok", "error", "timeout", "cancelled", "rejected"];

fn parse_outcome(
    span: &SpanRecord,
    strict: bool,
    warnings: &mut Vec<ImportWarning>,
) -> Result<Option<(String, bool)>, ImportError> {
    match get_string_field_state(span, TT_OUTCOME) {
        StringFieldState::Missing => Ok(Some(("ok".to_owned(), true))),
        StringFieldState::Value(value) => {
            if value.trim().is_empty() {
                strict_or_warn(
                    strict,
                    warnings,
                    format!(
                        "invalid field '{TT_OUTCOME}' in span '{}': expected non-empty, non-whitespace string",
                        span.name()
                    ),
                )?;
                Ok(None)
            } else {
                Ok(Some((value.to_owned(), false)))
            }
        }
        StringFieldState::InvalidType => {
            strict_or_warn(
                strict,
                warnings,
                format!(
                    "invalid field '{TT_OUTCOME}' in span '{}': expected string",
                    span.name()
                ),
            )?;
            Ok(None)
        }
    }
}

fn parse_success(
    span: &SpanRecord,
    strict: bool,
    warnings: &mut Vec<ImportWarning>,
) -> Result<OptionalField<bool>, ImportError> {
    match span.fields().get(TT_SUCCESS) {
        Some(FieldValue::Bool(value)) => Ok(OptionalField::Value(*value)),
        Some(FieldValue::String(value)) if value.eq_ignore_ascii_case("true") => {
            Ok(OptionalField::Value(true))
        }
        Some(FieldValue::String(value)) if value.eq_ignore_ascii_case("false") => {
            Ok(OptionalField::Value(false))
        }
        Some(_) => {
            strict_or_warn(strict, warnings, format!("invalid field '{TT_SUCCESS}' in span '{}': expected bool or 'true'/'false' string", span.name()))?;
            Ok(OptionalField::Invalid)
        }
        None => Ok(OptionalField::Missing),
    }
}

fn parse_depth_at_start(
    span: &SpanRecord,
    strict: bool,
    warnings: &mut Vec<ImportWarning>,
) -> Result<OptionalField<u64>, ImportError> {
    match span.fields().get(TT_DEPTH_AT_START) {
        Some(FieldValue::U64(value)) => Ok(OptionalField::Value(*value)),
        Some(FieldValue::I64(value)) if *value >= 0 => {
            if let Ok(parsed) = u64::try_from(*value) {
                Ok(OptionalField::Value(parsed))
            } else {
                strict_or_warn(
                strict,
                warnings,
                format!(
                    "invalid field '{TT_DEPTH_AT_START}' in span '{}': expected non-negative integer",
                    span.name()
                ),
            )?;
                Ok(OptionalField::Invalid)
            }
        }
        Some(_) => {
            strict_or_warn(strict, warnings, format!("invalid field '{TT_DEPTH_AT_START}' in span '{}': expected non-negative integer", span.name()))?;
            Ok(OptionalField::Invalid)
        }
        None => Ok(OptionalField::Missing),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tailtriage_core::CaptureMode;

    #[test]
    fn span_kind_parser_accepts_supported_values_only() {
        assert_eq!(SpanKind::parse("request"), Some(SpanKind::Request));
        assert_eq!(SpanKind::parse("stage"), Some(SpanKind::Stage));
        assert_eq!(SpanKind::parse("queue"), Some(SpanKind::Queue));
        assert_eq!(SpanKind::parse("Request"), None);
        assert_eq!(SpanKind::parse("wat"), None);
    }

    fn opts() -> ImportOptions {
        ImportOptions::new("svc").strict(false).run_id("run")
    }

    fn req(id: &str, start: u64, finish: u64) -> SpanRecord {
        SpanRecord::new(format!("req-{id}"), start, finish)
            .field(TT_KIND, "request")
            .field(TT_REQUEST_ID, id)
            .field(TT_ROUTE, "/")
    }

    fn stage(id: &str, name: &str, start: u64, finish: u64) -> SpanRecord {
        SpanRecord::new(format!("stage-{name}"), start, finish)
            .field(TT_KIND, "stage")
            .field(TT_REQUEST_ID, id)
            .field(TT_STAGE, name)
    }

    fn queue(id: &str, name: &str, start: u64, finish: u64) -> SpanRecord {
        SpanRecord::new(format!("queue-{name}"), start, finish)
            .field(TT_KIND, "queue")
            .field(TT_REQUEST_ID, id)
            .field(TT_QUEUE, name)
    }

    fn retained_indices(result: &ProvenanceImportedRun) -> Vec<usize> {
        result
            .source_outcomes
            .outcomes
            .iter()
            .filter_map(|outcome| match outcome.outcome {
                SourceOutcomeKind::Retained { .. } => Some(outcome.source_index),
                SourceOutcomeKind::Excluded { .. } => None,
            })
            .collect()
    }

    fn excluded_codes(result: &ProvenanceImportedRun) -> Vec<(usize, Vec<RunValidationIssueCode>)> {
        result
            .source_outcomes
            .outcomes
            .iter()
            .filter_map(|outcome| match &outcome.outcome {
                SourceOutcomeKind::Excluded { issue_codes } => {
                    Some((outcome.source_index, issue_codes.clone()))
                }
                SourceOutcomeKind::Retained { .. } => None,
            })
            .collect()
    }

    #[test]
    fn provenance_maps_valid_request_stage_queue_source_indices() {
        let spans = vec![
            req("r", 100, 120),
            stage("r", "db", 105, 115),
            queue("r", "work", 101, 104),
        ];
        let result = convert_span_records_with_provenance(spans.clone(), opts()).unwrap();
        assert_eq!(
            result
                .candidate_provenance
                .inputs
                .iter()
                .map(|i| (i.section, i.input_index, i.source_index))
                .collect::<Vec<_>>(),
            vec![
                (RunSection::Requests, 0, 0),
                (RunSection::Stages, 0, 1),
                (RunSection::Queues, 0, 2)
            ]
        );
        assert_eq!(retained_indices(&result), vec![0, 1, 2]);
        assert_eq!(result.retained_sources, spans);
    }

    #[test]
    fn provenance_retains_original_source_when_optional_precision_is_missing_or_repaired() {
        let missing = vec![
            req("r", 100, 120),
            stage("r", "db", 105, 115),
            queue("r", "work", 101, 104),
        ];
        let missing_result = convert_span_records_with_provenance(missing.clone(), opts()).unwrap();
        assert_eq!(retained_indices(&missing_result), vec![0, 1, 2]);
        assert_eq!(missing_result.retained_sources, missing);

        let repaired_req = req("r", 100, 120)
            .started_at_run_us(0)
            .finished_at_run_us(10)
            .duration_us(20_000);
        let repaired_stage = stage("r", "db", 105, 115)
            .started_at_run_us(5_000)
            .duration_us(10_000);
        let repaired_queue = queue("r", "work", 106, 107)
            .started_at_run_us(6_000)
            .finished_at_run_us(6_500)
            .duration_us(10_000);
        let repaired = vec![repaired_req, repaired_stage, repaired_queue];
        let repaired_result =
            convert_span_records_with_provenance(repaired.clone(), opts()).unwrap();
        assert_eq!(retained_indices(&repaired_result), vec![0, 1, 2]);
        assert_eq!(repaired_result.retained_sources, repaired);
        assert_eq!(
            repaired_result.imported.run().requests[0].started_at_run_us,
            None
        );
        assert_eq!(
            repaired_result.imported.run().stages[0].started_at_run_us,
            None
        );
        assert_eq!(
            repaired_result.imported.run().queues[0].waited_from_run_us,
            None
        );
    }

    #[test]
    fn provenance_excludes_core_duplicate_ambiguous_orphan_parent_and_containment_cases() {
        let spans = vec![
            req("dup", 100, 120),
            req("dup", 130, 150),
            stage("dup", "ambiguous", 135, 140),
            stage("orphan", "orphan", 200, 210),
            req("bad", 300, 320)
                .started_at_run_us(0)
                .finished_at_run_us(20_000),
            stage("bad", "outside", 300, 330)
                .started_at_run_us(0)
                .finished_at_run_us(30_000),
        ];
        let result = convert_span_records_with_provenance(spans, opts()).unwrap();
        assert_eq!(retained_indices(&result), vec![4]);
        let excluded = excluded_codes(&result);
        assert!(excluded.iter().any(|(idx, codes)| *idx == 0
            && codes.contains(&RunValidationIssueCode::DuplicateCompletedRequestId)));
        assert!(excluded.iter().any(|(idx, codes)| *idx == 1
            && codes.contains(&RunValidationIssueCode::DuplicateCompletedRequestId)));
        assert!(excluded.iter().any(|(idx, codes)| *idx == 2
            && codes.contains(&RunValidationIssueCode::AmbiguousParentRequestId)));
        assert!(excluded.iter().any(|(idx, codes)| *idx == 3
            && codes.contains(&RunValidationIssueCode::OrphanRequestScopedEvent)));
        assert!(excluded.iter().any(|(idx, codes)| *idx == 5
            && codes.contains(&RunValidationIssueCode::ChildIntervalOutsideRequest)));
    }

    #[test]
    fn provenance_semantic_limits_preserve_mappings_and_never_revive_dropped_sources() {
        let spans = vec![
            req("r1", 100, 120),
            req("r2", 130, 150),
            stage("r1", "s1", 101, 102),
            stage("r1", "s2", 103, 104),
            queue("r1", "q1", 105, 106),
            queue("r1", "q2", 107, 108),
        ];
        let options = opts().capture_limits(tailtriage_core::CaptureLimits {
            max_requests: 1,
            max_stages: 1,
            max_queues: 1,
            ..tailtriage_core::CaptureLimits::default()
        });
        let result = convert_span_records_with_provenance(spans, options).unwrap();
        assert_eq!(
            result
                .candidate_provenance
                .inputs
                .iter()
                .map(|i| i.source_index)
                .collect::<Vec<_>>(),
            vec![0, 2, 4]
        );
        assert_eq!(retained_indices(&result), vec![0, 2, 4]);
        assert_eq!(result.imported.run().truncation.dropped_requests, 1);
        assert_eq!(result.imported.run().truncation.dropped_stages, 1);
        assert_eq!(result.imported.run().truncation.dropped_queues, 1);
    }

    #[test]
    fn provenance_ordering_issue_codes_repeatability_and_public_equivalence_are_deterministic() {
        let spans = vec![
            stage("orphan", "first", 10, 11),
            req("r", 100, 120),
            queue("r", "q", 101, 102),
            stage("r", "s", 103, 104),
        ];
        let a = convert_span_records_with_provenance(spans.clone(), opts()).unwrap();
        let b = convert_span_records_with_provenance(spans.clone(), opts()).unwrap();
        assert_eq!(a.imported.run(), b.imported.run());
        assert_eq!(
            a.imported
                .warnings()
                .iter()
                .map(ImportWarning::message)
                .collect::<Vec<_>>(),
            b.imported
                .warnings()
                .iter()
                .map(ImportWarning::message)
                .collect::<Vec<_>>()
        );
        assert_eq!(a.normalized, b.normalized);
        assert_eq!(a.candidate_provenance, b.candidate_provenance);
        assert_eq!(a.source_outcomes, b.source_outcomes);
        assert_eq!(a.retained_sources, b.retained_sources);
        assert_eq!(
            a.source_outcomes
                .outcomes
                .iter()
                .map(|o| o.source_index)
                .collect::<Vec<_>>(),
            vec![0, 1, 2, 3]
        );
        assert_eq!(
            excluded_codes(&a),
            vec![(0, vec![RunValidationIssueCode::OrphanRequestScopedEvent])]
        );
        let public = run_from_span_records(spans, opts()).unwrap();
        assert_eq!(public.run(), a.imported.run());
        assert_eq!(
            public
                .warnings()
                .iter()
                .map(ImportWarning::message)
                .collect::<Vec<_>>(),
            a.imported
                .warnings()
                .iter()
                .map(ImportWarning::message)
                .collect::<Vec<_>>()
        );
    }
    #[test]
    fn request_only_conversion_creates_one_request_event() {
        let spans = vec![SpanRecord::new("req", 100, 110)
            .field(TT_KIND, "request")
            .field(TT_REQUEST_ID, "r1")
            .field(TT_ROUTE, "/a")];
        let imported = run_from_span_records(spans, ImportOptions::new("svc")).expect("ok");
        assert_eq!(imported.run().requests.len(), 1);
    }

    #[test]
    fn request_and_stage_convert() {
        let spans = vec![
            SpanRecord::new("req", 100, 120)
                .started_at_run_us(100_000)
                .finished_at_run_us(120_000)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_ROUTE, "/a"),
            SpanRecord::new("st", 105, 115)
                .field(TT_KIND, "stage")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_STAGE, "db"),
        ];
        let run = run_from_span_records(spans, ImportOptions::new("svc"))
            .unwrap()
            .run()
            .clone();
        assert_eq!(run.requests.len(), 1);
        assert_eq!(run.stages.len(), 1);
    }

    #[test]
    fn request_and_queue_convert() {
        let spans = vec![
            SpanRecord::new("req", 100, 120)
                .started_at_run_us(100_000)
                .finished_at_run_us(120_000)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_ROUTE, "/a"),
            SpanRecord::new("q", 102, 103)
                .field(TT_KIND, "queue")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_QUEUE, "permits"),
        ];
        let run = run_from_span_records(spans, ImportOptions::new("svc"))
            .unwrap()
            .run()
            .clone();
        assert_eq!(run.queues.len(), 1);
    }

    #[test]
    fn span_record_run_relative_fields_convert_to_core_events() {
        let spans = vec![
            SpanRecord::new("req", 100, 120)
                .started_at_run_us(10)
                .finished_at_run_us(20_010)
                .duration_us(20_000)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_ROUTE, "/a"),
            SpanRecord::new("st", 105, 115)
                .started_at_run_us(5_010)
                .finished_at_run_us(15_010)
                .duration_us(10_000)
                .field(TT_KIND, "stage")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_STAGE, "db"),
            SpanRecord::new("q", 102, 103)
                .started_at_run_us(2_010)
                .finished_at_run_us(3_010)
                .duration_us(1_000)
                .field(TT_KIND, "queue")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_QUEUE, "permits"),
        ];
        let run = run_from_span_records(spans, ImportOptions::new("svc"))
            .unwrap()
            .run()
            .clone();

        assert_eq!(run.requests[0].started_at_run_us, Some(10));
        assert_eq!(run.requests[0].finished_at_run_us, Some(20_010));
        assert_eq!(run.stages[0].started_at_run_us, Some(5_010));
        assert_eq!(run.stages[0].finished_at_run_us, Some(15_010));
        assert_eq!(run.queues[0].waited_from_run_us, Some(2_010));
        assert_eq!(run.queues[0].waited_until_run_us, Some(3_010));
    }

    #[test]
    fn missing_duration_uses_run_relative_delta_before_unix_bounds() {
        let spans = vec![SpanRecord::new("req", 10, 11)
            .started_at_run_us(1_000)
            .finished_at_run_us(51_000)
            .field(TT_KIND, "request")
            .field(TT_REQUEST_ID, "r1")
            .field(TT_ROUTE, "/a")];

        let imported = run_from_span_records(spans, ImportOptions::new("svc")).unwrap();

        assert_eq!(imported.run().requests[0].latency_us, 50_000);
    }

    #[test]
    fn strict_duration_matching_run_relative_allows_wall_clock_mismatch() {
        let spans = vec![SpanRecord::new("req", 10, 11)
            .started_at_run_us(1_000)
            .finished_at_run_us(51_000)
            .duration_us(50_000)
            .field(TT_KIND, "request")
            .field(TT_REQUEST_ID, "r1")
            .field(TT_ROUTE, "/a")];

        let imported =
            run_from_span_records(spans, ImportOptions::new("svc").strict(true)).unwrap();

        assert_eq!(imported.run().requests[0].latency_us, 50_000);
    }

    #[test]
    fn strict_duration_mismatching_run_relative_fails_even_if_unix_matches() {
        let spans = vec![SpanRecord::new("req", 10, 60)
            .started_at_run_us(1_000)
            .finished_at_run_us(11_000)
            .duration_us(50_000)
            .field(TT_KIND, "request")
            .field(TT_REQUEST_ID, "r1")
            .field(TT_ROUTE, "/a")];

        let err = run_from_span_records(spans, ImportOptions::new("svc").strict(true))
            .expect_err("strict import should reject duration/run-relative mismatch");

        let message = err.to_string();
        assert!(message.contains("duration_mismatch"));
    }

    #[test]
    fn non_strict_duration_mismatching_run_relative_warns_and_retains_duration() {
        let spans = vec![SpanRecord::new("req", 10, 60)
            .started_at_run_us(1_000)
            .finished_at_run_us(11_000)
            .duration_us(50_000)
            .field(TT_KIND, "request")
            .field(TT_REQUEST_ID, "r1")
            .field(TT_ROUTE, "/a")];

        let imported = run_from_span_records(spans, ImportOptions::new("svc")).unwrap();

        assert_eq!(imported.run().requests[0].latency_us, 50_000);
        assert!(imported
            .warnings()
            .iter()
            .any(|w| w.message().contains("duration_mismatch")));
        assert!(imported
            .run()
            .metadata
            .lifecycle_warnings
            .iter()
            .any(|w| w.contains("duration_mismatch")));
    }

    #[test]
    fn non_strict_inverted_request_run_relative_offsets_are_dropped_with_warning() {
        let spans = vec![SpanRecord::new("req", 100, 120)
            .started_at_run_us(20_000)
            .finished_at_run_us(10_000)
            .field(TT_KIND, "request")
            .field(TT_REQUEST_ID, "r1")
            .field(TT_ROUTE, "/a")];

        let imported = run_from_span_records(spans, ImportOptions::new("svc")).unwrap();

        assert_eq!(imported.run().requests.len(), 1);
        assert_eq!(imported.run().requests[0].started_at_run_us, None);
        assert_eq!(imported.run().requests[0].finished_at_run_us, None);
        assert_eq!(imported.run().requests[0].latency_us, 20_000);
        assert!(imported
            .warnings()
            .iter()
            .any(|w| w.message().contains("inverted_interval")));
        assert!(imported
            .warnings()
            .iter()
            .any(|w| w.message().contains("optional run-relative offsets")));
    }

    #[test]
    fn strict_inverted_request_run_relative_offsets_fail() {
        let spans = vec![SpanRecord::new("req", 100, 120)
            .started_at_run_us(20_000)
            .finished_at_run_us(10_000)
            .field(TT_KIND, "request")
            .field(TT_REQUEST_ID, "r1")
            .field(TT_ROUTE, "/a")];

        let err = run_from_span_records(spans, ImportOptions::new("svc").strict(true))
            .expect_err("strict import should reject inverted run-relative offsets");

        assert!(matches!(err, ImportError::StrictViolation(_)));
        assert!(err.to_string().contains("inverted_interval"));
    }

    #[test]
    fn non_strict_incomplete_request_run_relative_offsets_are_dropped_with_warning() {
        let spans = vec![SpanRecord::new("req", 100, 120)
            .started_at_run_us(10_000)
            .field(TT_KIND, "request")
            .field(TT_REQUEST_ID, "r1")
            .field(TT_ROUTE, "/a")];

        let imported = run_from_span_records(spans, ImportOptions::new("svc")).unwrap();

        assert_eq!(imported.run().requests.len(), 1);
        assert_eq!(imported.run().requests[0].started_at_run_us, None);
        assert_eq!(imported.run().requests[0].finished_at_run_us, None);
        assert!(imported
            .warnings()
            .iter()
            .any(|w| w.message().contains("partial_run_relative_interval")));
        assert!(imported
            .warnings()
            .iter()
            .any(|w| w.message().contains("optional run-relative offsets")));
    }

    #[test]
    fn non_strict_inverted_queue_run_relative_offsets_are_dropped_with_warning() {
        let spans = vec![
            SpanRecord::new("req", 100, 120)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_ROUTE, "/a"),
            SpanRecord::new("q", 102, 103)
                .started_at_run_us(3_000)
                .finished_at_run_us(2_000)
                .field(TT_KIND, "queue")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_QUEUE, "permits"),
        ];

        let imported = run_from_span_records(spans, ImportOptions::new("svc")).unwrap();

        assert_eq!(imported.run().queues.len(), 1);
        assert_eq!(imported.run().queues[0].waited_from_run_us, None);
        assert_eq!(imported.run().queues[0].waited_until_run_us, None);
        assert_eq!(imported.run().queues[0].wait_us, 1_000);
        assert!(imported
            .warnings()
            .iter()
            .any(|w| w.message().contains("inverted_interval")));
    }

    #[test]
    fn non_strict_inverted_stage_run_relative_without_duration_uses_coarse_delta() {
        let spans = vec![
            SpanRecord::new("req", 100, 120)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_ROUTE, "/a"),
            SpanRecord::new("stage", 102, 105)
                .started_at_run_us(5_000)
                .finished_at_run_us(2_000)
                .field(TT_KIND, "stage")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_STAGE, "db"),
        ];

        let imported = run_from_span_records(spans, ImportOptions::new("svc")).unwrap();

        assert_eq!(imported.run().stages.len(), 1);
        assert_eq!(imported.run().stages[0].started_at_run_us, None);
        assert_eq!(imported.run().stages[0].finished_at_run_us, None);
        assert_eq!(imported.run().stages[0].latency_us, 3_000);
        assert!(imported
            .warnings()
            .iter()
            .any(|w| w.message().contains("inverted_interval")));
        assert!(imported
            .warnings()
            .iter()
            .any(|w| w.message().contains("optional run-relative offsets")));
    }

    #[test]
    fn strict_inverted_queue_run_relative_offsets_fail() {
        let spans = vec![
            SpanRecord::new("req", 100, 120)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_ROUTE, "/a"),
            SpanRecord::new("q", 102, 103)
                .started_at_run_us(3_000)
                .finished_at_run_us(2_000)
                .field(TT_KIND, "queue")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_QUEUE, "permits"),
        ];

        let err = run_from_span_records(spans, ImportOptions::new("svc").strict(true))
            .expect_err("strict import should reject inverted queue run-relative offsets");

        assert!(matches!(err, ImportError::StrictViolation(_)));
        assert!(err.to_string().contains("inverted_interval"));
    }

    #[test]
    fn non_strict_incomplete_queue_run_relative_offsets_are_dropped_with_warning() {
        let spans = vec![
            SpanRecord::new("req", 100, 120)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_ROUTE, "/a"),
            SpanRecord::new("q", 102, 103)
                .finished_at_run_us(3_000)
                .field(TT_KIND, "queue")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_QUEUE, "permits"),
        ];

        let imported = run_from_span_records(spans, ImportOptions::new("svc")).unwrap();

        assert_eq!(imported.run().queues.len(), 1);
        assert_eq!(imported.run().queues[0].waited_from_run_us, None);
        assert_eq!(imported.run().queues[0].waited_until_run_us, None);
        assert!(imported
            .warnings()
            .iter()
            .any(|w| w.message().contains("partial_run_relative_interval")));
        assert!(imported
            .warnings()
            .iter()
            .any(|w| w.message().contains("optional run-relative offsets")));
    }

    #[test]
    fn strict_incomplete_queue_run_relative_offsets_fail() {
        let spans = vec![
            SpanRecord::new("req", 100, 120)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_ROUTE, "/a"),
            SpanRecord::new("q", 102, 103)
                .finished_at_run_us(3_000)
                .field(TT_KIND, "queue")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_QUEUE, "permits"),
        ];

        let err = run_from_span_records(spans, ImportOptions::new("svc").strict(true))
            .expect_err("strict import should reject incomplete run-relative offsets");

        assert!(matches!(err, ImportError::StrictViolation(_)));
        assert!(err.to_string().contains("partial_run_relative_interval"));
    }

    #[test]
    fn non_strict_orphan_stage_is_skipped_and_warning_is_durable() {
        let spans = vec![
            SpanRecord::new("req", 100, 120)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_ROUTE, "/a"),
            SpanRecord::new("st", 102, 119)
                .field(TT_KIND, "stage")
                .field(TT_REQUEST_ID, "r-orphan")
                .field(TT_STAGE, "db"),
        ];
        let imported = run_from_span_records(spans, ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.run().requests.len(), 1);
        assert_eq!(imported.run().stages.len(), 0);
        let warning = "orphan_request_scoped_event";
        assert!(imported
            .warnings()
            .iter()
            .any(|w| w.message().contains(warning)));
        assert!(imported
            .run()
            .metadata
            .lifecycle_warnings
            .iter()
            .any(|w| w.contains(warning)));
    }

    #[test]
    fn orphan_stage_does_not_affect_retained_bounds_or_default_stage_success_warning() {
        let spans = vec![
            SpanRecord::new("req", 100, 120)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_ROUTE, "/a"),
            SpanRecord::new("st", 1, 1_000_000)
                .field(TT_KIND, "stage")
                .field(TT_REQUEST_ID, "r-orphan")
                .field(TT_STAGE, "db"),
        ];
        let imported = run_from_span_records(spans, ImportOptions::new("svc")).unwrap();
        let run = imported.run();
        assert_eq!(run.requests.len(), 1);
        assert_eq!(run.stages.len(), 0);
        assert_eq!(run.metadata.started_at_unix_ms, 100);
        assert_eq!(run.metadata.finished_at_unix_ms, 120);
        assert!(run.metadata.run_id.contains("tracing-import-100-120"));
        assert!(imported
            .warnings()
            .iter()
            .any(|w| w.message().contains("orphan_request_scoped_event")));
        assert!(imported.warnings().iter().any(|w| w
            .message()
            .contains("missing optional 'tt.success'; assumed true")));
        assert!(run
            .metadata
            .lifecycle_warnings
            .iter()
            .any(|w| w.contains("missing optional 'tt.success'; assumed true")));
    }

    #[test]
    fn strict_orphan_stage_fails() {
        let spans = vec![
            SpanRecord::new("req", 100, 120)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_ROUTE, "/a"),
            SpanRecord::new("st", 102, 119)
                .field(TT_KIND, "stage")
                .field(TT_REQUEST_ID, "r-orphan")
                .field(TT_STAGE, "db"),
        ];
        let err = run_from_span_records(spans, ImportOptions::new("svc").strict(true)).unwrap_err();
        match err {
            ImportError::StrictViolation(message) => {
                assert!(message.contains("orphan_request_scoped_event"));
            }
            _ => panic!("expected StrictViolation"),
        }
    }

    #[test]
    fn non_strict_orphan_queue_is_skipped_and_warning_is_durable() {
        let spans = vec![
            SpanRecord::new("req", 100, 120)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_ROUTE, "/a"),
            SpanRecord::new("q", 102, 119)
                .field(TT_KIND, "queue")
                .field(TT_REQUEST_ID, "r-orphan")
                .field(TT_QUEUE, "permits"),
        ];
        let imported = run_from_span_records(spans, ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.run().requests.len(), 1);
        assert_eq!(imported.run().queues.len(), 0);
        let warning = "orphan_request_scoped_event";
        assert!(imported
            .warnings()
            .iter()
            .any(|w| w.message().contains(warning)));
        assert!(imported
            .run()
            .metadata
            .lifecycle_warnings
            .iter()
            .any(|w| w.contains(warning)));
    }

    #[test]
    fn strict_orphan_queue_fails() {
        let spans = vec![
            SpanRecord::new("req", 100, 120)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_ROUTE, "/a"),
            SpanRecord::new("q", 102, 119)
                .field(TT_KIND, "queue")
                .field(TT_REQUEST_ID, "r-orphan")
                .field(TT_QUEUE, "permits"),
        ];
        let err = run_from_span_records(spans, ImportOptions::new("svc").strict(true)).unwrap_err();
        match err {
            ImportError::StrictViolation(message) => {
                assert!(message.contains("orphan_request_scoped_event"));
            }
            _ => panic!("expected StrictViolation"),
        }
    }

    #[test]
    fn wall_clock_stage_before_request_start_is_retained_without_precise_containment_warning() {
        let spans = vec![
            SpanRecord::new("req", 100, 120)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_ROUTE, "/a"),
            SpanRecord::new("stage", 97, 110)
                .field(TT_KIND, "stage")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_STAGE, "db"),
        ];
        let imported = run_from_span_records(spans, ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.run().stages.len(), 1);
        assert!(!imported
            .warnings()
            .iter()
            .any(|w| w.message().contains("child_interval_outside_request")));
        assert!(imported.warnings().iter().any(|w| w
            .message()
            .contains("precise_interval_validation_unavailable")));
        assert!(!imported
            .run()
            .metadata
            .lifecycle_warnings
            .iter()
            .any(|w| w.contains("precise_interval_validation_unavailable")));
    }

    #[test]
    fn wall_clock_stage_after_request_finish_is_retained_without_precise_containment_warning() {
        let spans = vec![
            SpanRecord::new("req", 100, 120)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_ROUTE, "/a"),
            SpanRecord::new("stage", 110, 123)
                .field(TT_KIND, "stage")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_STAGE, "db"),
        ];
        let imported = run_from_span_records(spans, ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.run().stages.len(), 1);
        assert!(!imported
            .warnings()
            .iter()
            .any(|w| w.message().contains("child_interval_outside_request")));
        assert!(imported.warnings().iter().any(|w| w
            .message()
            .contains("precise_interval_validation_unavailable")));
        assert!(!imported
            .run()
            .metadata
            .lifecycle_warnings
            .iter()
            .any(|w| w.contains("precise_interval_validation_unavailable")));
    }

    #[test]
    fn wall_clock_queue_before_request_start_is_retained_without_precise_containment_warning() {
        let spans = vec![
            SpanRecord::new("req", 100, 120)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_ROUTE, "/a"),
            SpanRecord::new("queue", 97, 110)
                .field(TT_KIND, "queue")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_QUEUE, "permits"),
        ];
        let imported = run_from_span_records(spans, ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.run().queues.len(), 1);
        assert!(!imported
            .warnings()
            .iter()
            .any(|w| w.message().contains("child_interval_outside_request")));
        assert!(imported.warnings().iter().any(|w| w
            .message()
            .contains("precise_interval_validation_unavailable")));
        assert!(!imported
            .run()
            .metadata
            .lifecycle_warnings
            .iter()
            .any(|w| w.contains("precise_interval_validation_unavailable")));
    }

    #[test]
    fn wall_clock_queue_after_request_finish_is_retained_without_precise_containment_warning() {
        let spans = vec![
            SpanRecord::new("req", 100, 120)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_ROUTE, "/a"),
            SpanRecord::new("queue", 110, 123)
                .field(TT_KIND, "queue")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_QUEUE, "permits"),
        ];
        let imported = run_from_span_records(spans, ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.run().queues.len(), 1);
        assert!(!imported
            .warnings()
            .iter()
            .any(|w| w.message().contains("child_interval_outside_request")));
        assert!(imported.warnings().iter().any(|w| w
            .message()
            .contains("precise_interval_validation_unavailable")));
        assert!(!imported
            .run()
            .metadata
            .lifecycle_warnings
            .iter()
            .any(|w| w.contains("precise_interval_validation_unavailable")));
    }

    #[test]
    fn precise_stage_outside_request_is_excluded_permissively_and_rejected_strictly() {
        let spans = vec![
            SpanRecord::new("req", 100, 120)
                .started_at_run_us(100_000)
                .finished_at_run_us(120_000)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_ROUTE, "/a"),
            SpanRecord::new("stage", 97, 110)
                .started_at_run_us(97_000)
                .finished_at_run_us(110_000)
                .field(TT_KIND, "stage")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_STAGE, "db"),
        ];
        let imported = run_from_span_records(spans.clone(), ImportOptions::new("svc")).unwrap();
        assert!(imported.run().stages.is_empty());
        assert!(imported
            .warnings()
            .iter()
            .any(|w| w.message().contains("child_interval_outside_request")));
        let err = run_from_span_records(spans, ImportOptions::new("svc").strict(true)).unwrap_err();
        assert!(matches!(err, ImportError::StrictViolation(_)));
        assert!(err.to_string().contains("child_interval_outside_request"));
    }

    #[test]
    fn precise_queue_outside_request_is_excluded_permissively_and_rejected_strictly() {
        let spans = vec![
            SpanRecord::new("req", 100, 120)
                .started_at_run_us(100_000)
                .finished_at_run_us(120_000)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_ROUTE, "/a"),
            SpanRecord::new("queue", 110, 123)
                .started_at_run_us(110_000)
                .finished_at_run_us(123_000)
                .field(TT_KIND, "queue")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_QUEUE, "permits"),
        ];
        let imported = run_from_span_records(spans.clone(), ImportOptions::new("svc")).unwrap();
        assert!(imported.run().queues.is_empty());
        assert!(imported
            .warnings()
            .iter()
            .any(|w| w.message().contains("child_interval_outside_request")));
        let err = run_from_span_records(spans, ImportOptions::new("svc").strict(true)).unwrap_err();
        assert!(matches!(err, ImportError::StrictViolation(_)));
        assert!(err.to_string().contains("child_interval_outside_request"));
    }

    #[test]
    fn wall_clock_stage_starting_before_request_is_retained_without_containment() {
        let spans = vec![
            SpanRecord::new("req", 100, 120)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_ROUTE, "/a"),
            SpanRecord::new("stage", 99, 110)
                .field(TT_KIND, "stage")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_STAGE, "db"),
        ];
        let imported = run_from_span_records(spans, ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.run().stages.len(), 1);
    }

    #[test]
    fn wall_clock_queue_ending_after_request_is_retained_without_containment() {
        let spans = vec![
            SpanRecord::new("req", 100, 120)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_ROUTE, "/a"),
            SpanRecord::new("queue", 110, 121)
                .field(TT_KIND, "queue")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_QUEUE, "permits"),
        ];
        let imported = run_from_span_records(spans, ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.run().queues.len(), 1);
    }

    #[test]
    fn boundary_equal_stage_and_queue_are_accepted() {
        let spans = vec![
            SpanRecord::new("req", 100, 120)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_ROUTE, "/a"),
            SpanRecord::new("stage", 100, 120)
                .field(TT_KIND, "stage")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_STAGE, "db"),
            SpanRecord::new("queue", 100, 120)
                .field(TT_KIND, "queue")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_QUEUE, "permits"),
        ];
        let run = run_from_span_records(spans, ImportOptions::new("svc"))
            .unwrap()
            .run()
            .clone();
        assert_eq!(run.stages.len(), 1);
        assert_eq!(run.queues.len(), 1);
    }

    #[test]
    fn zero_duration_request_with_boundary_equal_stage_and_queue_is_accepted() {
        let spans = vec![
            SpanRecord::new("req", 100, 100)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_ROUTE, "/a"),
            SpanRecord::new("stage", 100, 100)
                .field(TT_KIND, "stage")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_STAGE, "db"),
            SpanRecord::new("queue", 100, 100)
                .field(TT_KIND, "queue")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_QUEUE, "permits"),
        ];
        let run = run_from_span_records(spans, ImportOptions::new("svc"))
            .unwrap()
            .run()
            .clone();
        assert_eq!(run.requests[0].started_at_unix_ms, 100);
        assert_eq!(run.requests[0].finished_at_unix_ms, 100);
        assert_eq!(run.stages.len(), 1);
        assert_eq!(run.queues.len(), 1);
    }

    #[test]
    fn non_strict_duplicate_request_id_excludes_all_duplicate_requests_and_children() {
        let spans = vec![
            SpanRecord::new("req-1", 100, 120)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "dup")
                .field(TT_ROUTE, "/a"),
            SpanRecord::new("req-2", 200, 260)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "dup")
                .field(TT_ROUTE, "/b")
                .field(TT_OUTCOME, "error"),
            SpanRecord::new("stage-skipped", 210, 220)
                .field(TT_KIND, "stage")
                .field(TT_REQUEST_ID, "dup")
                .field(TT_STAGE, "downstream"),
        ];
        let imported = run_from_span_records(spans, ImportOptions::new("svc")).unwrap();
        let run = imported.run();
        assert_eq!(run.requests.len(), 0);
        assert_eq!(run.stages.len(), 0);
        assert_eq!(run.truncation.dropped_requests, 0);

        assert!(imported
            .warnings()
            .iter()
            .any(|w| w.message().contains("duplicate_completed_request_id")));
        assert!(imported
            .warnings()
            .iter()
            .any(|w| w.message().contains("were excluded from analysis")));
        assert!(imported
            .warnings()
            .iter()
            .any(|w| w.message().contains("ambiguous_parent_request_id")));
        assert!(imported
            .run()
            .metadata
            .lifecycle_warnings
            .iter()
            .any(|w| w.contains("duplicate_completed_request_id")));
        assert!(imported
            .run()
            .metadata
            .lifecycle_warnings
            .iter()
            .any(|w| w.contains("ambiguous_parent_request_id")));
    }

    #[test]
    fn non_strict_retained_duplicate_missing_outcome_warns_before_core_exclusion() {
        let spans = vec![
            SpanRecord::new("req-1", 100, 120)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "dup")
                .field(TT_ROUTE, "/a")
                .field(TT_OUTCOME, "ok"),
            SpanRecord::new("req-2", 200, 260)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "dup")
                .field(TT_ROUTE, "/b"),
        ];
        let imported = run_from_span_records(spans, ImportOptions::new("svc")).unwrap();
        let run = imported.run();
        assert_eq!(run.requests.len(), 0);
        assert!(imported
            .warnings()
            .iter()
            .any(|w| w.message().contains("duplicate_completed_request_id")));
        assert!(imported.warnings().iter().any(|w| w
            .message()
            .contains("missing optional 'tt.outcome'; assumed 'ok'")));
        assert!(run
            .metadata
            .lifecycle_warnings
            .iter()
            .any(|w| w.contains("missing optional 'tt.outcome'; assumed 'ok'")));
    }

    #[test]
    fn strict_duplicate_request_id_fails() {
        let spans = vec![
            SpanRecord::new("req-1", 100, 120)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "dup")
                .field(TT_ROUTE, "/a"),
            SpanRecord::new("req-2", 101, 121)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "dup")
                .field(TT_ROUTE, "/b"),
        ];
        let err = run_from_span_records(spans, ImportOptions::new("svc").strict(true)).unwrap_err();
        assert!(matches!(err, ImportError::StrictViolation(_)));
    }

    #[test]
    fn strict_error_lists_unique_core_issue_codes_once_in_order() {
        let spans = vec![
            SpanRecord::new("req-1", 100, 120)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "dup")
                .field(TT_ROUTE, "/a"),
            SpanRecord::new("req-2", 130, 150)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "dup")
                .field(TT_ROUTE, "/b"),
            SpanRecord::new("stage", 135, 140)
                .field(TT_KIND, "stage")
                .field(TT_REQUEST_ID, "dup")
                .field(TT_STAGE, "db"),
        ];

        let err = run_from_span_records(spans, ImportOptions::new("svc").strict(true))
            .expect_err("strict import should reject duplicate requests and ambiguous child");
        let message = err.to_string();
        let duplicate_index = message
            .find("duplicate_completed_request_id")
            .expect("duplicate issue code should be listed");
        let ambiguous_index = message
            .find("ambiguous_parent_request_id")
            .expect("ambiguous parent issue code should be listed");
        assert!(duplicate_index < ambiguous_index);
        assert_eq!(message.matches("duplicate_completed_request_id").count(), 1);
        assert_eq!(message.matches("ambiguous_parent_request_id").count(), 1);
    }

    #[test]
    fn overflow_duplicate_request_ids_beyond_max_requests_do_not_warn() {
        let spans = vec![
            SpanRecord::new("req-1", 100, 120)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_ROUTE, "/a"),
            SpanRecord::new("req-2", 101, 121)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r2")
                .field(TT_ROUTE, "/b"),
            SpanRecord::new("req-overflow", 102, 122)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_ROUTE, "/overflow"),
        ];
        let imported = run_from_span_records(
            spans,
            ImportOptions::new("svc").capture_limits_override(
                tailtriage_core::CaptureLimitsOverride {
                    max_requests: Some(2),
                    ..tailtriage_core::CaptureLimitsOverride::default()
                },
            ),
        )
        .unwrap();
        assert!(!imported
            .warnings()
            .iter()
            .any(|w| w.message().contains("duplicate_completed_request_id")));
    }

    #[test]
    fn invalid_extreme_timestamps_do_not_affect_metadata_bounds_or_default_run_id() {
        let spans = vec![
            SpanRecord::new("req", 100, 120)
                .started_at_run_us(100_000)
                .finished_at_run_us(120_000)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_ROUTE, "/a"),
            SpanRecord::new("stage-extreme", 1, 1_000_000)
                .started_at_run_us(1_000)
                .finished_at_run_us(1_000_000_000)
                .field(TT_KIND, "stage")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_STAGE, "db"),
            SpanRecord::new("queue-extreme", 1, 1_000_000)
                .started_at_run_us(1_000)
                .finished_at_run_us(1_000_000_000)
                .field(TT_KIND, "queue")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_QUEUE, "permits"),
        ];
        let imported = run_from_span_records(spans, ImportOptions::new("svc")).unwrap();
        let run = imported.run();
        assert_eq!(run.stages.len(), 0);
        assert_eq!(run.queues.len(), 0);
        assert_eq!(run.metadata.started_at_unix_ms, 100);
        assert_eq!(run.metadata.finished_at_unix_ms, 120);
        assert!(run.metadata.run_id.contains("tracing-import-100-120"));
    }

    #[test]
    fn orphan_queue_does_not_affect_retained_bounds_or_default_run_id() {
        let spans = vec![
            SpanRecord::new("req", 100, 120)
                .started_at_run_us(100_000)
                .finished_at_run_us(120_000)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_ROUTE, "/a"),
            SpanRecord::new("q", 1, 1_000_000)
                .field(TT_KIND, "queue")
                .field(TT_REQUEST_ID, "r-orphan")
                .field(TT_QUEUE, "permits"),
        ];
        let imported = run_from_span_records(spans, ImportOptions::new("svc")).unwrap();
        let run = imported.run();
        assert_eq!(run.requests.len(), 1);
        assert_eq!(run.queues.len(), 0);
        assert_eq!(run.metadata.started_at_unix_ms, 100);
        assert_eq!(run.metadata.finished_at_unix_ms, 120);
        assert!(run.metadata.run_id.contains("tracing-import-100-120"));
    }

    #[test]
    fn overflow_request_does_not_affect_metadata_bounds_or_run_id() {
        let max_requests = 2;
        let mut spans = Vec::new();
        for index in 0..max_requests {
            spans.push(
                SpanRecord::new(format!("req-{index}"), 100, 120)
                    .field(TT_KIND, "request")
                    .field(TT_REQUEST_ID, format!("r{index}"))
                    .field(TT_ROUTE, "/a")
                    .field(TT_OUTCOME, "ok"),
            );
        }
        spans.push(
            SpanRecord::new("overflow", 1, 1_000_000)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r-overflow")
                .field(TT_ROUTE, "/overflow"),
        );
        let imported = run_from_span_records(
            spans,
            ImportOptions::new("svc").capture_limits_override(
                tailtriage_core::CaptureLimitsOverride {
                    max_requests: Some(max_requests),
                    ..tailtriage_core::CaptureLimitsOverride::default()
                },
            ),
        )
        .unwrap();
        let run = imported.run();
        assert_eq!(run.requests.len(), max_requests);
        assert_eq!(run.truncation.dropped_requests, 1);
        assert_eq!(run.metadata.started_at_unix_ms, 100);
        assert_eq!(run.metadata.finished_at_unix_ms, 120);
        assert!(run.metadata.run_id.contains("tracing-import-100-120"));
        assert!(!imported.warnings().iter().any(|w| w
            .message()
            .contains("missing optional 'tt.outcome'; assumed 'ok'")));
    }

    #[test]
    fn overflow_stage_does_not_affect_metadata_bounds_or_success_warning() {
        let max_stages = 2;
        let mut spans = vec![SpanRecord::new("req", 100, 120)
            .field(TT_KIND, "request")
            .field(TT_REQUEST_ID, "r1")
            .field(TT_ROUTE, "/a")
            .field(TT_OUTCOME, "ok")];
        for index in 0..max_stages {
            spans.push(
                SpanRecord::new(format!("stage-{index}"), 101, 110)
                    .field(TT_KIND, "stage")
                    .field(TT_REQUEST_ID, "r1")
                    .field(TT_STAGE, format!("s{index}"))
                    .field(TT_SUCCESS, true),
            );
        }
        spans.push(
            SpanRecord::new("overflow-stage", 1, 1_000_000)
                .field(TT_KIND, "stage")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_STAGE, "overflow-stage"),
        );
        let imported = run_from_span_records(
            spans,
            ImportOptions::new("svc").capture_limits_override(
                tailtriage_core::CaptureLimitsOverride {
                    max_stages: Some(max_stages),
                    ..tailtriage_core::CaptureLimitsOverride::default()
                },
            ),
        )
        .unwrap();
        let run = imported.run();
        assert_eq!(run.stages.len(), max_stages);
        assert_eq!(run.truncation.dropped_stages, 1);
        assert_eq!(run.metadata.started_at_unix_ms, 100);
        assert_eq!(run.metadata.finished_at_unix_ms, 120);
        assert!(!imported.warnings().iter().any(|w| w
            .message()
            .contains("missing optional 'tt.success'; assumed true")));
    }

    #[test]
    fn overflow_queue_does_not_affect_metadata_bounds() {
        let max_queues = 2;
        let mut spans = vec![SpanRecord::new("req", 100, 120)
            .field(TT_KIND, "request")
            .field(TT_REQUEST_ID, "r1")
            .field(TT_ROUTE, "/a")
            .field(TT_OUTCOME, "ok")];
        for index in 0..max_queues {
            spans.push(
                SpanRecord::new(format!("queue-{index}"), 101, 110)
                    .field(TT_KIND, "queue")
                    .field(TT_REQUEST_ID, "r1")
                    .field(TT_QUEUE, format!("q{index}")),
            );
        }
        spans.push(
            SpanRecord::new("overflow-queue", 1, 1_000_000)
                .field(TT_KIND, "queue")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_QUEUE, "overflow-queue"),
        );
        let imported = run_from_span_records(
            spans,
            ImportOptions::new("svc").capture_limits_override(
                tailtriage_core::CaptureLimitsOverride {
                    max_queues: Some(max_queues),
                    ..tailtriage_core::CaptureLimitsOverride::default()
                },
            ),
        )
        .unwrap();
        let run = imported.run();
        assert_eq!(run.queues.len(), max_queues);
        assert_eq!(run.truncation.dropped_queues, 1);
        assert_eq!(run.metadata.started_at_unix_ms, 100);
        assert_eq!(run.metadata.finished_at_unix_ms, 120);
    }

    #[test]
    fn run_from_span_records_respects_import_mode_and_resolved_limits() {
        let spans = vec![
            SpanRecord::new("req-1", 100, 120)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_ROUTE, "/a"),
            SpanRecord::new("req-2", 101, 121)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r2")
                .field(TT_ROUTE, "/b"),
            SpanRecord::new("stage-1", 102, 110)
                .field(TT_KIND, "stage")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_STAGE, "s1"),
            SpanRecord::new("stage-2", 103, 111)
                .field(TT_KIND, "stage")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_STAGE, "s2"),
            SpanRecord::new("queue-1", 104, 112)
                .field(TT_KIND, "queue")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_QUEUE, "q1"),
            SpanRecord::new("queue-2", 105, 113)
                .field(TT_KIND, "queue")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_QUEUE, "q2"),
        ];
        let limits = tailtriage_core::CaptureLimitsOverride {
            max_requests: Some(1),
            max_stages: Some(1),
            max_queues: Some(1),
            ..tailtriage_core::CaptureLimitsOverride::default()
        };
        let imported = run_from_span_records(
            spans,
            ImportOptions::new("svc")
                .mode(CaptureMode::Investigation)
                .capture_limits_override(limits),
        )
        .unwrap();
        let run = imported.run();
        let effective = run
            .metadata
            .effective_core_config
            .as_ref()
            .expect("effective core config should be present");
        assert_eq!(run.metadata.mode, CaptureMode::Investigation);
        assert_eq!(effective.capture_limits.max_requests, 1);
        assert_eq!(effective.capture_limits.max_stages, 1);
        assert_eq!(effective.capture_limits.max_queues, 1);
        assert!(!effective.strict_lifecycle);
        assert_eq!(run.requests.len(), 1);
        assert_eq!(run.stages.len(), 1);
        assert_eq!(run.queues.len(), 1);
        assert_eq!(run.truncation.dropped_requests, 1);
        assert_eq!(run.truncation.dropped_stages, 1);
        assert_eq!(run.truncation.dropped_queues, 1);
    }

    #[test]
    fn retained_request_missing_outcome_still_warns() {
        let spans = vec![SpanRecord::new("req", 100, 120)
            .field(TT_KIND, "request")
            .field(TT_REQUEST_ID, "r1")
            .field(TT_ROUTE, "/a")];
        let imported = run_from_span_records(spans, ImportOptions::new("svc")).unwrap();
        assert!(imported.warnings().iter().any(|w| w
            .message()
            .contains("missing optional 'tt.outcome'; assumed 'ok'")));
    }

    #[test]
    fn retained_stage_missing_success_still_warns() {
        let spans = vec![
            SpanRecord::new("req", 100, 120)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_ROUTE, "/a")
                .field(TT_OUTCOME, "ok"),
            SpanRecord::new("stage", 105, 115)
                .field(TT_KIND, "stage")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_STAGE, "db"),
        ];
        let imported = run_from_span_records(spans, ImportOptions::new("svc")).unwrap();
        assert!(imported.warnings().iter().any(|w| w
            .message()
            .contains("missing optional 'tt.success'; assumed true")));
    }

    #[test]
    fn matched_request_stage_queue_are_retained() {
        let spans = vec![
            SpanRecord::new("req", 100, 120)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_ROUTE, "/a"),
            SpanRecord::new("st", 105, 115)
                .field(TT_KIND, "stage")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_STAGE, "db"),
            SpanRecord::new("q", 102, 103)
                .field(TT_KIND, "queue")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_QUEUE, "permits"),
        ];
        let run = run_from_span_records(spans, ImportOptions::new("svc"))
            .unwrap()
            .run()
            .clone();
        assert_eq!(run.requests.len(), 1);
        assert_eq!(run.stages.len(), 1);
        assert_eq!(run.queues.len(), 1);
    }

    #[test]
    fn missing_optional_fields_default() {
        let spans = vec![
            SpanRecord::new("req", 1, 2)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_ROUTE, "/"),
            SpanRecord::new("st", 1, 2)
                .field(TT_KIND, "stage")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_STAGE, "s1"),
            SpanRecord::new("q", 1, 2)
                .field(TT_KIND, "queue")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_QUEUE, "q1"),
        ];
        let run = run_from_span_records(spans, ImportOptions::new("svc"))
            .unwrap()
            .run()
            .clone();
        assert_eq!(run.requests[0].outcome, "ok");
        assert!(run.stages[0].success);
        assert_eq!(run.queues[0].depth_at_start, None);
    }

    #[test]
    fn missing_required_field_warns_and_skips_non_strict() {
        let spans = vec![SpanRecord::new("req", 1, 2)
            .field(TT_KIND, "request")
            .field(TT_REQUEST_ID, "r1")];
        let imported = run_from_span_records(spans, ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.run().requests.len(), 0);
        assert!(!imported.warnings().is_empty());
    }

    #[test]
    fn missing_required_field_errors_in_strict() {
        let spans = vec![SpanRecord::new("req", 1, 2)
            .field(TT_KIND, "request")
            .field(TT_REQUEST_ID, "r1")];
        let err = run_from_span_records(spans, ImportOptions::new("svc").strict(true)).unwrap_err();
        assert!(matches!(err, ImportError::StrictViolation(_)));
    }

    #[test]
    fn unknown_kind_warns_non_strict() {
        let spans = vec![SpanRecord::new("x", 1, 2).field(TT_KIND, "wat")];
        let imported = run_from_span_records(spans, ImportOptions::new("svc")).unwrap();
        assert!(!imported.warnings().is_empty());
        assert!(imported
            .run()
            .metadata
            .lifecycle_warnings
            .iter()
            .any(|warning| warning.contains("unknown tt.kind 'wat'")));
    }

    #[test]
    fn unknown_kind_errors_in_strict() {
        let spans = vec![SpanRecord::new("x", 1, 2).field(TT_KIND, "wat")];
        let err = run_from_span_records(spans, ImportOptions::new("svc").strict(true)).unwrap_err();
        assert!(matches!(err, ImportError::StrictViolation(_)));
    }

    #[test]
    fn span_without_kind_ignored_silently() {
        let spans = vec![SpanRecord::new("x", 1, 2).field("a", "b")];
        let imported = run_from_span_records(spans, ImportOptions::new("svc")).unwrap();
        assert!(imported.warnings().is_empty());
    }

    #[test]
    fn tailtriage_tagged_span_missing_kind_warns_or_errors() {
        let spans = vec![SpanRecord::new("http.request", 1, 2)
            .field(TT_REQUEST_ID, "r1")
            .field(TT_ROUTE, "/a")];
        let imported = run_from_span_records(spans.clone(), ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.run().requests.len(), 0);
        assert!(imported.warnings().iter().any(|w| w
            .message()
            .contains("missing required field 'tt.kind' in span 'http.request'")));

        let err = run_from_span_records(spans, ImportOptions::new("svc").strict(true)).unwrap_err();
        assert!(matches!(err, ImportError::StrictViolation(_)));
    }

    #[test]
    fn missing_optional_defaults_emit_aggregate_warnings() {
        let spans = vec![
            SpanRecord::new("req1", 1, 2)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_ROUTE, "/a"),
            SpanRecord::new("req2", 1, 2)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r2")
                .field(TT_ROUTE, "/b"),
            SpanRecord::new("st1", 1, 2)
                .field(TT_KIND, "stage")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_STAGE, "db"),
            SpanRecord::new("st2", 1, 2)
                .field(TT_KIND, "stage")
                .field(TT_REQUEST_ID, "r2")
                .field(TT_STAGE, "cache"),
            SpanRecord::new("q", 1, 2)
                .field(TT_KIND, "queue")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_QUEUE, "permits"),
        ];
        let imported = run_from_span_records(spans, ImportOptions::new("svc")).unwrap();
        let msgs = imported
            .warnings()
            .iter()
            .map(ImportWarning::message)
            .collect::<Vec<_>>();
        assert!(msgs
            .iter()
            .any(|m| m.contains("2 request span(s) missing optional 'tt.outcome'; assumed 'ok'")));
        assert!(msgs
            .iter()
            .any(|m| m.contains("2 stage span(s) missing optional 'tt.success'; assumed true")));
        assert!(!msgs.iter().any(|m| m.contains("tt.depth_at_start")));
        let lifecycle_warnings = &imported.run().metadata.lifecycle_warnings;
        assert!(lifecycle_warnings
            .iter()
            .any(|m| m.contains("2 request span(s) missing optional 'tt.outcome'; assumed 'ok'")));
        assert!(lifecycle_warnings
            .iter()
            .any(|m| m.contains("2 stage span(s) missing optional 'tt.success'; assumed true")));
        assert_eq!(
            lifecycle_warnings
                .iter()
                .filter(|m| m.contains("missing optional 'tt.outcome'; assumed 'ok'"))
                .count(),
            1
        );
        assert_eq!(
            lifecycle_warnings
                .iter()
                .filter(|m| m.contains("missing optional 'tt.success'; assumed true"))
                .count(),
            1
        );
    }

    #[test]
    fn inverted_timestamps_warn_or_error() {
        let spans = vec![SpanRecord::new("req", 5, 4)
            .field(TT_KIND, "request")
            .field(TT_REQUEST_ID, "r1")
            .field(TT_ROUTE, "/")];
        let imported = run_from_span_records(spans.clone(), ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.run().requests.len(), 0);
        let err = run_from_span_records(spans, ImportOptions::new("svc").strict(true)).unwrap_err();
        assert!(matches!(err, ImportError::StrictViolation(_)));
    }

    #[test]
    fn run_from_span_records_validates_service_name_before_strict_span_parsing() {
        let spans = vec![SpanRecord::new("bad", 10, 20).field(TT_KIND, 123_u64)];
        let err = run_from_span_records(spans, ImportOptions::new("   ").strict(true)).unwrap_err();
        assert!(matches!(err, ImportError::EmptyServiceName));
    }

    #[test]
    fn run_from_span_records_empty_input_uses_equal_start_finish_finalized() {
        let imported = run_from_span_records(Vec::new(), ImportOptions::new("svc")).unwrap();
        assert!(imported.run().requests.is_empty());
        assert!(imported.run().stages.is_empty());
        assert!(imported.run().queues.is_empty());
        assert_eq!(
            imported.run().metadata.finished_at_unix_ms,
            imported.run().metadata.started_at_unix_ms
        );
        assert_eq!(
            imported.run().metadata.finalized_at_unix_ms,
            Some(imported.run().metadata.finished_at_unix_ms)
        );
    }

    #[test]
    fn run_from_span_records_uses_schema_v1_finalization_semantics() {
        let spans = vec![SpanRecord::new("req", 10, 20)
            .field(TT_KIND, "request")
            .field(TT_REQUEST_ID, "r1")
            .field(TT_ROUTE, "/")];
        let run = run_from_span_records(spans, ImportOptions::new("svc"))
            .unwrap()
            .run()
            .clone();
        assert_eq!(run.metadata.started_at_unix_ms, 10);
        assert_eq!(run.metadata.finished_at_unix_ms, 20);
        assert_eq!(run.metadata.finalized_at_unix_ms, Some(20));
    }

    #[test]
    fn run_from_span_records_preserves_computed_import_run_id() {
        let spans = vec![SpanRecord::new("req", 10, 20)
            .field(TT_KIND, "request")
            .field(TT_REQUEST_ID, "r1")
            .field(TT_ROUTE, "/")];
        let run = run_from_span_records(spans, ImportOptions::new("svc"))
            .unwrap()
            .run()
            .clone();
        assert_eq!(run.metadata.run_id, "tracing-import-10-20");
    }

    #[test]
    fn run_from_span_records_preserves_explicit_import_run_id() {
        let spans = vec![SpanRecord::new("req", 10, 20)
            .field(TT_KIND, "request")
            .field(TT_REQUEST_ID, "r1")
            .field(TT_ROUTE, "/")];
        let run = run_from_span_records(spans, ImportOptions::new("svc").run_id("explicit-run"))
            .unwrap()
            .run()
            .clone();
        assert_eq!(run.metadata.run_id, "explicit-run");
    }

    #[test]
    fn runtime_snapshots_and_inflight_are_empty() {
        let imported = run_from_span_records(Vec::new(), ImportOptions::new("svc")).unwrap();
        assert!(imported.run().runtime_snapshots.is_empty());
        assert!(imported.run().inflight.is_empty());
    }

    #[test]
    fn ordinary_span_without_kind_does_not_affect_metadata_bounds() {
        let spans = vec![
            SpanRecord::new("ordinary", 1, 1_000).field("foo", "bar"),
            SpanRecord::new("req", 10, 20)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_ROUTE, "/"),
        ];
        let run = run_from_span_records(spans, ImportOptions::new("svc"))
            .unwrap()
            .run()
            .clone();
        assert_eq!(run.metadata.started_at_unix_ms, 10);
        assert_eq!(run.metadata.finished_at_unix_ms, 20);
    }

    #[test]
    fn unknown_kind_does_not_affect_metadata_bounds_and_is_durable_lifecycle_warning() {
        let spans = vec![
            SpanRecord::new("unknown", 1, 1_000).field(TT_KIND, "wat"),
            SpanRecord::new("req", 10, 20)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_ROUTE, "/"),
        ];
        let imported = run_from_span_records(spans, ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.run().metadata.started_at_unix_ms, 10);
        assert_eq!(imported.run().metadata.finished_at_unix_ms, 20);
        assert!(imported
            .run()
            .metadata
            .lifecycle_warnings
            .iter()
            .any(|w| w.contains("unknown tt.kind")));
    }

    #[test]
    fn non_string_kind_warns_non_strict_and_errors_strict() {
        let bad = SpanRecord::new("bad", 1, 2).field(TT_KIND, true);
        let imported = run_from_span_records(vec![bad.clone()], ImportOptions::new("svc")).unwrap();
        assert!(!imported.warnings().is_empty());
        assert!(run_from_span_records(vec![bad], ImportOptions::new("svc").strict(true)).is_err());
    }

    #[test]
    fn non_string_required_and_optional_fields_warn_non_strict_and_error_strict() {
        let bad_route = SpanRecord::new("req", 1, 2)
            .field(TT_KIND, "request")
            .field(TT_REQUEST_ID, "r1")
            .field(TT_ROUTE, true);
        let imported =
            run_from_span_records(vec![bad_route.clone()], ImportOptions::new("svc")).unwrap();
        assert!(imported.run().requests.is_empty());
        assert!(!imported.warnings().is_empty());
        assert!(
            run_from_span_records(vec![bad_route], ImportOptions::new("svc").strict(true)).is_err()
        );

        let bad_outcome = SpanRecord::new("req", 1, 2)
            .field(TT_KIND, "request")
            .field(TT_REQUEST_ID, "r1")
            .field(TT_ROUTE, "/")
            .field(TT_OUTCOME, 7_u64);
        let imported =
            run_from_span_records(vec![bad_outcome.clone()], ImportOptions::new("svc")).unwrap();
        assert!(imported.run().requests.is_empty());
        assert!(!imported.warnings().is_empty());
        assert!(
            run_from_span_records(vec![bad_outcome], ImportOptions::new("svc").strict(true))
                .is_err()
        );
    }

    #[test]
    fn whitespace_only_request_id_non_strict_skips_request_and_warns() {
        let spans = vec![SpanRecord::new("req", 1, 2)
            .field(TT_KIND, "request")
            .field(TT_REQUEST_ID, "   ")
            .field(TT_ROUTE, "/")];
        let imported = run_from_span_records(spans, ImportOptions::new("svc")).unwrap();
        assert!(imported.run().requests.is_empty());
        assert!(imported.warnings().iter().any(|w| {
            w.message()
                .contains("invalid field 'tt.request_id' in span 'req'")
                && w.message().contains("cannot be empty or whitespace")
        }));
    }

    #[test]
    fn whitespace_only_route_non_strict_skips_request_and_warns() {
        let spans = vec![SpanRecord::new("req", 1, 2)
            .field(TT_KIND, "request")
            .field(TT_REQUEST_ID, "r1")
            .field(TT_ROUTE, "  \t")];
        let imported = run_from_span_records(spans, ImportOptions::new("svc")).unwrap();
        assert!(imported.run().requests.is_empty());
        assert!(imported.warnings().iter().any(|w| {
            w.message()
                .contains("invalid field 'tt.route' in span 'req'")
                && w.message().contains("cannot be empty or whitespace")
        }));
    }

    #[test]
    fn whitespace_only_stage_non_strict_skips_stage_and_warns() {
        let spans = vec![
            SpanRecord::new("req", 1, 3)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_ROUTE, "/"),
            SpanRecord::new("stage", 1, 2)
                .field(TT_KIND, "stage")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_STAGE, "   "),
        ];
        let imported = run_from_span_records(spans, ImportOptions::new("svc")).unwrap();
        assert!(imported.run().stages.is_empty());
        assert!(imported.warnings().iter().any(|w| {
            w.message()
                .contains("invalid field 'tt.stage' in span 'stage'")
                && w.message().contains("cannot be empty or whitespace")
        }));
    }

    #[test]
    fn whitespace_only_queue_non_strict_skips_queue_and_warns() {
        let spans = vec![
            SpanRecord::new("req", 1, 3)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_ROUTE, "/"),
            SpanRecord::new("queue", 1, 2)
                .field(TT_KIND, "queue")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_QUEUE, " "),
        ];
        let imported = run_from_span_records(spans, ImportOptions::new("svc")).unwrap();
        assert!(imported.run().queues.is_empty());
        assert!(imported.warnings().iter().any(|w| {
            w.message()
                .contains("invalid field 'tt.queue' in span 'queue'")
                && w.message().contains("cannot be empty or whitespace")
        }));
    }

    #[test]
    fn whitespace_only_required_field_strict_returns_strict_violation() {
        let spans = vec![SpanRecord::new("req", 1, 2)
            .field(TT_KIND, "request")
            .field(TT_REQUEST_ID, " ")
            .field(TT_ROUTE, "/")];
        let err = run_from_span_records(spans, ImportOptions::new("svc").strict(true)).unwrap_err();
        assert!(matches!(err, ImportError::StrictViolation(_)));
        assert!(err
            .to_string()
            .contains("invalid field 'tt.request_id' in span 'req'"));
    }

    #[test]
    fn builtin_request_outcomes_are_retained_exactly() {
        let spans = RECOMMENDED_OUTCOME_LABELS
            .iter()
            .enumerate()
            .map(|(idx, outcome)| {
                SpanRecord::new("req", idx as u64 + 1, idx as u64 + 2)
                    .field(TT_KIND, "request")
                    .field(TT_REQUEST_ID, format!("r{idx}"))
                    .field(TT_ROUTE, "/")
                    .field(TT_OUTCOME, *outcome)
            })
            .collect::<Vec<_>>();
        let imported = run_from_span_records(spans, ImportOptions::new("svc")).unwrap();
        let retained = imported
            .run()
            .requests
            .iter()
            .map(|request| request.outcome.as_str())
            .collect::<Vec<_>>();
        assert_eq!(retained, RECOMMENDED_OUTCOME_LABELS);
    }

    #[test]
    fn missing_outcome_defaults_ok_and_warns() {
        let spans = vec![SpanRecord::new("req", 1, 2)
            .field(TT_KIND, "request")
            .field(TT_REQUEST_ID, "r1")
            .field(TT_ROUTE, "/")];
        let imported = run_from_span_records(spans, ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.run().requests.len(), 1);
        assert_eq!(imported.run().requests[0].outcome, "ok");
        assert!(imported.warnings().iter().any(|w| w
            .message()
            .contains("missing optional 'tt.outcome'; assumed 'ok'")));
    }

    #[test]
    fn custom_outcome_is_accepted_and_preserved_exactly() {
        let spans = vec![SpanRecord::new("http.request", 1, 2)
            .field(TT_KIND, "request")
            .field(TT_REQUEST_ID, "r1")
            .field(TT_ROUTE, "/")
            .field(TT_OUTCOME, "cache_miss_fallback")];
        let imported = run_from_span_records(spans, ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.run().requests.len(), 1);
        assert_eq!(imported.run().requests[0].outcome, "cache_miss_fallback");
    }

    #[test]
    fn whitespace_only_outcome_non_strict_skips_request_and_warns() {
        let spans = vec![SpanRecord::new("http.request", 1, 2)
            .field(TT_KIND, "request")
            .field(TT_REQUEST_ID, "r1")
            .field(TT_ROUTE, "/")
            .field(TT_OUTCOME, "   ")];
        let imported = run_from_span_records(spans, ImportOptions::new("svc")).unwrap();
        assert!(imported.run().requests.is_empty());
        assert!(imported.warnings().iter().any(|w| w
            .message()
            .contains("invalid field 'tt.outcome' in span 'http.request': expected non-empty, non-whitespace string")));
    }

    #[test]
    fn whitespace_only_outcome_strict_fails() {
        let spans = vec![SpanRecord::new("http.request", 1, 2)
            .field(TT_KIND, "request")
            .field(TT_REQUEST_ID, "r1")
            .field(TT_ROUTE, "/")
            .field(TT_OUTCOME, " \t")];
        let err = run_from_span_records(spans, ImportOptions::new("svc").strict(true)).unwrap_err();
        assert!(matches!(err, ImportError::StrictViolation(_)));
        assert!(err
            .to_string()
            .contains("invalid field 'tt.outcome' in span 'http.request': expected non-empty, non-whitespace string"));
    }

    #[test]
    fn non_string_outcome_non_strict_skips_request_and_warns() {
        let spans = vec![SpanRecord::new("http.request", 1, 2)
            .field(TT_KIND, "request")
            .field(TT_REQUEST_ID, "r1")
            .field(TT_ROUTE, "/")
            .field(TT_OUTCOME, 42_u64)];
        let imported = run_from_span_records(spans, ImportOptions::new("svc")).unwrap();
        assert!(imported.run().requests.is_empty());
        assert!(imported.warnings().iter().any(|w| w
            .message()
            .contains("invalid field 'tt.outcome' in span 'http.request': expected string")));
    }

    #[test]
    fn non_string_outcome_strict_fails() {
        let spans = vec![SpanRecord::new("http.request", 1, 2)
            .field(TT_KIND, "request")
            .field(TT_REQUEST_ID, "r1")
            .field(TT_ROUTE, "/")
            .field(TT_OUTCOME, false)];
        let err = run_from_span_records(spans, ImportOptions::new("svc").strict(true)).unwrap_err();
        assert!(matches!(err, ImportError::StrictViolation(_)));
        assert!(err
            .to_string()
            .contains("invalid field 'tt.outcome' in span 'http.request': expected string"));
    }

    #[test]
    fn native_other_outcome_round_trips_through_tracing_style_outcome_field() {
        let native = tailtriage_core::Outcome::Other("custom".to_owned());
        let spans = vec![SpanRecord::new("http.request", 1, 2)
            .field(TT_KIND, "request")
            .field(TT_REQUEST_ID, "r1")
            .field(TT_ROUTE, "/")
            .field(TT_OUTCOME, native.as_str())];
        let imported = run_from_span_records(spans, ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.run().requests.len(), 1);
        assert_eq!(imported.run().requests[0].outcome, native.as_str());
    }

    #[test]
    fn invalid_whitespace_outcome_skips_child_spans_via_existing_correlation_logic() {
        let spans = vec![
            SpanRecord::new("http.request", 1_000, 2_000)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_ROUTE, "/")
                .field(TT_OUTCOME, "   "),
            SpanRecord::new("db.stage", 1_100, 1_200)
                .field(TT_KIND, "stage")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_STAGE, "db")
                .field(TT_SUCCESS, true),
            SpanRecord::new("worker.queue", 1_300, 1_350)
                .field(TT_KIND, "queue")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_QUEUE, "worker"),
        ];
        let imported = run_from_span_records(spans, ImportOptions::new("svc")).unwrap();
        assert!(imported.run().requests.is_empty());
        assert!(imported.run().stages.is_empty());
        assert!(imported.run().queues.is_empty());
        assert!(imported
            .warnings()
            .iter()
            .any(|w| w.message().contains("orphan_request_scoped_event")));
    }

    #[test]
    fn strict_mode_duplicate_request_id_overflow_keeps_retained_children() {
        let spans = vec![
            SpanRecord::new("req-1", 100, 200)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_ROUTE, "/a"),
            SpanRecord::new("st-1", 120, 150)
                .field(TT_KIND, "stage")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_STAGE, "db"),
            SpanRecord::new("q-1", 130, 140)
                .field(TT_KIND, "queue")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_QUEUE, "permits"),
            SpanRecord::new("req-2", 300, 400)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_ROUTE, "/a"),
        ];
        let imported = run_from_span_records(
            spans,
            ImportOptions::new("checkout")
                .strict(true)
                .capture_limits_override(tailtriage_core::CaptureLimitsOverride {
                    max_requests: Some(1),
                    max_stages: None,
                    max_queues: None,
                    ..tailtriage_core::CaptureLimitsOverride::default()
                }),
        )
        .expect("strict import should keep children that match retained request interval");

        assert_eq!(imported.run().requests.len(), 1);
        assert_eq!(imported.run().requests[0].request_id, "r1");
        assert_eq!(imported.run().requests[0].started_at_unix_ms, 100);
        assert_eq!(imported.run().requests[0].finished_at_unix_ms, 200);
        assert_eq!(imported.run().stages.len(), 1);
        assert_eq!(imported.run().stages[0].request_id, "r1");
        assert_eq!(imported.run().stages[0].started_at_unix_ms, 120);
        assert_eq!(imported.run().stages[0].finished_at_unix_ms, 150);
        assert_eq!(imported.run().queues.len(), 1);
        assert_eq!(imported.run().queues[0].request_id, "r1");
        assert_eq!(imported.run().queues[0].waited_from_unix_ms, 130);
        assert_eq!(imported.run().queues[0].waited_until_unix_ms, 140);
        assert_eq!(imported.run().truncation.dropped_requests, 1);
        assert_eq!(imported.run().truncation.dropped_stages, 0);
        assert_eq!(imported.run().truncation.dropped_queues, 0);
    }

    #[test]
    fn strict_mode_duplicate_request_id_overflow_only_children_retains_coarse_timing() {
        let spans = vec![
            SpanRecord::new("req-1", 100, 200)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_ROUTE, "/a"),
            SpanRecord::new("req-2", 300, 400)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_ROUTE, "/a"),
            SpanRecord::new("st-1", 320, 350)
                .field(TT_KIND, "stage")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_STAGE, "db"),
            SpanRecord::new("q-1", 330, 340)
                .field(TT_KIND, "queue")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_QUEUE, "permits"),
        ];
        let imported = run_from_span_records(
            spans,
            ImportOptions::new("checkout")
                .strict(true)
                .capture_limits_override(tailtriage_core::CaptureLimitsOverride {
                    max_requests: Some(1),
                    max_stages: None,
                    max_queues: None,
                    ..tailtriage_core::CaptureLimitsOverride::default()
                }),
        )
        .expect("strict import should retain coarse-only children for retained overflow request");
        assert_eq!(imported.run().stages.len(), 1);
        assert_eq!(imported.run().queues.len(), 1);
    }

    #[test]
    fn non_strict_duplicate_request_id_overflow_only_children_retains_coarse_timing() {
        let spans = vec![
            SpanRecord::new("req-1", 100, 200)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_ROUTE, "/a"),
            SpanRecord::new("req-2", 300, 400)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_ROUTE, "/a"),
            SpanRecord::new("st-1", 320, 350)
                .field(TT_KIND, "stage")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_STAGE, "db"),
            SpanRecord::new("q-1", 330, 340)
                .field(TT_KIND, "queue")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_QUEUE, "permits"),
        ];
        let imported = run_from_span_records(
            spans,
            ImportOptions::new("checkout").capture_limits_override(
                tailtriage_core::CaptureLimitsOverride {
                    max_requests: Some(1),
                    max_stages: None,
                    max_queues: None,
                    ..tailtriage_core::CaptureLimitsOverride::default()
                },
            ),
        )
        .expect("non-strict import should succeed with warnings");

        assert_eq!(imported.run().requests.len(), 1);
        assert_eq!(imported.run().stages.len(), 1);
        assert_eq!(imported.run().queues.len(), 1);
        assert_eq!(imported.run().truncation.dropped_requests, 1);
        assert_eq!(imported.run().truncation.dropped_stages, 0);
        assert_eq!(imported.run().truncation.dropped_queues, 0);

        let warning_msgs: Vec<&str> = imported
            .warnings()
            .iter()
            .map(ImportWarning::message)
            .collect();
        assert!(warning_msgs
            .iter()
            .all(|msg| !msg.contains("child_interval_outside_request")));
        assert!(warning_msgs
            .iter()
            .all(|msg| !msg.contains("valid but not retained due to max_requests")));

        let lifecycle_warnings = &imported.run().metadata.lifecycle_warnings;
        assert!(lifecycle_warnings
            .iter()
            .all(|msg| !msg.contains("child_interval_outside_request")));
        assert!(lifecycle_warnings
            .iter()
            .all(|msg| !msg.contains("valid but not retained due to max_requests")));
    }
    #[test]
    fn strict_mode_max_requests_overflow_children_are_retention_fallout() {
        let spans = vec![
            SpanRecord::new("req-1", 100, 200)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_ROUTE, "/a"),
            SpanRecord::new("st-1", 120, 150)
                .field(TT_KIND, "stage")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_STAGE, "db"),
            SpanRecord::new("q-1", 121, 130)
                .field(TT_KIND, "queue")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_QUEUE, "permits"),
            SpanRecord::new("req-2", 300, 400)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r2")
                .field(TT_ROUTE, "/a"),
            SpanRecord::new("st-2", 320, 350)
                .field(TT_KIND, "stage")
                .field(TT_REQUEST_ID, "r2")
                .field(TT_STAGE, "db"),
            SpanRecord::new("q-2", 321, 330)
                .field(TT_KIND, "queue")
                .field(TT_REQUEST_ID, "r2")
                .field(TT_QUEUE, "permits"),
        ];
        let imported = run_from_span_records(
            spans,
            ImportOptions::new("checkout")
                .strict(true)
                .capture_limits_override(tailtriage_core::CaptureLimitsOverride {
                    max_requests: Some(1),
                    max_stages: None,
                    max_queues: None,
                    ..tailtriage_core::CaptureLimitsOverride::default()
                }),
        )
        .expect_err("strict import should reject retained children whose parent was dropped by max_requests");
        assert!(matches!(imported, ImportError::StrictViolation(_)));
        assert!(imported.to_string().contains("orphan_request_scoped_event"));
    }

    #[test]
    fn strict_mode_max_requests_overflow_invalid_stage_still_fails() {
        let spans = vec![
            SpanRecord::new("req-1", 100, 200)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_ROUTE, "/a"),
            SpanRecord::new("req-2", 300, 400)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r2")
                .field(TT_ROUTE, "/a"),
            SpanRecord::new("st-2", 320, 450)
                .field(TT_KIND, "stage")
                .field(TT_REQUEST_ID, "r2")
                .field(TT_STAGE, "db"),
        ];
        let err = run_from_span_records(
            spans,
            ImportOptions::new("checkout")
                .strict(true)
                .capture_limits_override(tailtriage_core::CaptureLimitsOverride {
                    max_requests: Some(1),
                    max_stages: None,
                    max_queues: None,
                    ..tailtriage_core::CaptureLimitsOverride::default()
                }),
        )
        .expect_err("strict import should fail for out-of-window overflow stage");
        assert!(matches!(err, ImportError::StrictViolation(_)));
        let msg = err.to_string();
        assert!(msg.contains("orphan_request_scoped_event"));
        assert!(!msg.contains("valid but not retained due to max_requests"));
    }

    #[test]
    fn strict_mode_max_requests_overflow_invalid_queue_still_fails() {
        let spans = vec![
            SpanRecord::new("req-1", 100, 200)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_ROUTE, "/a"),
            SpanRecord::new("req-2", 300, 400)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r2")
                .field(TT_ROUTE, "/a"),
            SpanRecord::new("q-2", 320, 450)
                .field(TT_KIND, "queue")
                .field(TT_REQUEST_ID, "r2")
                .field(TT_QUEUE, "permits"),
        ];
        let err = run_from_span_records(
            spans,
            ImportOptions::new("checkout")
                .strict(true)
                .capture_limits_override(tailtriage_core::CaptureLimitsOverride {
                    max_requests: Some(1),
                    max_stages: None,
                    max_queues: None,
                    ..tailtriage_core::CaptureLimitsOverride::default()
                }),
        )
        .expect_err("strict import should fail for out-of-window overflow queue");
        assert!(matches!(err, ImportError::StrictViolation(_)));
        let msg = err.to_string();
        assert!(msg.contains("orphan_request_scoped_event"));
        assert!(!msg.contains("valid but not retained due to max_requests"));
    }

    #[test]
    fn strict_mode_max_requests_overflow_non_lexical_request_ids_follow_input_order() {
        let spans = vec![
            SpanRecord::new("req-1", 100, 200)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "z-retained")
                .field(TT_ROUTE, "/a"),
            SpanRecord::new("st-1", 120, 150)
                .field(TT_KIND, "stage")
                .field(TT_REQUEST_ID, "z-retained")
                .field(TT_STAGE, "db"),
            SpanRecord::new("q-1", 121, 130)
                .field(TT_KIND, "queue")
                .field(TT_REQUEST_ID, "z-retained")
                .field(TT_QUEUE, "permits"),
            SpanRecord::new("req-2", 300, 400)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "a-overflow")
                .field(TT_ROUTE, "/a"),
            SpanRecord::new("st-2", 320, 350)
                .field(TT_KIND, "stage")
                .field(TT_REQUEST_ID, "a-overflow")
                .field(TT_STAGE, "db"),
            SpanRecord::new("q-2", 321, 330)
                .field(TT_KIND, "queue")
                .field(TT_REQUEST_ID, "a-overflow")
                .field(TT_QUEUE, "permits"),
        ];
        let imported = run_from_span_records(
            spans,
            ImportOptions::new("checkout")
                .strict(true)
                .capture_limits_override(tailtriage_core::CaptureLimitsOverride {
                    max_requests: Some(1),
                    max_stages: None,
                    max_queues: None,
                    ..tailtriage_core::CaptureLimitsOverride::default()
                }),
        )
        .expect_err("strict import should reject overflow children whose parent was dropped by max_requests");
        assert!(matches!(imported, ImportError::StrictViolation(_)));
        assert!(imported.to_string().contains("orphan_request_scoped_event"));
    }

    #[test]
    fn invalid_success_warns_and_skips_stage_non_strict() {
        let spans = vec![
            SpanRecord::new("req", 10, 20)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_ROUTE, "/")
                .field(TT_OUTCOME, "ok"),
            SpanRecord::new("st", 1, 1_000)
                .field(TT_KIND, "stage")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_STAGE, "db")
                .field(TT_SUCCESS, 7_u64),
        ];
        let imported = run_from_span_records(spans, ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.run().stages.len(), 0);
        assert!(!imported.warnings().is_empty());
        assert_eq!(imported.run().metadata.started_at_unix_ms, 10);
        assert_eq!(imported.run().metadata.finished_at_unix_ms, 20);
    }

    #[test]
    fn invalid_success_errors_strict() {
        let spans = vec![
            SpanRecord::new("req", 10, 20)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_ROUTE, "/")
                .field(TT_OUTCOME, "ok"),
            SpanRecord::new("st", 1, 1_000)
                .field(TT_KIND, "stage")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_STAGE, "db")
                .field(TT_SUCCESS, 7_u64),
        ];
        assert!(run_from_span_records(spans, ImportOptions::new("svc").strict(true)).is_err());
    }

    #[test]
    fn invalid_depth_warns_and_skips_queue_non_strict() {
        let spans = vec![
            SpanRecord::new("req", 10, 20)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_ROUTE, "/")
                .field(TT_OUTCOME, "ok"),
            SpanRecord::new("q", 1, 1_000)
                .field(TT_KIND, "queue")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_QUEUE, "permits")
                .field(TT_DEPTH_AT_START, -1_i64),
        ];
        let imported = run_from_span_records(spans, ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.run().queues.len(), 0);
        assert!(!imported.warnings().is_empty());
        assert_eq!(imported.run().metadata.started_at_unix_ms, 10);
        assert_eq!(imported.run().metadata.finished_at_unix_ms, 20);
    }

    #[test]
    fn invalid_depth_errors_strict() {
        let spans = vec![
            SpanRecord::new("req", 10, 20)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_ROUTE, "/")
                .field(TT_OUTCOME, "ok"),
            SpanRecord::new("q", 1, 1_000)
                .field(TT_KIND, "queue")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_QUEUE, "permits")
                .field(TT_DEPTH_AT_START, 3.5_f64),
        ];
        assert!(run_from_span_records(spans, ImportOptions::new("svc").strict(true)).is_err());
    }

    #[test]
    fn valid_optional_fields_are_applied() {
        let spans = vec![
            SpanRecord::new("req", 1, 2)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_ROUTE, "/"),
            SpanRecord::new("st", 1, 2)
                .field(TT_KIND, "stage")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_STAGE, "db")
                .field(TT_SUCCESS, "false"),
            SpanRecord::new("q", 1, 2)
                .field(TT_KIND, "queue")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_QUEUE, "permits")
                .field(TT_DEPTH_AT_START, 9_i64),
        ];
        let run = run_from_span_records(spans, ImportOptions::new("svc"))
            .unwrap()
            .run()
            .clone();
        assert!(!run.stages[0].success);
        assert_eq!(run.queues[0].depth_at_start, Some(9));
    }

    #[test]
    fn mismatched_request_duration_warns_and_retains_duration_us_in_non_strict_mode() {
        let spans = vec![SpanRecord::new("req", 100, 101)
            .started_at_run_us(100_000)
            .finished_at_run_us(101_000)
            .duration_us(50_000)
            .field(TT_KIND, "request")
            .field(TT_REQUEST_ID, "r1")
            .field(TT_ROUTE, "/a")];
        let imported = run_from_span_records(spans, ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.run().requests[0].latency_us, 50_000);
        assert!(imported
            .warnings()
            .iter()
            .any(|w| w.message().contains("duration evidence was retained")));
        assert!(imported
            .warnings()
            .iter()
            .any(|w| w.message().contains("duration_mismatch")));
        assert!(imported
            .run()
            .metadata
            .lifecycle_warnings
            .iter()
            .any(|w| w.contains("duration evidence was retained")));
    }

    #[test]
    fn absent_request_duration_us_derives_from_timestamps() {
        let spans = vec![SpanRecord::new("req", 100, 101)
            .field(TT_KIND, "request")
            .field(TT_REQUEST_ID, "r1")
            .field(TT_ROUTE, "/a")
            .field(TT_OUTCOME, "ok")];
        let imported = run_from_span_records(spans, ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.run().requests[0].latency_us, 1_000);
        assert!(imported.warnings().iter().any(|w| w
            .message()
            .contains("precise_interval_validation_unavailable")));
        assert!(!imported
            .run()
            .metadata
            .lifecycle_warnings
            .iter()
            .any(|w| w.contains("precise_interval_validation_unavailable")));
    }

    #[test]
    fn mismatched_stage_duration_warns_and_retains_duration_us_in_non_strict_mode() {
        let spans = vec![
            SpanRecord::new("req", 99, 101)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_ROUTE, "/"),
            SpanRecord::new("stage", 100, 101)
                .started_at_run_us(100_000)
                .finished_at_run_us(101_000)
                .duration_us(50_000)
                .field(TT_KIND, "stage")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_STAGE, "db"),
        ];
        let imported = run_from_span_records(spans, ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.run().stages[0].latency_us, 50_000);
        assert!(imported
            .warnings()
            .iter()
            .any(|w| w.message().contains("duration evidence was retained")));
    }

    #[test]
    fn mismatched_queue_duration_warns_and_retains_duration_us_in_non_strict_mode() {
        let spans = vec![
            SpanRecord::new("req", 100, 110)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_ROUTE, "/a"),
            SpanRecord::new("q", 101, 102)
                .started_at_run_us(101_000)
                .finished_at_run_us(102_000)
                .duration_us(50_000)
                .field(TT_KIND, "queue")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_QUEUE, "permits"),
        ];
        let imported = run_from_span_records(spans, ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.run().queues[0].wait_us, 50_000);
        assert!(imported
            .warnings()
            .iter()
            .any(|w| w.message().contains("duration evidence was retained")));
    }

    #[test]
    fn strict_mode_rejects_mismatched_request_duration() {
        let spans = vec![SpanRecord::new("req", 100, 101)
            .started_at_run_us(100_000)
            .finished_at_run_us(101_000)
            .duration_us(50_000)
            .field(TT_KIND, "request")
            .field(TT_REQUEST_ID, "r1")
            .field(TT_ROUTE, "/a")];
        let err = run_from_span_records(spans, ImportOptions::new("svc").strict(true))
            .expect_err("strict import should reject mismatched duration_us");
        assert!(matches!(err, ImportError::StrictViolation(_)));
        let message = err.to_string();
        assert!(message.contains("duration_mismatch"));
    }

    #[test]
    fn strict_mode_rejects_contradictory_stage_duration() {
        let spans = vec![
            SpanRecord::new("req", 99, 101)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_ROUTE, "/"),
            SpanRecord::new("stage", 100, 101)
                .started_at_run_us(100_000)
                .finished_at_run_us(101_000)
                .duration_us(50_000)
                .field(TT_KIND, "stage")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_STAGE, "db"),
        ];
        assert!(matches!(
            run_from_span_records(spans, ImportOptions::new("svc").strict(true)),
            Err(ImportError::StrictViolation(_))
        ));
    }

    #[test]
    fn strict_mode_rejects_contradictory_queue_duration() {
        let spans = vec![
            SpanRecord::new("req", 100, 110)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_ROUTE, "/a"),
            SpanRecord::new("q", 101, 102)
                .started_at_run_us(101_000)
                .finished_at_run_us(102_000)
                .duration_us(50_000)
                .field(TT_KIND, "queue")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_QUEUE, "permits"),
        ];
        assert!(matches!(
            run_from_span_records(spans, ImportOptions::new("svc").strict(true)),
            Err(ImportError::StrictViolation(_))
        ));
    }

    #[test]
    fn duration_us_within_2000_microseconds_is_accepted() {
        let spans = vec![SpanRecord::new("req", 100, 101)
            .duration_us(2_999)
            .field(TT_KIND, "request")
            .field(TT_REQUEST_ID, "r1")
            .field(TT_ROUTE, "/a")];
        let imported = run_from_span_records(spans, ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.run().requests[0].latency_us, 2_999);
        assert!(!imported
            .warnings()
            .iter()
            .any(|w| w.message().contains("duration_mismatch")));
    }

    #[test]
    fn empty_service_name_is_rejected() {
        let err = run_from_span_records(Vec::new(), ImportOptions::new(" ")).unwrap_err();
        assert!(matches!(err, ImportError::EmptyServiceName));
    }
}
