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

use tailtriage_core::{
    CaptureMode, EffectiveCoreConfig, QueueEvent, RequestEvent, Run, RunMetadata, StageEvent,
    UnfinishedRequests, SCHEMA_VERSION,
};

/// Converts in-memory [`SpanRecord`] values into a valid [`tailtriage_core::Run`].
///
/// # Errors
/// Returns [`ImportError`] when strict mode rejects malformed `tt.*` spans.
#[allow(clippy::too_many_lines)]
pub fn run_from_span_records<I>(
    spans: I,
    options: ImportOptions,
) -> Result<ImportedRun, ImportError>
where
    I: IntoIterator<Item = SpanRecord>,
{
    let (service_name, service_version, explicit_run_id, strict) = options.into_parts();
    let mut warnings: Vec<ImportWarning> = Vec::new();
    let mut requests = Vec::new();
    let mut stages = Vec::new();
    let mut queues = Vec::new();

    let mut min_start: Option<u64> = None;
    let mut max_finish: Option<u64> = None;

    for span in spans {
        let started = span.started_at_unix_ms();
        let finished = span.finished_at_unix_ms();
        min_start = Some(min_start.map_or(started, |x| x.min(started)));
        max_finish = Some(max_finish.map_or(finished, |x| x.max(finished)));

        let Some(kind) = field_string(&span, TT_KIND) else {
            continue;
        };

        if finished < started {
            let message = format!(
                "skipped span `{}`: finished_at_unix_ms ({finished}) < started_at_unix_ms ({started})",
                span.name()
            );
            if strict {
                return Err(ImportError::StrictViolation(message));
            }
            warnings.push(ImportWarning::new(message));
            continue;
        }

        match kind {
            "request" => {
                let Some(request_id) =
                    required_string_field(&span, TT_REQUEST_ID, strict, &mut warnings)?
                else {
                    continue;
                };
                let Some(route) = required_string_field(&span, TT_ROUTE, strict, &mut warnings)?
                else {
                    continue;
                };
                let outcome =
                    optional_string_field(&span, TT_OUTCOME).unwrap_or_else(|| "ok".to_owned());
                let latency_us = finished.saturating_sub(started).saturating_mul(1_000);
                requests.push(RequestEvent {
                    request_id,
                    route,
                    kind: None,
                    started_at_unix_ms: started,
                    finished_at_unix_ms: finished,
                    latency_us,
                    outcome,
                });
            }
            "stage" => {
                let Some(request_id) =
                    required_string_field(&span, TT_REQUEST_ID, strict, &mut warnings)?
                else {
                    continue;
                };
                let Some(stage) = required_string_field(&span, TT_STAGE, strict, &mut warnings)?
                else {
                    continue;
                };
                let Some(success) = optional_success_field(&span, strict, &mut warnings)? else {
                    continue;
                };
                let latency_us = finished.saturating_sub(started).saturating_mul(1_000);
                stages.push(StageEvent {
                    request_id,
                    stage,
                    started_at_unix_ms: started,
                    finished_at_unix_ms: finished,
                    latency_us,
                    success,
                });
            }
            "queue" => {
                let Some(request_id) =
                    required_string_field(&span, TT_REQUEST_ID, strict, &mut warnings)?
                else {
                    continue;
                };
                let Some(queue) = required_string_field(&span, TT_QUEUE, strict, &mut warnings)?
                else {
                    continue;
                };
                let Some(depth_at_start) = optional_depth_field(&span, strict, &mut warnings)?
                else {
                    continue;
                };
                let wait_us = finished.saturating_sub(started).saturating_mul(1_000);
                queues.push(QueueEvent {
                    request_id,
                    queue,
                    waited_from_unix_ms: started,
                    waited_until_unix_ms: finished,
                    wait_us,
                    depth_at_start,
                });
            }
            other => {
                let message = format!(
                    "unknown `{TT_KIND}` value `{other}` on span `{}`",
                    span.name()
                );
                if strict {
                    return Err(ImportError::StrictViolation(message));
                }
                warnings.push(ImportWarning::new(message));
            }
        }
    }

    let started_at = min_start.unwrap_or_else(tailtriage_core::unix_time_ms);
    let finished_at = max_finish.unwrap_or(started_at);
    let run_id = explicit_run_id.map_or_else(
        || format!("tracing-import-{started_at}-{finished_at}"),
        |value| value,
    );

    let lifecycle_warnings = warnings
        .iter()
        .map(ImportWarning::message)
        .filter(|msg| msg.contains("skipped") || msg.contains("missing required"))
        .map(ToOwned::to_owned)
        .collect();

    let mut run = Run::new(RunMetadata {
        run_id,
        service_name,
        service_version,
        started_at_unix_ms: started_at,
        finished_at_unix_ms: finished_at,
        finalized_at_unix_ms: Some(finished_at),
        mode: CaptureMode::Light,
        effective_core_config: Some(EffectiveCoreConfig {
            mode: CaptureMode::Light,
            capture_limits: tailtriage_core::CaptureMode::Light.core_defaults(),
            strict_lifecycle: false,
        }),
        effective_tokio_sampler_config: None,
        host: None,
        pid: None,
        lifecycle_warnings,
        unfinished_requests: UnfinishedRequests::default(),
        run_end_reason: None,
    });
    run.schema_version = SCHEMA_VERSION;
    run.requests = requests;
    run.stages = stages;
    run.queues = queues;

    Ok(ImportedRun::new(run, warnings))
}

