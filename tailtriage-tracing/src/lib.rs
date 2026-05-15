#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

//! Tracing intake bridge types for tailtriage triage workflows.
//!
//! This crate provides semantic keys and typed records for importing
//! tracing-shaped span data into [`tailtriage_core::Run`].
//! It intentionally does not provide JSONL parsing, a `tracing` layer,
//! OpenTelemetry integration, or analyzer behavior changes.
//!
//! # Example
//!
//! ```
//! use tailtriage_tracing::{
//!     ImportOptions, SpanRecord, TT_DEPTH_AT_START, TT_KIND, TT_REQUEST_ID, TT_ROUTE, TT_SUCCESS,
//! };
//!
//! let record = SpanRecord::new("http.request", 1_700_000_000_000, 1_700_000_000_120)
//!     .field(TT_KIND, "request")
//!     .field(TT_REQUEST_ID, "req-42")
//!     .field(TT_ROUTE, "/checkout")
//!     .field(TT_SUCCESS, true)
//!     .field(TT_DEPTH_AT_START, 7_u64);
//!
//! let options = ImportOptions::new("checkout-service").strict(false);
//! assert_eq!(record.name(), "http.request");
//! assert_eq!(options.service_name(), "checkout-service");
//! ```

mod convention;
mod error;
mod jsonl;
mod types;

use tailtriage_core::{
    CaptureMode, EffectiveCoreConfig, QueueEvent, RequestEvent, Run, RunMetadata, StageEvent,
    TruncationSummary, UnfinishedRequests, SCHEMA_VERSION,
};

pub use convention::{
    TT_DEPTH_AT_START, TT_KIND, TT_OUTCOME, TT_QUEUE, TT_REQUEST_ID, TT_ROUTE, TT_STAGE, TT_SUCCESS,
};
pub use error::ImportError;
pub use types::{FieldValue, ImportOptions, ImportWarning, ImportedRun, SpanKind, SpanRecord};

/// Imports newline-delimited JSON span records and converts them into a
/// `tailtriage_core::Run` using [`run_from_span_records`].
///
/// This phase supports normalized records and close-event-like records that
/// carry explicit start/end unix-ms timestamps.
///
/// # Errors
///
/// Returns [`ImportError`] for malformed JSON and path/reader failures.
pub fn import_jsonl_reader<R: std::io::Read>(
    reader: R,
    options: ImportOptions,
) -> Result<ImportedRun, ImportError> {
    jsonl::import_jsonl_reader(reader, options)
}

/// Imports newline-delimited JSON from a path and converts it into a
/// `tailtriage_core::Run` using [`run_from_span_records`].
///
/// # Errors
///
/// Returns [`ImportError`] for I/O failures, malformed JSON, and strict-mode
/// conversion violations.
pub fn import_jsonl_path(
    path: impl AsRef<std::path::Path>,
    options: ImportOptions,
) -> Result<ImportedRun, ImportError> {
    jsonl::import_jsonl_path(path, options)
}

