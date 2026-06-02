#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

//! Tracing intake bridge types for tailtriage triage workflows.
//!
//! This crate provides semantic `tt.*` keys, typed [`SpanRecord`] intake,
//! and conversion to [`tailtriage_core::Run`] via [`run_from_span_records`].
//! The `jsonl` feature adds JSONL import APIs.
//! The `live` feature adds live in-memory recording APIs.
//! It does not implement OpenTelemetry/OTLP and does not change analyzer behavior.
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

use std::collections::{BTreeMap, BTreeSet};
use tailtriage_core::{
    BuildError, QueueEvent, RequestEvent, RunBuilder, RunBuilderOptions, StageEvent,
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
    validate_service_name(options.service_name())?;
    let mut warnings = Vec::new();
    let mut parsed_requests = Vec::new();
    let mut parsed_stages = Vec::new();
    let mut queues = Vec::new();

    for span in spans {
        let kind = match get_string_field_state(&span, TT_KIND) {
            StringFieldState::Missing => {
                if span_has_tailtriage_field(&span) {
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
            let message = format!(
                "skipped span '{}' due to inverted timestamps: start={} finish={}",
                span.name(),
                span.started_at_unix_ms(),
                span.finished_at_unix_ms()
            );
            strict_or_warn(options.strict_mode(), &mut warnings, message)?;
            continue;
        }

        match kind {
            SpanKind::Request => {
                let request_id =
                    required_string(&span, TT_REQUEST_ID, options.strict_mode(), &mut warnings)?;
                let route = required_string(&span, TT_ROUTE, options.strict_mode(), &mut warnings)?;
                if let (Some(request_id), Some(route)) = (request_id, route) {
                    let Some((outcome, outcome_defaulted)) =
                        parse_outcome(&span, options.strict_mode(), &mut warnings)?
                    else {
                        continue;
                    };
                    parsed_requests.push(ParsedRequestEvent {
                        event: RequestEvent {
                            request_id,
                            route,
                            kind: None,
                            started_at_unix_ms: span.started_at_unix_ms(),
                            started_at_run_us: span.started_at_run_us_ref(),
                            finished_at_unix_ms: span.finished_at_unix_ms(),
                            finished_at_run_us: span.finished_at_run_us_ref(),
                            latency_us: elapsed_duration_us(
                                &span,
                                options.strict_mode(),
                                &mut warnings,
                            )?,
                            outcome,
                        },
                        outcome_defaulted,
                    });
                }
            }
            SpanKind::Stage => {
                let request_id =
                    required_string(&span, TT_REQUEST_ID, options.strict_mode(), &mut warnings)?;
                let stage = required_string(&span, TT_STAGE, options.strict_mode(), &mut warnings)?;
                if let (Some(request_id), Some(stage)) = (request_id, stage) {
                    let success_field = parse_success(&span, options.strict_mode(), &mut warnings)?;
                    let success = match success_field {
                        OptionalField::Missing => true,
                        OptionalField::Value(success) => success,
                        OptionalField::Invalid => continue,
                    };
                    parsed_stages.push(ParsedStageEvent {
                        event: StageEvent {
                            request_id,
                            stage,
                            started_at_unix_ms: span.started_at_unix_ms(),
                            started_at_run_us: span.started_at_run_us_ref(),
                            finished_at_unix_ms: span.finished_at_unix_ms(),
                            finished_at_run_us: span.finished_at_run_us_ref(),
                            latency_us: elapsed_duration_us(
                                &span,
                                options.strict_mode(),
                                &mut warnings,
                            )?,
                            success,
                        },
                        success_defaulted: matches!(success_field, OptionalField::Missing),
                    });
                }
            }
            SpanKind::Queue => {
                let request_id =
                    required_string(&span, TT_REQUEST_ID, options.strict_mode(), &mut warnings)?;
                let queue = required_string(&span, TT_QUEUE, options.strict_mode(), &mut warnings)?;
                if let (Some(request_id), Some(queue)) = (request_id, queue) {
                    let depth_at_start =
                        match parse_depth_at_start(&span, options.strict_mode(), &mut warnings)? {
                            OptionalField::Missing => None,
                            OptionalField::Value(depth) => Some(depth),
                            OptionalField::Invalid => continue,
                        };
                    queues.push(QueueEvent {
                        request_id,
                        queue,
                        waited_from_unix_ms: span.started_at_unix_ms(),
                        waited_from_run_us: span.started_at_run_us_ref(),
                        waited_until_unix_ms: span.finished_at_unix_ms(),
                        waited_until_run_us: span.finished_at_run_us_ref(),
                        wait_us: elapsed_duration_us(&span, options.strict_mode(), &mut warnings)?,
                        depth_at_start,
                    });
                }
            }
        }
    }
    let mode = options.mode_value();
    let capture_limits = options.resolved_capture_limits();
    dedupe_retained_requests(
        &mut parsed_requests,
        capture_limits.max_requests,
        options.strict_mode(),
        &mut warnings,
    )?;
    let request_outcome_default_count = parsed_requests
        .iter()
        .take(capture_limits.max_requests)
        .filter(|request| request.outcome_defaulted)
        .count();
    if request_outcome_default_count > 0 {
        warnings.push(ImportWarning::new(format!(
            "{request_outcome_default_count} request span(s) missing optional '{TT_OUTCOME}'; assumed 'ok'"
        )));
    }
    let requests: Vec<RequestEvent> = parsed_requests
        .into_iter()
        .map(|request| request.event)
        .collect();
    let all_valid_request_intervals = all_valid_request_intervals(&requests);
    let retained_request_intervals =
        retained_request_intervals(&requests, capture_limits.max_requests);
    let mut dropped_children_due_to_request_retention =
        DroppedChildrenDueToRequestRetention::default();
    filter_correlated_parsed_stages(
        &mut parsed_stages,
        &all_valid_request_intervals,
        &retained_request_intervals,
        options.strict_mode(),
        &mut warnings,
        &mut dropped_children_due_to_request_retention,
    )?;
    filter_correlated_queues(
        &mut queues,
        &all_valid_request_intervals,
        &retained_request_intervals,
        options.strict_mode(),
        &mut warnings,
        &mut dropped_children_due_to_request_retention,
    )?;
    let stage_success_default_count = parsed_stages
        .iter()
        .take(capture_limits.max_stages)
        .filter(|stage| stage.success_defaulted)
        .count();
    if stage_success_default_count > 0 {
        warnings.push(ImportWarning::new(format!(
            "{stage_success_default_count} stage span(s) missing optional '{TT_SUCCESS}'; assumed true"
        )));
    }
    let stages: Vec<StageEvent> = parsed_stages.into_iter().map(|stage| stage.event).collect();
    let retained_requests = &requests[..requests.len().min(capture_limits.max_requests)];
    let retained_stages = &stages[..stages.len().min(capture_limits.max_stages)];
    let retained_queues = &queues[..queues.len().min(capture_limits.max_queues)];

    let (started_at_unix_ms, finished_at_unix_ms) =
        retained_event_time_bounds(retained_requests, retained_stages, retained_queues)
            .unwrap_or_else(|| {
                let now = tailtriage_core::unix_time_ms();
                (now, now)
            });
    let run_id = options.run_id_ref().map_or_else(
        || format!("tracing-import-{started_at_unix_ms}-{finished_at_unix_ms}"),
        ToOwned::to_owned,
    );

    let mut builder_options = RunBuilderOptions::new(options.service_name())
        .run_id(run_id)
        .mode(mode)
        .capture_limits(capture_limits)
        .strict_lifecycle(false)
        .started_at_unix_ms(started_at_unix_ms)
        .finished_at_unix_ms(finished_at_unix_ms)
        .finalized_at_unix_ms(finished_at_unix_ms);

    if let Some(service_version) = options.service_version_ref() {
        builder_options = builder_options.service_version(service_version);
    }

    let mut run_builder = RunBuilder::new(builder_options).map_err(|err| match err {
        BuildError::EmptyServiceName => ImportError::EmptyServiceName,
        BuildError::InvalidRunTimeBounds {
            started_at_unix_ms,
                    finished_at_unix_ms,
        } => ImportError::InvalidField {
            field: "tt.finished_at_unix_ms",
            reason: format!(
                "finished_at_unix_ms ({finished_at_unix_ms}) must be >= started_at_unix_ms ({started_at_unix_ms})"
            ),
        },
        BuildError::InvalidFinalizationTime {
            finished_at_unix_ms,
            finalized_at_unix_ms,
        } => ImportError::InvalidField {
            field: "tt.finalized_at_unix_ms",
            reason: format!(
                "finalized_at_unix_ms ({finalized_at_unix_ms}) must be >= finished_at_unix_ms ({finished_at_unix_ms})"
            ),
        },
    })?;

    for request in requests {
        run_builder
            .push_request(request)
            .map_err(|err| ImportError::InvalidRunEvent(err.to_string()))?;
    }
    for stage in stages {
        run_builder
            .push_stage(stage)
            .map_err(|err| ImportError::InvalidRunEvent(err.to_string()))?;
    }
    for queue in queues {
        run_builder
            .push_queue(queue)
            .map_err(|err| ImportError::InvalidRunEvent(err.to_string()))?;
    }
    let mut run = run_builder.finish();
    run.truncation.dropped_stages = run
        .truncation
        .dropped_stages
        .saturating_add(dropped_children_due_to_request_retention.stages);
    run.truncation.dropped_queues = run
        .truncation
        .dropped_queues
        .saturating_add(dropped_children_due_to_request_retention.queues);
    if dropped_children_due_to_request_retention.stages > 0
        || dropped_children_due_to_request_retention.queues > 0
    {
        run.truncation.limits_hit = true;
    }
    attach_durable_conversion_warnings(&mut run, &warnings);

    Ok(ImportedRun::new(run, warnings))
}

#[derive(Clone, Copy)]
struct RequestInterval {
    started_at_unix_ms: u64,
    finished_at_unix_ms: u64,
}

fn dedupe_retained_requests(
    requests: &mut Vec<ParsedRequestEvent>,
    max_requests: usize,
    strict: bool,
    warnings: &mut Vec<ImportWarning>,
) -> Result<(), ImportError> {
    let mut seen = BTreeSet::new();
    let mut retained = Vec::with_capacity(requests.len());
    for request in requests.drain(..) {
        if retained.len() >= max_requests {
            retained.push(request);
            continue;
        }
        if !seen.insert(request.event.request_id.clone()) {
            strict_or_warn(
                strict,
                warnings,
                format!(
                    "duplicate tt.request_id '{}' is an input-quality problem; skipped later duplicate request event; child stage/queue evidence outside the retained first request interval may also be skipped",
                    request.event.request_id
                ),
            )?;
            continue;
        }
        retained.push(request);
    }
    *requests = retained;
    Ok(())
}

fn all_valid_request_intervals(requests: &[RequestEvent]) -> BTreeMap<String, RequestInterval> {
    let mut intervals = BTreeMap::new();
    for request in requests {
        intervals
            .entry(request.request_id.clone())
            .or_insert(RequestInterval {
                started_at_unix_ms: request.started_at_unix_ms,
                finished_at_unix_ms: request.finished_at_unix_ms,
            });
    }
    intervals
}

fn retained_request_intervals(
    requests: &[RequestEvent],
    max_requests: usize,
) -> BTreeMap<String, RequestInterval> {
    let mut intervals = BTreeMap::new();
    for request in requests.iter().take(max_requests) {
        intervals.insert(
            request.request_id.clone(),
            RequestInterval {
                started_at_unix_ms: request.started_at_unix_ms,
                finished_at_unix_ms: request.finished_at_unix_ms,
            },
        );
    }
    intervals
}

struct ParsedStageEvent {
    event: StageEvent,
    success_defaulted: bool,
}

struct ParsedRequestEvent {
    event: RequestEvent,
    outcome_defaulted: bool,
}

#[derive(Default)]
struct DroppedChildrenDueToRequestRetention {
    stages: u64,
    queues: u64,
}

fn interval_within_request_with_tolerance(
    child_start_ms: u64,
    child_finish_ms: u64,
    request_start_ms: u64,
    request_finish_ms: u64,
) -> bool {
    child_start_ms.saturating_add(TRACE_TIME_TOLERANCE_MS) >= request_start_ms
        && child_finish_ms <= request_finish_ms.saturating_add(TRACE_TIME_TOLERANCE_MS)
}

fn filter_correlated_parsed_stages(
    stages: &mut Vec<ParsedStageEvent>,
    all_valid_request_intervals: &BTreeMap<String, RequestInterval>,
    retained_request_intervals: &BTreeMap<String, RequestInterval>,
    strict: bool,
    warnings: &mut Vec<ImportWarning>,
    dropped_due_to_request_retention: &mut DroppedChildrenDueToRequestRetention,
) -> Result<(), ImportError> {
    let mut filtered = Vec::with_capacity(stages.len());
    for stage in stages.drain(..) {
        let request_id = stage.event.request_id.as_str();
        let Some(valid_interval) = all_valid_request_intervals.get(request_id) else {
            strict_or_warn(
                strict,
                warnings,
                format!(
                    "skipped stage span for request_id '{}' because no valid matching request event was imported",
                    stage.event.request_id
                ),
            )?;
            continue;
        };
        if !interval_within_request_with_tolerance(
            stage.event.started_at_unix_ms,
            stage.event.finished_at_unix_ms,
            valid_interval.started_at_unix_ms,
            valid_interval.finished_at_unix_ms,
        ) {
            strict_or_warn(
                strict,
                warnings,
                format!(
                    "skipped stage span '{}' for request_id '{}' because interval [{}, {}] falls outside request interval [{}, {}] beyond tolerance_ms={TRACE_TIME_TOLERANCE_MS}",
                    stage.event.stage,
                    stage.event.request_id,
                    stage.event.started_at_unix_ms,
                    stage.event.finished_at_unix_ms,
                    valid_interval.started_at_unix_ms,
                    valid_interval.finished_at_unix_ms
                ),
            )?;
            continue;
        }
        if !retained_request_intervals.contains_key(request_id) {
            dropped_due_to_request_retention.stages =
                dropped_due_to_request_retention.stages.saturating_add(1);
            warnings.push(ImportWarning::new(format!(
                "skipped stage span for request_id '{}' because the matching request was valid but not retained due to max_requests",
                stage.event.request_id
            )));
            continue;
        }
        filtered.push(stage);
    }
    *stages = filtered;
    Ok(())
}

fn filter_correlated_queues(
    queues: &mut Vec<QueueEvent>,
    all_valid_request_intervals: &BTreeMap<String, RequestInterval>,
    retained_request_intervals: &BTreeMap<String, RequestInterval>,
    strict: bool,
    warnings: &mut Vec<ImportWarning>,
    dropped_due_to_request_retention: &mut DroppedChildrenDueToRequestRetention,
) -> Result<(), ImportError> {
    let mut filtered = Vec::with_capacity(queues.len());
    for queue in queues.drain(..) {
        let request_id = queue.request_id.as_str();
        let Some(valid_interval) = all_valid_request_intervals.get(request_id) else {
            strict_or_warn(
                strict,
                warnings,
                format!(
                    "skipped queue span for request_id '{}' because no valid matching request event was imported",
                    queue.request_id
                ),
            )?;
            continue;
        };
        if !interval_within_request_with_tolerance(
            queue.waited_from_unix_ms,
            queue.waited_until_unix_ms,
            valid_interval.started_at_unix_ms,
            valid_interval.finished_at_unix_ms,
        ) {
            strict_or_warn(
                strict,
                warnings,
                format!(
                    "skipped queue span '{}' for request_id '{}' because interval [{}, {}] falls outside request interval [{}, {}] beyond tolerance_ms={TRACE_TIME_TOLERANCE_MS}",
                    queue.queue,
                    queue.request_id,
                    queue.waited_from_unix_ms,
                    queue.waited_until_unix_ms,
                    valid_interval.started_at_unix_ms,
                    valid_interval.finished_at_unix_ms
                ),
            )?;
            continue;
        }
        if !retained_request_intervals.contains_key(request_id) {
            dropped_due_to_request_retention.queues =
                dropped_due_to_request_retention.queues.saturating_add(1);
            warnings.push(ImportWarning::new(format!(
                "skipped queue span for request_id '{}' because the matching request was valid but not retained due to max_requests",
                queue.request_id
            )));
            continue;
        }
        filtered.push(queue);
    }
    *queues = filtered;
    Ok(())
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

pub(crate) const TRACE_TIME_TOLERANCE_US: u64 = 2_000;
pub(crate) const TRACE_TIME_TOLERANCE_MS: u64 = TRACE_TIME_TOLERANCE_US / 1_000;

pub(crate) fn duration_within_tolerance(
    duration_us: u64,
    started_at_unix_ms: u64,
    finished_at_unix_ms: u64,
) -> bool {
    let derived_us = timestamp_derived_duration_us(started_at_unix_ms, finished_at_unix_ms);
    duration_us.abs_diff(derived_us) <= TRACE_TIME_TOLERANCE_US
}

fn timestamp_derived_duration_us(started_at_unix_ms: u64, finished_at_unix_ms: u64) -> u64 {
    finished_at_unix_ms
        .saturating_sub(started_at_unix_ms)
        .saturating_mul(1000)
}

fn elapsed_duration_us(
    span: &SpanRecord,
    strict: bool,
    warnings: &mut Vec<ImportWarning>,
) -> Result<u64, ImportError> {
    let derived_us =
        timestamp_derived_duration_us(span.started_at_unix_ms(), span.finished_at_unix_ms());
    let Some(duration_us) = span.duration_us_ref() else {
        return Ok(derived_us);
    };
    if duration_within_tolerance(
        duration_us,
        span.started_at_unix_ms(),
        span.finished_at_unix_ms(),
    ) {
        return Ok(duration_us);
    }
    if strict {
        return Err(ImportError::StrictViolation(format!(
            "span '{}' duration_us differs from timestamp-derived duration beyond tolerance: duration_us={} timestamp_derived_duration_us={} tolerance_us={TRACE_TIME_TOLERANCE_US}; strict import rejected the mismatch; Unix timestamps remain wall-clock anchors",
            span.name(),
            duration_us,
            derived_us
        )));
    }
    warnings.push(ImportWarning::new(format!(
        "span '{}' duration_us differs from timestamp-derived duration beyond tolerance: duration_us={} timestamp_derived_duration_us={} tolerance_us={TRACE_TIME_TOLERANCE_US}; duration_us was retained as authoritative elapsed-time evidence; Unix timestamps remain wall-clock anchors",
        span.name(),
        duration_us,
        derived_us
    )));
    Ok(duration_us)
}

fn is_durable_conversion_warning(message: &str) -> bool {
    message.starts_with("skipped ")
        || message.starts_with("duplicate tt.request_id")
        || message.starts_with("missing required field")
        || message.starts_with("invalid field")
        || message.starts_with("unknown tt.kind")
        || message.contains("duration_us differs from timestamp-derived duration")
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
        let warning = "skipped stage span for request_id 'r-orphan' because no valid matching request event was imported";
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
        assert!(imported.warnings().iter().any(|w| w
            .message()
            .contains("skipped stage span for request_id 'r-orphan'")));
        assert!(imported.warnings().iter().all(|w| !w
            .message()
            .contains("missing optional 'tt.success'; assumed true")));
        assert!(run
            .metadata
            .lifecycle_warnings
            .iter()
            .all(|w| !w.contains("missing optional 'tt.success'; assumed true")));
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
                assert!(message.contains("no valid matching request event"));
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
        let warning = "skipped queue span for request_id 'r-orphan' because no valid matching request event was imported";
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
                assert!(message.contains("no valid matching request event"));
            }
            _ => panic!("expected StrictViolation"),
        }
    }

    #[test]
    fn non_strict_stage_before_request_start_is_skipped_and_warning_is_durable() {
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
        assert!(imported.run().stages.is_empty());
        let warning = "skipped stage span 'db' for request_id 'r1' because interval [97, 110] falls outside request interval [100, 120] beyond tolerance_ms=2";
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
    fn non_strict_stage_after_request_finish_is_skipped_and_warning_is_durable() {
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
        assert!(imported.run().stages.is_empty());
        let warning = "skipped stage span 'db' for request_id 'r1' because interval [110, 123] falls outside request interval [100, 120] beyond tolerance_ms=2";
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
    fn non_strict_queue_before_request_start_is_skipped_and_warning_is_durable() {
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
        assert!(imported.run().queues.is_empty());
        let warning = "skipped queue span 'permits' for request_id 'r1' because interval [97, 110] falls outside request interval [100, 120] beyond tolerance_ms=2";
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
    fn non_strict_queue_after_request_finish_is_skipped_and_warning_is_durable() {
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
        assert!(imported.run().queues.is_empty());
        let warning = "skipped queue span 'permits' for request_id 'r1' because interval [110, 123] falls outside request interval [100, 120] beyond tolerance_ms=2";
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
    fn strict_mode_fails_for_out_of_window_stage() {
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
        let err = run_from_span_records(spans, ImportOptions::new("svc").strict(true)).unwrap_err();
        assert!(matches!(err, ImportError::StrictViolation(_)));
    }

    #[test]
    fn strict_mode_fails_for_out_of_window_queue() {
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
        let err = run_from_span_records(spans, ImportOptions::new("svc").strict(true)).unwrap_err();
        assert!(matches!(err, ImportError::StrictViolation(_)));
    }

    #[test]
    fn stage_starting_1ms_before_request_is_retained_with_tolerance() {
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
    fn queue_ending_1ms_after_request_is_retained_with_tolerance() {
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
    fn non_strict_duplicate_request_id_skips_later_request() {
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
        assert_eq!(run.requests.len(), 1);
        assert_eq!(run.requests[0].route, "/a");
        assert_eq!(run.stages.len(), 0);
        assert_eq!(run.truncation.dropped_requests, 0);

        assert!(imported.warnings().iter().any(|w| w
            .message()
            .contains("duplicate tt.request_id 'dup' is an input-quality problem")));
        assert!(imported.warnings().iter().any(|w| w
            .message()
            .contains("skipped later duplicate request event")));
        assert!(imported.warnings().iter().any(|w| w
            .message()
            .contains("child stage/queue evidence outside the retained first request interval may also be skipped")));
        assert!(imported
            .run()
            .metadata
            .lifecycle_warnings
            .iter()
            .any(|w| w.contains("duplicate tt.request_id 'dup' is an input-quality problem")));
    }

    #[test]
    fn non_strict_skipped_duplicate_missing_outcome_does_not_warn_outcome_defaulted() {
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
        assert_eq!(run.requests.len(), 1);
        assert_eq!(run.requests[0].route, "/a");
        assert!(imported.warnings().iter().any(|w| w
            .message()
            .contains("duplicate tt.request_id 'dup' is an input-quality problem")));
        assert!(!imported.warnings().iter().any(|w| w
            .message()
            .contains("missing optional 'tt.outcome'; assumed 'ok'")));
        assert!(!run
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
            .any(|w| w.message().contains("duplicate tt.request_id")));
    }

    #[test]
    fn invalid_extreme_timestamps_do_not_affect_metadata_bounds_or_default_run_id() {
        let spans = vec![
            SpanRecord::new("req", 100, 120)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_ROUTE, "/a"),
            SpanRecord::new("stage-extreme", 1, 1_000_000)
                .field(TT_KIND, "stage")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_STAGE, "db"),
            SpanRecord::new("queue-extreme", 1, 1_000_000)
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
        let limits = CaptureMode::Light.core_defaults();
        let mut spans = Vec::new();
        for index in 0..limits.max_requests {
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
        let imported = run_from_span_records(spans, ImportOptions::new("svc")).unwrap();
        let run = imported.run();
        assert_eq!(run.requests.len(), limits.max_requests);
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
        let limits = CaptureMode::Light.core_defaults();
        let mut spans = vec![SpanRecord::new("req", 100, 120)
            .field(TT_KIND, "request")
            .field(TT_REQUEST_ID, "r1")
            .field(TT_ROUTE, "/a")
            .field(TT_OUTCOME, "ok")];
        for index in 0..limits.max_stages {
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
        let imported = run_from_span_records(spans, ImportOptions::new("svc")).unwrap();
        let run = imported.run();
        assert_eq!(run.stages.len(), limits.max_stages);
        assert_eq!(run.truncation.dropped_stages, 0);
        assert_eq!(run.metadata.started_at_unix_ms, 100);
        assert_eq!(run.metadata.finished_at_unix_ms, 120);
        assert!(!imported.warnings().iter().any(|w| w
            .message()
            .contains("missing optional 'tt.success'; assumed true")));
    }

    #[test]
    fn overflow_queue_does_not_affect_metadata_bounds() {
        let limits = CaptureMode::Light.core_defaults();
        let mut spans = vec![SpanRecord::new("req", 100, 120)
            .field(TT_KIND, "request")
            .field(TT_REQUEST_ID, "r1")
            .field(TT_ROUTE, "/a")
            .field(TT_OUTCOME, "ok")];
        for index in 0..limits.max_queues {
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
        let imported = run_from_span_records(spans, ImportOptions::new("svc")).unwrap();
        let run = imported.run();
        assert_eq!(run.queues.len(), limits.max_queues);
        assert_eq!(run.truncation.dropped_queues, 0);
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
        assert_eq!(imported.warnings().len(), 1);
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
        assert_eq!(imported.warnings().len(), 1);
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
        assert_eq!(imported.warnings().len(), 1);
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
        assert_eq!(imported.warnings().len(), 1);
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
        assert_eq!(imported.warnings().len(), 1);
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
        assert!(imported.warnings().iter().any(|w| w.message().contains(
            "skipped stage span for request_id 'r1' because no valid matching request event was imported"
        )));
        assert!(imported.warnings().iter().any(|w| w.message().contains(
            "skipped queue span for request_id 'r1' because no valid matching request event was imported"
        )));
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
    fn strict_mode_duplicate_request_id_overflow_only_children_fail_outside_interval() {
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
        .expect_err("strict import should fail when child only matches overflow duplicate request");
        assert!(matches!(err, ImportError::StrictViolation(_)));
        let msg = err.to_string();
        assert!(msg.contains("falls outside request interval"));
        assert!(!msg.contains("valid but not retained due to max_requests"));
    }

    #[test]
    fn non_strict_duplicate_request_id_overflow_only_children_warn_outside_interval() {
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
        assert_eq!(imported.run().stages.len(), 0);
        assert_eq!(imported.run().queues.len(), 0);
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
            .any(|msg| msg.contains("falls outside request interval")));
        assert!(warning_msgs
            .iter()
            .all(|msg| !msg.contains("valid but not retained due to max_requests")));

        let lifecycle_warnings = &imported.run().metadata.lifecycle_warnings;
        assert!(lifecycle_warnings
            .iter()
            .any(|msg| msg.contains("falls outside request interval")));
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
        .expect("strict import should succeed for valid overflow request children");

        assert_eq!(imported.run().requests.len(), 1);
        assert_eq!(imported.run().requests[0].request_id, "r1");
        assert_eq!(imported.run().stages.len(), 1);
        assert_eq!(imported.run().stages[0].request_id, "r1");
        assert_eq!(imported.run().queues.len(), 1);
        assert_eq!(imported.run().queues[0].request_id, "r1");
        assert_eq!(imported.run().truncation.dropped_requests, 1);
        assert_eq!(imported.run().truncation.dropped_stages, 1);
        assert_eq!(imported.run().truncation.dropped_queues, 1);
        assert!(imported.run().truncation.limits_hit);
        assert!(imported.warnings().iter().any(|w| w.message().contains(
            "skipped stage span for request_id 'r2' because the matching request was valid but not retained due to max_requests"
        )));
        assert!(imported.warnings().iter().any(|w| w.message().contains(
            "skipped queue span for request_id 'r2' because the matching request was valid but not retained due to max_requests"
        )));
        assert!(imported.warnings().iter().all(|w| !w
            .message()
            .contains("no retained request event was imported")));
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
        assert!(msg.contains("falls outside request interval"));
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
        assert!(msg.contains("falls outside request interval"));
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
        .expect("strict import should succeed and retain first input request");
        assert_eq!(imported.run().requests.len(), 1);
        assert_eq!(imported.run().requests[0].request_id, "z-retained");
        assert_eq!(imported.run().stages.len(), 1);
        assert_eq!(imported.run().stages[0].request_id, "z-retained");
        assert_eq!(imported.run().queues.len(), 1);
        assert_eq!(imported.run().queues[0].request_id, "z-retained");
        assert_eq!(imported.run().truncation.dropped_requests, 1);
        assert_eq!(imported.run().truncation.dropped_stages, 1);
        assert_eq!(imported.run().truncation.dropped_queues, 1);
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
        assert_eq!(imported.warnings().len(), 1);
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
        assert_eq!(imported.warnings().len(), 1);
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
            .duration_us(50_000)
            .field(TT_KIND, "request")
            .field(TT_REQUEST_ID, "r1")
            .field(TT_ROUTE, "/a")];
        let imported = run_from_span_records(spans, ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.run().requests[0].latency_us, 50_000);
        assert!(imported
            .warnings()
            .iter()
            .any(|w| w.message().contains("duration_us was retained")));
        assert!(imported.warnings().iter().any(|w| w
            .message()
            .contains("duration_us differs from timestamp-derived duration")));
        assert!(imported.warnings().iter().any(|w| w
            .message()
            .contains("Unix timestamps remain wall-clock anchors")));
        assert!(imported
            .run()
            .metadata
            .lifecycle_warnings
            .iter()
            .any(|w| w.contains("duration_us was retained")));
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
        assert!(imported.warnings().is_empty());
    }

    #[test]
    fn mismatched_stage_duration_warns_and_retains_duration_us_in_non_strict_mode() {
        let spans = vec![
            SpanRecord::new("req", 99, 101)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_ROUTE, "/"),
            SpanRecord::new("stage", 100, 101)
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
            .any(|w| w.message().contains("duration_us was retained")));
    }

    #[test]
    fn mismatched_queue_duration_warns_and_retains_duration_us_in_non_strict_mode() {
        let spans = vec![
            SpanRecord::new("req", 100, 110)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_ROUTE, "/a"),
            SpanRecord::new("q", 101, 102)
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
            .any(|w| w.message().contains("duration_us was retained")));
    }

    #[test]
    fn strict_mode_rejects_mismatched_request_duration() {
        let spans = vec![SpanRecord::new("req", 100, 101)
            .duration_us(50_000)
            .field(TT_KIND, "request")
            .field(TT_REQUEST_ID, "r1")
            .field(TT_ROUTE, "/a")];
        let err = run_from_span_records(spans, ImportOptions::new("svc").strict(true))
            .expect_err("strict import should reject mismatched duration_us");
        assert!(matches!(err, ImportError::StrictViolation(_)));
        let message = err.to_string();
        assert!(message.contains("duration_us differs from timestamp-derived duration"));
        assert!(message.contains("strict import rejected the mismatch"));
        assert!(!message.contains("duration_us was retained"));
    }

    #[test]
    fn strict_mode_rejects_contradictory_stage_duration() {
        let spans = vec![
            SpanRecord::new("req", 99, 101)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_ROUTE, "/"),
            SpanRecord::new("stage", 100, 101)
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
        assert!(!imported.warnings().iter().any(|w| w
            .message()
            .contains("duration_us differs from timestamp-derived duration")));
    }

    #[test]
    fn empty_service_name_is_rejected() {
        let err = run_from_span_records(Vec::new(), ImportOptions::new(" ")).unwrap_err();
        assert!(matches!(err, ImportError::EmptyServiceName));
    }
}
