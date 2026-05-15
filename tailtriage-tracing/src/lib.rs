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

pub use convention::{
    TT_DEPTH_AT_START, TT_KIND, TT_OUTCOME, TT_QUEUE, TT_REQUEST_ID, TT_ROUTE, TT_STAGE, TT_SUCCESS,
};
pub use error::ImportError;
pub use types::{FieldValue, ImportOptions, ImportWarning, ImportedRun, SpanKind, SpanRecord};

/// Converts in-memory [`SpanRecord`] values into a `tailtriage_core::Run`.
///
/// Spans without `tt.kind` are ignored silently.
/// In non-strict mode (`ImportOptions::strict(false)`), malformed tailtriage
/// spans are skipped and collected in warnings. In strict mode, the first
/// malformed tailtriage span returns an [`ImportError`].
///
/// # Errors
///
/// Returns an error when strict mode is enabled and an import violation occurs.
#[allow(clippy::too_many_lines)]
#[allow(clippy::needless_pass_by_value)]
pub fn run_from_span_records<I>(
    spans: I,
    options: ImportOptions,
) -> Result<ImportedRun, ImportError>
where
    I: IntoIterator<Item = SpanRecord>,
{
    let service_name = options.service_name().to_owned();
    let service_version = options.service_version_ref().map(ToOwned::to_owned);
    let run_id_override = options.run_id_ref().map(ToOwned::to_owned);
    let strict_mode = options.strict_mode();

    let mut warnings = Vec::new();
    let mut requests = Vec::new();
    let mut stages = Vec::new();
    let mut queues = Vec::new();
    let mut started_at = u64::MAX;
    let mut finished_at = 0_u64;

    for span in spans {
        let fields = span.fields();
        let Some(kind) = parse_string(fields.get(TT_KIND)) else {
            continue;
        };

        if span.finished_at_unix_ms() < span.started_at_unix_ms() {
            handle_violation(
                strict_mode,
                &mut warnings,
                format!(
                    "skipping span `{}`: finished_at_unix_ms ({}) is before started_at_unix_ms ({})",
                    span.name(),
                    span.finished_at_unix_ms(),
                    span.started_at_unix_ms()
                ),
            )?;
            continue;
        }

        started_at = started_at.min(span.started_at_unix_ms());
        finished_at = finished_at.max(span.finished_at_unix_ms());
        let latency_us = (span.finished_at_unix_ms() - span.started_at_unix_ms()) * 1_000;

        match kind {
            "request" => {
                let Some(request_id) = required_string_or_skip(
                    strict_mode,
                    &mut warnings,
                    fields.get(TT_REQUEST_ID),
                    TT_REQUEST_ID,
                    span.name(),
                )?
                else {
                    continue;
                };
                let Some(route) = required_string_or_skip(
                    strict_mode,
                    &mut warnings,
                    fields.get(TT_ROUTE),
                    TT_ROUTE,
                    span.name(),
                )?
                else {
                    continue;
                };
                let outcome = parse_string(fields.get(TT_OUTCOME))
                    .unwrap_or("ok")
                    .to_owned();
                requests.push(tailtriage_core::RequestEvent {
                    request_id: request_id.to_owned(),
                    route: route.to_owned(),
                    kind: None,
                    started_at_unix_ms: span.started_at_unix_ms(),
                    finished_at_unix_ms: span.finished_at_unix_ms(),
                    latency_us,
                    outcome,
                });
            }
            "stage" => {
                let Some(request_id) = required_string_or_skip(
                    strict_mode,
                    &mut warnings,
                    fields.get(TT_REQUEST_ID),
                    TT_REQUEST_ID,
                    span.name(),
                )?
                else {
                    continue;
                };
                let Some(stage) = required_string_or_skip(
                    strict_mode,
                    &mut warnings,
                    fields.get(TT_STAGE),
                    TT_STAGE,
                    span.name(),
                )?
                else {
                    continue;
                };
                let success = parse_success(fields.get(TT_SUCCESS)).unwrap_or(true);
                stages.push(tailtriage_core::StageEvent {
                    request_id: request_id.to_owned(),
                    stage: stage.to_owned(),
                    started_at_unix_ms: span.started_at_unix_ms(),
                    finished_at_unix_ms: span.finished_at_unix_ms(),
                    latency_us,
                    success,
                });
            }
            "queue" => {
                let Some(request_id) = required_string_or_skip(
                    strict_mode,
                    &mut warnings,
                    fields.get(TT_REQUEST_ID),
                    TT_REQUEST_ID,
                    span.name(),
                )?
                else {
                    continue;
                };
                let Some(queue) = required_string_or_skip(
                    strict_mode,
                    &mut warnings,
                    fields.get(TT_QUEUE),
                    TT_QUEUE,
                    span.name(),
                )?
                else {
                    continue;
                };
                let depth_at_start = parse_depth(fields.get(TT_DEPTH_AT_START));
                queues.push(tailtriage_core::QueueEvent {
                    request_id: request_id.to_owned(),
                    queue: queue.to_owned(),
                    waited_from_unix_ms: span.started_at_unix_ms(),
                    waited_until_unix_ms: span.finished_at_unix_ms(),
                    wait_us: latency_us,
                    depth_at_start,
                });
            }
            other => {
                handle_violation(
                    strict_mode,
                    &mut warnings,
                    format!(
                        "unknown `{TT_KIND}` value `{other}` on span `{}`",
                        span.name()
                    ),
                )?;
            }
        }
    }

    let started_at_unix_ms = if started_at == u64::MAX {
        tailtriage_core::unix_time_ms()
    } else {
        started_at
    };
    let finished_at_unix_ms = if finished_at == 0 && started_at == u64::MAX {
        started_at_unix_ms
    } else {
        finished_at
    };
    let run_id = run_id_override.map_or_else(
        || format!("tracing-import-{started_at_unix_ms}-{finished_at_unix_ms}"),
        |value| value,
    );

    let lifecycle_warnings = warnings
        .iter()
        .map(ImportWarning::message)
        .filter(|m| m.contains("missing required field") || m.contains("skipping span"))
        .map(ToOwned::to_owned)
        .collect();
    let metadata = tailtriage_core::RunMetadata {
        run_id,
        service_name,
        service_version,
        started_at_unix_ms,
        finished_at_unix_ms,
        finalized_at_unix_ms: Some(finished_at_unix_ms),
        mode: tailtriage_core::CaptureMode::Light,
        effective_core_config: Some(tailtriage_core::EffectiveCoreConfig {
            mode: tailtriage_core::CaptureMode::Light,
            capture_limits: tailtriage_core::CaptureMode::Light.core_defaults(),
            strict_lifecycle: false,
        }),
        effective_tokio_sampler_config: None,
        host: None,
        pid: None,
        lifecycle_warnings,
        unfinished_requests: tailtriage_core::UnfinishedRequests::default(),
        run_end_reason: None,
    };
    let run = tailtriage_core::Run {
        schema_version: tailtriage_core::SCHEMA_VERSION,
        metadata,
        requests,
        stages,
        queues,
        inflight: Vec::new(),
        runtime_snapshots: Vec::new(),
        truncation: tailtriage_core::TruncationSummary::default(),
    };

    Ok(ImportedRun::new(run, warnings))
}