/// Converts in-memory tracing span records into a `tailtriage_core::Run`.
///
/// Spans without [`TT_KIND`] are ignored silently. In non-strict mode, malformed
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
    let mut warnings = Vec::new();
    let mut requests = Vec::new();
    let mut stages = Vec::new();
    let mut queues = Vec::new();
    let mut min_start: Option<u64> = None;
    let mut max_finish: Option<u64> = None;

    for span in spans {
        let kind = match get_string_field_state(&span, TT_KIND) {
            StringFieldState::Missing => continue,
            StringFieldState::Value(kind) => kind,
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
            "request" => {
                let request_id =
                    required_string(&span, TT_REQUEST_ID, options.strict_mode(), &mut warnings)?;
                let route = required_string(&span, TT_ROUTE, options.strict_mode(), &mut warnings)?;
                if let (Some(request_id), Some(route)) = (request_id, route) {
                    let Some(outcome) = parse_outcome(&span, options.strict_mode(), &mut warnings)?
                    else {
                        continue;
                    };
                    requests.push(RequestEvent {
                        request_id,
                        route,
                        kind: None,
                        started_at_unix_ms: span.started_at_unix_ms(),
                        finished_at_unix_ms: span.finished_at_unix_ms(),
                        latency_us: (span.finished_at_unix_ms() - span.started_at_unix_ms()) * 1000,
                        outcome,
                    });
                    update_min_max(&mut min_start, &mut max_finish, &span);
                }
            }
            "stage" => {
                let request_id =
                    required_string(&span, TT_REQUEST_ID, options.strict_mode(), &mut warnings)?;
                let stage = required_string(&span, TT_STAGE, options.strict_mode(), &mut warnings)?;
                if let (Some(request_id), Some(stage)) = (request_id, stage) {
                    let success = match parse_success(&span, options.strict_mode(), &mut warnings)?
                    {
                        OptionalField::Missing => true,
                        OptionalField::Value(success) => success,
                        OptionalField::Invalid => continue,
                    };
                    stages.push(StageEvent {
                        request_id,
                        stage,
                        started_at_unix_ms: span.started_at_unix_ms(),
                        finished_at_unix_ms: span.finished_at_unix_ms(),
                        latency_us: (span.finished_at_unix_ms() - span.started_at_unix_ms()) * 1000,
                        success,
                    });
                    update_min_max(&mut min_start, &mut max_finish, &span);
                }
            }
            "queue" => {
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
                        wait_us: (span.finished_at_unix_ms() - span.started_at_unix_ms()) * 1000,
                        depth_at_start,
                    });
                    update_min_max(&mut min_start, &mut max_finish, &span);
                }
            }
            other => {
                strict_or_warn(
                    options.strict_mode(),
                    &mut warnings,
                    format!("unknown tt.kind '{other}' in span '{}'", span.name()),
                )?;
            }
        }
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

    let metadata = RunMetadata {
        run_id,
        service_name: options.service_name().to_owned(),
        service_version: options.service_version_ref().map(ToOwned::to_owned),
        started_at_unix_ms,
        finished_at_unix_ms,
        finalized_at_unix_ms: Some(finished_at_unix_ms),
        mode: CaptureMode::Light,
        effective_core_config: Some(EffectiveCoreConfig {
            mode: CaptureMode::Light,
            capture_limits: CaptureMode::Light.core_defaults(),
            strict_lifecycle: false,
        }),
        effective_tokio_sampler_config: None,
        host: None,
        pid: None,
        lifecycle_warnings,
        unfinished_requests: UnfinishedRequests::default(),
        run_end_reason: None,
    };

    let run = Run {
        schema_version: SCHEMA_VERSION,
        metadata,
        requests,
        stages,
        queues,
        inflight: Vec::new(),
        runtime_snapshots: Vec::new(),
        truncation: TruncationSummary::default(),
    };

    Ok(ImportedRun::new(run, warnings))
}

fn update_min_max(min_start: &mut Option<u64>, max_finish: &mut Option<u64>, span: &SpanRecord) {
    *min_start = Some(min_start.map_or(span.started_at_unix_ms(), |current| {
        current.min(span.started_at_unix_ms())
    }));
    *max_finish = Some(max_finish.map_or(span.finished_at_unix_ms(), |current| {
        current.max(span.finished_at_unix_ms())
    }));
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
) -> Result<Option<String>, ImportError> {
    match get_string_field_state(span, TT_OUTCOME) {
        StringFieldState::Missing => Ok(Some("ok".to_owned())),
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
    fn span_without_kind_ignored_silently() {
        let spans = vec![SpanRecord::new("x", 1, 2).field("a", "b")];
        let imported = run_from_span_records(spans, ImportOptions::new("svc")).unwrap();
        assert!(imported.warnings().is_empty());
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
    fn empty_input_returns_valid_run_with_zero_events() {
        let imported = run_from_span_records(Vec::new(), ImportOptions::new("svc")).unwrap();
        assert!(imported.run().requests.is_empty());
        assert!(imported.run().stages.is_empty());
        assert!(imported.run().queues.is_empty());
        assert_eq!(
            imported.run().metadata.finished_at_unix_ms,
            imported.run().metadata.started_at_unix_ms
        );
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
}
