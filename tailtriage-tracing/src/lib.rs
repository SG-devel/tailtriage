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
        update_min_max(&mut min_start, &mut max_finish, &span);

        let Some(kind) = get_string_field(&span, TT_KIND) else {
            continue;
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
                    let outcome = get_string_field(&span, TT_OUTCOME)
                        .map_or_else(|| "ok".to_owned(), ToOwned::to_owned);
                    requests.push(RequestEvent {
                        request_id,
                        route,
                        kind: None,
                        started_at_unix_ms: span.started_at_unix_ms(),
                        finished_at_unix_ms: span.finished_at_unix_ms(),
                        latency_us: (span.finished_at_unix_ms() - span.started_at_unix_ms()) * 1000,
                        outcome,
                    });
                }
            }
            "stage" => {
                let request_id =
                    required_string(&span, TT_REQUEST_ID, options.strict_mode(), &mut warnings)?;
                let stage = required_string(&span, TT_STAGE, options.strict_mode(), &mut warnings)?;
                if let (Some(request_id), Some(stage)) = (request_id, stage) {
                    let success =
                        parse_success(&span, options.strict_mode(), &mut warnings)?.unwrap_or(true);
                    stages.push(StageEvent {
                        request_id,
                        stage,
                        started_at_unix_ms: span.started_at_unix_ms(),
                        finished_at_unix_ms: span.finished_at_unix_ms(),
                        latency_us: (span.finished_at_unix_ms() - span.started_at_unix_ms()) * 1000,
                        success,
                    });
                }
            }
            "queue" => {
                let request_id =
                    required_string(&span, TT_REQUEST_ID, options.strict_mode(), &mut warnings)?;
                let queue = required_string(&span, TT_QUEUE, options.strict_mode(), &mut warnings)?;
                if let (Some(request_id), Some(queue)) = (request_id, queue) {
                    let depth_at_start =
                        parse_depth_at_start(&span, options.strict_mode(), &mut warnings)?;
                    queues.push(QueueEvent {
                        request_id,
                        queue,
                        waited_from_unix_ms: span.started_at_unix_ms(),
                        waited_until_unix_ms: span.finished_at_unix_ms(),
                        wait_us: (span.finished_at_unix_ms() - span.started_at_unix_ms()) * 1000,
                        depth_at_start,
                    });
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

fn get_string_field<'a>(span: &'a SpanRecord, key: &str) -> Option<&'a str> {
    match span.fields().get(key) {
        Some(FieldValue::String(value)) => Some(value.as_str()),
        _ => None,
    }
}

fn required_string(
    span: &SpanRecord,
    key: &'static str,
    strict: bool,
    warnings: &mut Vec<ImportWarning>,
) -> Result<Option<String>, ImportError> {
    if let Some(value) = get_string_field(span, key) {
        return Ok(Some(value.to_owned()));
    }

    strict_or_warn(
        strict,
        warnings,
        format!("missing required field '{key}' in span '{}'", span.name()),
    )?;
    Ok(None)
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

fn parse_success(
    span: &SpanRecord,
    strict: bool,
    warnings: &mut Vec<ImportWarning>,
) -> Result<Option<bool>, ImportError> {
    match span.fields().get(TT_SUCCESS) {
        Some(FieldValue::Bool(value)) => Ok(Some(*value)),
        Some(FieldValue::String(value)) if value.eq_ignore_ascii_case("true") => Ok(Some(true)),
        Some(FieldValue::String(value)) if value.eq_ignore_ascii_case("false") => Ok(Some(false)),
        Some(_) => {
            strict_or_warn(strict, warnings, format!("invalid field '{TT_SUCCESS}' in span '{}': expected bool or 'true'/'false' string", span.name()))?;
            Ok(None)
        }
        None => Ok(None),
    }
}

fn parse_depth_at_start(
    span: &SpanRecord,
    strict: bool,
    warnings: &mut Vec<ImportWarning>,
) -> Result<Option<u64>, ImportError> {
    match span.fields().get(TT_DEPTH_AT_START) {
        Some(FieldValue::U64(value)) => Ok(Some(*value)),
        Some(FieldValue::I64(value)) if *value >= 0 => {
            if let Ok(parsed) = u64::try_from(*value) {
                Ok(Some(parsed))
            } else {
                strict_or_warn(
                strict,
                warnings,
                format!(
                    "invalid field '{TT_DEPTH_AT_START}' in span '{}': expected non-negative integer",
                    span.name()
                ),
            )?;
                Ok(None)
            }
        }
        Some(_) => {
            strict_or_warn(strict, warnings, format!("invalid field '{TT_DEPTH_AT_START}' in span '{}': expected non-negative integer", span.name()))?;
            Ok(None)
        }
        None => Ok(None),
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
}