fn required_string_or_skip<'a>(
    strict_mode: bool,
    warnings: &mut Vec<ImportWarning>,
    value: Option<&'a FieldValue>,
    field: &'static str,
    span_name: &str,
) -> Result<Option<&'a str>, ImportError> {
    if let Some(value) = parse_string(value) {
        return Ok(Some(value));
    }
    let message = format!("missing required field `{field}` for tt span `{span_name}`");
    if strict_mode {
        return Err(ImportError::MissingField(field));
    }
    warnings.push(ImportWarning::new(message));
    Ok(None)
}

fn parse_string(value: Option<&FieldValue>) -> Option<&str> {
    match value {
        Some(FieldValue::String(s)) => Some(s.as_str()),
        _ => None,
    }
}

fn parse_success(value: Option<&FieldValue>) -> Option<bool> {
    match value {
        Some(FieldValue::Bool(b)) => Some(*b),
        Some(FieldValue::String(s)) if s.eq_ignore_ascii_case("true") => Some(true),
        Some(FieldValue::String(s)) if s.eq_ignore_ascii_case("false") => Some(false),
        _ => None,
    }
}

fn parse_depth(value: Option<&FieldValue>) -> Option<u64> {
    match value {
        Some(FieldValue::U64(v)) => Some(*v),
        Some(FieldValue::I64(v)) if *v >= 0 => u64::try_from(*v).ok(),
        _ => None,
    }
}

