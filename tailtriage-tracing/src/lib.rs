#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

//! Tracing intake bridge types for tailtriage triage workflows.
//!
//! This crate provides semantic `tt.*` keys, typed [`SpanRecord`] intake,
//! conversion to [`tailtriage_core::Run`] via [`run_from_span_records`],
//! JSONL import via [`import_jsonl_reader`] and [`import_jsonl_path`], and
//! live in-memory recording via [`TracingRecorder`] and [`TailtriageLayer`].
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
mod jsonl;
mod recorder;
#[cfg(feature = "tokio")]
/// Optional Tokio runtime sampler coupling for tracing sessions.
pub mod tokio;
mod types;

use tailtriage_core::{
    BuildError, CaptureMode, QueueEvent, RequestEvent, RunBuilder, RunBuilderOptions, StageEvent,
};

pub use convention::{
    TT_DEPTH_AT_START, TT_KIND, TT_OUTCOME, TT_QUEUE, TT_REQUEST_ID, TT_ROUTE, TT_STAGE, TT_SUCCESS,
};
pub use error::ImportError;
pub use jsonl::{import_jsonl_path, import_jsonl_reader};
pub use recorder::{
    RecorderLimits, TailtriageLayer, TracingRecorder, TracingRecorderBuilder,
    DEFAULT_MAX_COMPLETED_SPANS, DEFAULT_MAX_OPEN_SPANS,
};
pub use types::{FieldValue, ImportOptions, ImportWarning, ImportedRun, SpanKind, SpanRecord};

/// Converts in-memory tracing span records into a `tailtriage_core::Run`.
///
/// Spans without `tt.*` fields are ignored silently. In non-strict mode, malformed
/// `tt.*` spans are skipped and surfaced as warnings. In strict mode, the first
/// malformed `tt.*` span returns an [`ImportError`].
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
    let mut requests = Vec::new();
    let mut stages = Vec::new();
    let mut queues = Vec::new();
    let mut min_start: Option<u64> = None;
    let mut max_finish: Option<u64> = None;
    let mut request_outcome_default_count = 0_u64;
    let mut stage_success_default_count = 0_u64;

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
                    let Some(outcome) = parse_outcome(
                        &span,
                        options.strict_mode(),
                        &mut warnings,
                        &mut request_outcome_default_count,
                    )?
                    else {
                        continue;
                    };
                    requests.push(RequestEvent {
                        request_id,
                        route,
                        kind: None,
                        started_at_unix_ms: span.started_at_unix_ms(),
                        finished_at_unix_ms: span.finished_at_unix_ms(),
                        latency_us: span.duration_us_ref().unwrap_or(
                            (span.finished_at_unix_ms() - span.started_at_unix_ms())
                                .saturating_mul(1000),
                        ),
                        outcome,
                    });
                    update_min_max(&mut min_start, &mut max_finish, &span);
                }
            }
            SpanKind::Stage => {
                let request_id =
                    required_string(&span, TT_REQUEST_ID, options.strict_mode(), &mut warnings)?;
                let stage = required_string(&span, TT_STAGE, options.strict_mode(), &mut warnings)?;
                if let (Some(request_id), Some(stage)) = (request_id, stage) {
                    let success = match parse_success(
                        &span,
                        options.strict_mode(),
                        &mut warnings,
                        &mut stage_success_default_count,
                    )? {
                        OptionalField::Missing => true,
                        OptionalField::Value(success) => success,
                        OptionalField::Invalid => continue,
                    };
                    stages.push(StageEvent {
                        request_id,
                        stage,
                        started_at_unix_ms: span.started_at_unix_ms(),
                        finished_at_unix_ms: span.finished_at_unix_ms(),
                        latency_us: span.duration_us_ref().unwrap_or(
                            (span.finished_at_unix_ms() - span.started_at_unix_ms())
                                .saturating_mul(1000),
                        ),
                        success,
                    });
                    update_min_max(&mut min_start, &mut max_finish, &span);
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
                        waited_until_unix_ms: span.finished_at_unix_ms(),
                        wait_us: span.duration_us_ref().unwrap_or(
                            (span.finished_at_unix_ms() - span.started_at_unix_ms())
                                .saturating_mul(1000),
                        ),
                        depth_at_start,
                    });
                    update_min_max(&mut min_start, &mut max_finish, &span);
                }
            }
        }
    }
    if request_outcome_default_count > 0 {
        warnings.push(ImportWarning::new(format!(
            "{request_outcome_default_count} request span(s) missing optional '{TT_OUTCOME}'; assumed 'ok'"
        )));
    }
    if stage_success_default_count > 0 {
        warnings.push(ImportWarning::new(format!(
            "{stage_success_default_count} stage span(s) missing optional '{TT_SUCCESS}'; assumed true"
        )));
    }

    let started_at_unix_ms = min_start.unwrap_or_else(tailtriage_core::unix_time_ms);
    let finished_at_unix_ms = max_finish.unwrap_or(started_at_unix_ms);
    let run_id = options.run_id_ref().map_or_else(
        || format!("tracing-import-{started_at_unix_ms}-{finished_at_unix_ms}"),
        ToOwned::to_owned,
    );

    let lifecycle_warnings = warnings
        .iter()
        .filter(|warning| is_lifecycle_warning(warning.message()))
        .map(|w| w.message().to_owned())
        .collect::<Vec<_>>();

    let mut builder_options = RunBuilderOptions::new(options.service_name())
        .run_id(run_id)
        .mode(CaptureMode::Light)
        .capture_limits(CaptureMode::Light.core_defaults())
        .strict_lifecycle(false)
        .started_at_unix_ms(started_at_unix_ms)
        .finished_at_unix_ms(finished_at_unix_ms)
        .finalized_at_unix_ms(finished_at_unix_ms);

    if let Some(service_version) = options.service_version_ref() {
        builder_options = builder_options.service_version(service_version);
    }

    let mut run_builder = RunBuilder::new(builder_options).map_err(|err| match err {
        BuildError::EmptyServiceName => ImportError::EmptyServiceName,
    })?;

    for request in requests {
        run_builder.push_request(request);
    }
    for stage in stages {
        run_builder.push_stage(stage);
    }
    for queue in queues {
        run_builder.push_queue(queue);
    }
    for warning in &lifecycle_warnings {
        run_builder.add_lifecycle_warning(warning.clone());
    }

    let run = run_builder.finish();

    Ok(ImportedRun::new(run, warnings))
}