fn field_string<'a>(span: &'a SpanRecord, field: &'static str) -> Option<&'a str> {
    match span.fields().get(field) {
        Some(FieldValue::String(v)) => Some(v.as_str()),
        _ => None,
    }
}

fn required_string_field(
    span: &SpanRecord,
    field: &'static str,
    strict: bool,
    warnings: &mut Vec<ImportWarning>,
) -> Result<Option<String>, ImportError> {
    if let Some(value) = field_string(span, field) {
        return Ok(Some(value.to_owned()));
    }

    let message = format!("missing required `{field}` on span `{}`", span.name());
    if strict {
        return Err(ImportError::StrictViolation(message));
    }
    warnings.push(ImportWarning::new(message));
    Ok(None)
}

fn optional_success_field(
    span: &SpanRecord,
    strict: bool,
    warnings: &mut Vec<ImportWarning>,
) -> Result<Option<bool>, ImportError> {
    match span.fields().get(TT_SUCCESS) {
        None => Ok(Some(true)),
        Some(FieldValue::Bool(value)) => Ok(Some(*value)),
        Some(FieldValue::String(value)) => match value.as_str() {
            "true" => Ok(Some(true)),
            "false" => Ok(Some(false)),
            _ => invalid_field(
                span,
                TT_SUCCESS,
                "expected bool or \"true\"/\"false\" string",
                strict,
                warnings,
            ),
        },
        _ => invalid_field(
            span,
            TT_SUCCESS,
            "expected bool or \"true\"/\"false\" string",
            strict,
            warnings,
        ),
    }
}

#[allow(clippy::cast_sign_loss)]
#[allow(clippy::option_option)]
fn optional_depth_field(
    span: &SpanRecord,
    strict: bool,
    warnings: &mut Vec<ImportWarning>,
) -> Result<Option<Option<u64>>, ImportError> {
    match span.fields().get(TT_DEPTH_AT_START) {
        None => Ok(Some(None)),
        Some(FieldValue::U64(value)) => Ok(Some(Some(*value))),
        Some(FieldValue::I64(value)) if *value >= 0 => Ok(Some(Some(*value as u64))),
        _ => invalid_field(
            span,
            TT_DEPTH_AT_START,
            "must be a non-negative integer",
            strict,
            warnings,
        ),
    }
}

fn optional_string_field(span: &SpanRecord, field: &'static str) -> Option<String> {
    field_string(span, field).map(ToOwned::to_owned)
}