fn handle_violation(
    strict_mode: bool,
    warnings: &mut Vec<ImportWarning>,
    message: String,
) -> Result<(), ImportError> {
    if strict_mode {
        return Err(ImportError::StrictViolation(message));
    }
    warnings.push(ImportWarning::new(message));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_request() -> SpanRecord {
        SpanRecord::new("req", 10, 12)
            .field(TT_KIND, "request")
            .field(TT_REQUEST_ID, "r1")
            .field(TT_ROUTE, "/checkout")
    }

    #[test]
    fn request_only_conversion_creates_one_request_event() {
        let imported =
            run_from_span_records(vec![base_request()], ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.run().requests.len(), 1);
    }

    #[test]
    fn request_and_stage_convert() {
        let stage = SpanRecord::new("stage", 12, 14)
            .field(TT_KIND, "stage")
            .field(TT_REQUEST_ID, "r1")
            .field(TT_STAGE, "db");
        let imported =
            run_from_span_records(vec![base_request(), stage], ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.run().requests.len(), 1);
        assert_eq!(imported.run().stages.len(), 1);
    }

    #[test]
    fn request_and_queue_convert() {
        let queue = SpanRecord::new("queue", 12, 14)
            .field(TT_KIND, "queue")
            .field(TT_REQUEST_ID, "r1")
            .field(TT_QUEUE, "db_pool");
        let imported =
            run_from_span_records(vec![base_request(), queue], ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.run().requests.len(), 1);
        assert_eq!(imported.run().queues.len(), 1);
    }

    #[test]
    fn missing_optional_fields_use_defaults() {
        let stage = SpanRecord::new("stage", 12, 14)
            .field(TT_KIND, "stage")
            .field(TT_REQUEST_ID, "r1")
            .field(TT_STAGE, "db");
        let queue = SpanRecord::new("queue", 12, 14)
            .field(TT_KIND, "queue")
            .field(TT_REQUEST_ID, "r1")
            .field(TT_QUEUE, "q");
        let imported = run_from_span_records(
            vec![base_request(), stage, queue],
            ImportOptions::new("svc"),
        )
        .unwrap();
        assert_eq!(imported.run().requests[0].outcome, "ok");
        assert!(imported.run().stages[0].success);
        assert_eq!(imported.run().queues[0].depth_at_start, None);
    }

    #[test]
    fn missing_required_request_field_warns_non_strict() {
        let bad = SpanRecord::new("req", 1, 2)
            .field(TT_KIND, "request")
            .field(TT_REQUEST_ID, "r1");
        let imported = run_from_span_records(vec![bad], ImportOptions::new("svc")).unwrap();
        assert!(imported.run().requests.is_empty());
        assert_eq!(imported.warnings().len(), 1);
    }

    #[test]
    fn missing_required_request_field_errors_strict() {
        let bad = SpanRecord::new("req", 1, 2)
            .field(TT_KIND, "request")
            .field(TT_REQUEST_ID, "r1");
        let err =
            run_from_span_records(vec![bad], ImportOptions::new("svc").strict(true)).unwrap_err();
        assert!(matches!(err, ImportError::MissingField(TT_ROUTE)));
    }

    #[test]
    fn unknown_kind_warns_non_strict() {
        let bad = SpanRecord::new("x", 1, 2).field(TT_KIND, "weird");
        let imported = run_from_span_records(vec![bad], ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.warnings().len(), 1);
    }

    #[test]
    fn span_without_kind_is_ignored_silently() {
        let span = SpanRecord::new("normal", 1, 2).field("foo", "bar");
        let imported = run_from_span_records(vec![span], ImportOptions::new("svc")).unwrap();
        assert!(imported.warnings().is_empty());
    }

    #[test]
    fn inverted_timestamps_warn_or_error() {
        let bad = SpanRecord::new("req", 5, 2)
            .field(TT_KIND, "request")
            .field(TT_REQUEST_ID, "r1")
            .field(TT_ROUTE, "/");
        let imported = run_from_span_records(vec![bad.clone()], ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.warnings().len(), 1);
        let err =
            run_from_span_records(vec![bad], ImportOptions::new("svc").strict(true)).unwrap_err();
        assert!(matches!(err, ImportError::StrictViolation(_)));
    }

    #[test]
    fn empty_input_returns_valid_empty_run() {
        let imported = run_from_span_records(Vec::new(), ImportOptions::new("svc")).unwrap();
        assert!(imported.run().requests.is_empty());
        assert_eq!(
            imported.run().metadata.finished_at_unix_ms,
            imported.run().metadata.started_at_unix_ms
        );
    }

    #[test]
    fn runtime_and_inflight_are_empty() {
        let imported =
            run_from_span_records(vec![base_request()], ImportOptions::new("svc")).unwrap();
        assert!(imported.run().runtime_snapshots.is_empty());
        assert!(imported.run().inflight.is_empty());
    }
}