fn validate_service_name(service_name: &str) -> Result<(), ImportError> {
    if service_name.trim().is_empty() {
        return Err(ImportError::EmptyServiceName);
    }
    Ok(())
}

fn update_min_max(min_start: &mut Option<u64>, max_finish: &mut Option<u64>, span: &SpanRecord) {
    *min_start = Some(min_start.map_or(span.started_at_unix_ms(), |current| {
        current.min(span.started_at_unix_ms())
    }));
    *max_finish = Some(max_finish.map_or(span.finished_at_unix_ms(), |current| {
        current.max(span.finished_at_unix_ms())
    }));
}

fn span_has_tailtriage_field(span: &SpanRecord) -> bool {
    span.fields().keys().any(|key| key.starts_with("tt."))
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

fn required_string(
    span: &SpanRecord,
    key: &'static str,
    strict: bool,
    warnings: &mut Vec<ImportWarning>,
) -> Result<Option<String>, ImportError> {
    match get_string_field_state(span, key) {
        StringFieldState::Value(value) => Ok(Some(value.to_owned())),
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

fn is_lifecycle_warning(message: &str) -> bool {
    message.starts_with("skipped span")
        || message.starts_with("missing required field")
        || message.starts_with("invalid field")
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

fn parse_outcome(
    span: &SpanRecord,
    strict: bool,
    warnings: &mut Vec<ImportWarning>,
    default_count: &mut u64,
) -> Result<Option<String>, ImportError> {
    match get_string_field_state(span, TT_OUTCOME) {
        StringFieldState::Missing => {
            *default_count += 1;
            Ok(Some("ok".to_owned()))
        }
        StringFieldState::Value(value) => Ok(Some(value.to_owned())),
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
    default_count: &mut u64,
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
        None => {
            *default_count += 1;
            Ok(OptionalField::Missing)
        }
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
    fn span_with_tt_fields_but_missing_kind_warns_and_skips_non_strict() {
        let spans = vec![SpanRecord::new("http.request", 1, 2)
            .field(TT_REQUEST_ID, "r1")
            .field(TT_ROUTE, "/checkout")];
        let imported = run_from_span_records(spans, ImportOptions::new("svc")).unwrap();
        assert!(imported.run().requests.is_empty());
        assert_eq!(imported.warnings().len(), 1);
        assert!(imported.warnings()[0]
            .message()
            .contains("missing required field 'tt.kind' in span 'http.request'"));
    }

    #[test]
    fn span_with_tt_fields_but_missing_kind_errors_in_strict_mode() {
        let spans = vec![SpanRecord::new("http.request", 1, 2)
            .field(TT_REQUEST_ID, "r1")
            .field(TT_ROUTE, "/checkout")];
        let err = run_from_span_records(spans, ImportOptions::new("svc").strict(true)).unwrap_err();
        assert!(matches!(err, ImportError::StrictViolation(_)));
    }

    #[test]
    fn missing_optional_defaults_emit_aggregate_warnings() {
        let spans = vec![
            SpanRecord::new("req-1", 1, 2)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_ROUTE, "/"),
            SpanRecord::new("req-2", 3, 4)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r2")
                .field(TT_ROUTE, "/"),
            SpanRecord::new("st-1", 1, 2)
                .field(TT_KIND, "stage")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_STAGE, "db"),
            SpanRecord::new("st-2", 3, 4)
                .field(TT_KIND, "stage")
                .field(TT_REQUEST_ID, "r2")
                .field(TT_STAGE, "cache"),
            SpanRecord::new("q-1", 1, 2)
                .field(TT_KIND, "queue")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_QUEUE, "admission"),
        ];
        let imported = run_from_span_records(spans, ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.run().requests[0].outcome, "ok");
        assert!(imported.run().stages[0].success);
        assert_eq!(imported.run().queues[0].depth_at_start, None);
        assert_eq!(imported.warnings().len(), 2);
        assert!(imported.warnings().iter().any(|w| w
            .message()
            .contains("2 request span(s) missing optional 'tt.outcome'")));
        assert!(imported.warnings().iter().any(|w| w
            .message()
            .contains("2 stage span(s) missing optional 'tt.success'")));
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
    fn unknown_kind_does_not_affect_metadata_bounds_and_lifecycle_warning() {
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
            .all(|w| !w.contains("unknown tt.kind")));
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
    fn invalid_success_warns_and_skips_stage_non_strict() {
        let spans = vec![
            SpanRecord::new("req", 10, 20)
                .field(TT_KIND, "request")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_ROUTE, "/"),
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
                .field(TT_ROUTE, "/"),
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
                .field(TT_ROUTE, "/"),
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
                .field(TT_ROUTE, "/"),
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
            SpanRecord::new("st", 10, 20)
                .field(TT_KIND, "stage")
                .field(TT_REQUEST_ID, "r1")
                .field(TT_STAGE, "db")
                .field(TT_SUCCESS, "false"),
            SpanRecord::new("q", 21, 30)
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
    fn span_duration_us_is_used_for_stage_latency() {
        let spans = vec![SpanRecord::new("stage", 100, 100)
            .duration_us(123)
            .field(TT_KIND, "stage")
            .field(TT_REQUEST_ID, "r1")
            .field(TT_STAGE, "db")];
        let run = run_from_span_records(spans, ImportOptions::new("svc"))
            .unwrap()
            .run()
            .clone();
        assert_eq!(run.stages[0].latency_us, 123);
    }

    #[test]
    fn span_duration_us_is_used_for_request_latency() {
        let spans = vec![SpanRecord::new("req", 100, 100)
            .duration_us(456)
            .field(TT_KIND, "request")
            .field(TT_REQUEST_ID, "r1")
            .field(TT_ROUTE, "/a")];
        let run = run_from_span_records(spans, ImportOptions::new("svc"))
            .unwrap()
            .run()
            .clone();
        assert_eq!(run.requests[0].latency_us, 456);
    }

    #[test]
    fn empty_service_name_is_rejected() {
        let err = run_from_span_records(Vec::new(), ImportOptions::new(" ")).unwrap_err();
        assert!(matches!(err, ImportError::EmptyServiceName));
    }
}