fn invalid_field<T>(
    span: &SpanRecord,
    field: &'static str,
    reason: &'static str,
    strict: bool,
    warnings: &mut Vec<ImportWarning>,
) -> Result<T, ImportError> {
    let message = format!("invalid `{field}` on span `{}`: {reason}", span.name());
    if strict {
        return Err(ImportError::StrictViolation(message));
    }
    warnings.push(ImportWarning::new(message));
    Err(ImportError::InvalidField {
        field,
        reason: reason.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request_span() -> SpanRecord {
        SpanRecord::new("request-span", 1_000, 1_020)
            .field(TT_KIND, "request")
            .field(TT_REQUEST_ID, "req-1")
            .field(TT_ROUTE, "/checkout")
    }

    #[test]
    fn request_only_conversion_creates_request_event() {
        let imported =
            run_from_span_records(vec![request_span()], ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.run().requests.len(), 1);
        assert!(imported.run().stages.is_empty());
        assert!(imported.run().queues.is_empty());
    }

    #[test]
    fn request_and_stage_creates_events() {
        let stage = SpanRecord::new("stage-span", 1_005, 1_010)
            .field(TT_KIND, "stage")
            .field(TT_REQUEST_ID, "req-1")
            .field(TT_STAGE, "db")
            .field(TT_SUCCESS, "false");
        let imported =
            run_from_span_records(vec![request_span(), stage], ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.run().requests.len(), 1);
        assert_eq!(imported.run().stages.len(), 1);
        assert!(!imported.run().stages[0].success);
    }

    #[test]
    fn request_and_queue_creates_events() {
        let queue = SpanRecord::new("queue-span", 1_001, 1_004)
            .field(TT_KIND, "queue")
            .field(TT_REQUEST_ID, "req-1")
            .field(TT_QUEUE, "db_pool")
            .field(TT_DEPTH_AT_START, 9_i64);
        let imported =
            run_from_span_records(vec![request_span(), queue], ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.run().requests.len(), 1);
        assert_eq!(imported.run().queues.len(), 1);
        assert_eq!(imported.run().queues[0].depth_at_start, Some(9));
    }

    #[test]
    fn missing_optional_fields_use_defaults() {
        let stage = SpanRecord::new("stage-span", 1_005, 1_010)
            .field(TT_KIND, "stage")
            .field(TT_REQUEST_ID, "req-1")
            .field(TT_STAGE, "db");
        let queue = SpanRecord::new("queue-span", 1_011, 1_012)
            .field(TT_KIND, "queue")
            .field(TT_REQUEST_ID, "req-1")
            .field(TT_QUEUE, "permits");
        let imported = run_from_span_records(
            vec![request_span(), stage, queue],
            ImportOptions::new("svc"),
        )
        .unwrap();
        assert_eq!(imported.run().requests[0].outcome, "ok");
        assert!(imported.run().stages[0].success);
        assert_eq!(imported.run().queues[0].depth_at_start, None);
    }

    #[test]
    fn missing_required_request_field_warns_and_skips_non_strict() {
        let bad = SpanRecord::new("bad", 1, 2)
            .field(TT_KIND, "request")
            .field(TT_REQUEST_ID, "req");
        let imported = run_from_span_records(vec![bad], ImportOptions::new("svc")).unwrap();
        assert!(imported.run().requests.is_empty());
        assert_eq!(imported.warnings().len(), 1);
    }

    #[test]
    fn missing_required_request_field_errors_in_strict_mode() {
        let bad = SpanRecord::new("bad", 1, 2)
            .field(TT_KIND, "request")
            .field(TT_REQUEST_ID, "req");
        assert!(run_from_span_records(vec![bad], ImportOptions::new("svc").strict(true)).is_err());
    }

    #[test]
    fn unknown_kind_warns_non_strict() {
        let unknown = SpanRecord::new("mystery", 1, 2)
            .field(TT_KIND, "blob")
            .field(TT_REQUEST_ID, "req");
        let imported = run_from_span_records(vec![unknown], ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.warnings().len(), 1);
    }

    #[test]
    fn span_without_kind_ignored_silently() {
        let plain = SpanRecord::new("plain", 1, 2).field(TT_REQUEST_ID, "req");
        let imported = run_from_span_records(vec![plain], ImportOptions::new("svc")).unwrap();
        assert!(imported.warnings().is_empty());
    }

    #[test]
    fn inverted_timestamps_warn_or_error() {
        let inverted = SpanRecord::new("bad-time", 10, 5)
            .field(TT_KIND, "request")
            .field(TT_REQUEST_ID, "req")
            .field(TT_ROUTE, "/r");
        let imported =
            run_from_span_records(vec![inverted.clone()], ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.warnings().len(), 1);
        assert!(
            run_from_span_records(vec![inverted], ImportOptions::new("svc").strict(true)).is_err()
        );
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
        let imported =
            run_from_span_records(vec![request_span()], ImportOptions::new("svc")).unwrap();
        assert!(imported.run().runtime_snapshots.is_empty());
        assert!(imported.run().inflight.is_empty());
    }
}
