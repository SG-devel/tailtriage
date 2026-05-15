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
    CaptureLimits, CaptureMode, EffectiveCoreConfig, QueueEvent, RequestEvent, Run, RunMetadata,
    StageEvent, UnfinishedRequests,
};

/// Converts in-memory [`SpanRecord`] values into a [`tailtriage_core::Run`].
///
/// # Errors
///
/// Returns [`ImportError`] when strict mode is enabled and a `tt.*` span is
/// malformed/incomplete, or when required field parsing fails.
#[allow(clippy::needless_pass_by_value)]
pub fn run_from_span_records<I>(
    spans: I,
    options: ImportOptions,
) -> Result<ImportedRun, ImportError>
where
    I: IntoIterator<Item = SpanRecord>,
{
    let mut run = Run::new(RunMetadata {
        run_id: String::new(),
        service_name: options.service_name().to_owned(),
        service_version: options.service_version_ref().map(str::to_owned),
        started_at_unix_ms: 0,
        finished_at_unix_ms: 0,
        finalized_at_unix_ms: None,
        mode: CaptureMode::Light,
        effective_core_config: Some(EffectiveCoreConfig {
            mode: CaptureMode::Light,
            capture_limits: CaptureLimits::default(),
            strict_lifecycle: false,
        }),
        effective_tokio_sampler_config: None,
        host: None,
        pid: None,
        lifecycle_warnings: Vec::new(),
        unfinished_requests: UnfinishedRequests::default(),
        run_end_reason: None,
    });
    let mut warnings = Vec::new();
    let mut started_at_min = None;
    let mut finished_at_max = None;

    for span in spans {
        ingest_span(
            &span,
            &options,
            &mut run,
            &mut warnings,
            &mut started_at_min,
            &mut finished_at_max,
        )?;
    }

    let started = started_at_min.unwrap_or_else(tailtriage_core::unix_time_ms);
    let finished = finished_at_max.unwrap_or(started);
    run.metadata.run_id = options.run_id_ref().map_or_else(
        || format!("tracing-import-{started}-{finished}"),
        str::to_owned,
    );
    run.metadata.started_at_unix_ms = started;
    run.metadata.finished_at_unix_ms = finished;
    run.metadata.finalized_at_unix_ms = Some(finished);
    run.metadata.lifecycle_warnings = warnings
        .iter()
        .map(ImportWarning::message)
        .filter(|m| m.contains("missing required") || m.contains("skipped"))
        .map(str::to_owned)
        .collect();
    debug_assert_eq!(run.schema_version, tailtriage_core::SCHEMA_VERSION);

    Ok(ImportedRun::new(run, warnings))
}
fn ingest_span(
    span: &SpanRecord,
    options: &ImportOptions,
    run: &mut Run,
    warnings: &mut Vec<ImportWarning>,
    started_at_min: &mut Option<u64>,
    finished_at_max: &mut Option<u64>,
) -> Result<(), ImportError> {
    /* omitted for brevity in patch? */
    let kind = match get_string_field(span.fields(), TT_KIND) {
        Ok(Some(value)) => value,
        Ok(None) => return Ok(()),
        Err(message) => {
            if options.strict_mode() {
                return Err(ImportError::InvalidField {
                    field: TT_KIND,
                    reason: message,
                });
            }
            warnings.push(ImportWarning::new(format!(
                "ignored span '{}' due to invalid `{TT_KIND}`: {message}",
                span.name()
            )));
            return Ok(());
        }
    };
    if span.finished_at_unix_ms() < span.started_at_unix_ms() {
        let message = format!(
            "span '{}' has inverted timestamps: started_at_unix_ms={} finished_at_unix_ms={}",
            span.name(),
            span.started_at_unix_ms(),
            span.finished_at_unix_ms()
        );
        if options.strict_mode() {
            return Err(ImportError::StrictViolation(message));
        }
        warnings.push(ImportWarning::new(message));
        return Ok(());
    }
    let latency_us = (span.finished_at_unix_ms() - span.started_at_unix_ms()) * 1_000;
    match kind {
        "request" => {
            let request_id = required_string(span, TT_REQUEST_ID, options, warnings)?;
            let route = required_string(span, TT_ROUTE, options, warnings)?;
            if let (Some(request_id), Some(route)) = (request_id, route) {
                let outcome = optional_string(span.fields(), TT_OUTCOME)?.unwrap_or("ok");
                run.requests.push(RequestEvent {
                    request_id: request_id.to_owned(),
                    route: route.to_owned(),
                    kind: None,
                    started_at_unix_ms: span.started_at_unix_ms(),
                    finished_at_unix_ms: span.finished_at_unix_ms(),
                    latency_us,
                    outcome: outcome.to_owned(),
                });
                update_bounds(span, started_at_min, finished_at_max);
            }
        }
        "stage" => {
            let request_id = required_string(span, TT_REQUEST_ID, options, warnings)?;
            let stage = required_string(span, TT_STAGE, options, warnings)?;
            if let (Some(request_id), Some(stage)) = (request_id, stage) {
                let success = parse_success(span.fields().get(TT_SUCCESS)).unwrap_or(true);
                run.stages.push(StageEvent {
                    request_id: request_id.to_owned(),
                    stage: stage.to_owned(),
                    started_at_unix_ms: span.started_at_unix_ms(),
                    finished_at_unix_ms: span.finished_at_unix_ms(),
                    latency_us,
                    success,
                });
                update_bounds(span, started_at_min, finished_at_max);
            }
        }
        "queue" => {
            let request_id = required_string(span, TT_REQUEST_ID, options, warnings)?;
            let queue = required_string(span, TT_QUEUE, options, warnings)?;
            if let (Some(request_id), Some(queue)) = (request_id, queue) {
                let depth_at_start =
                    parse_depth(span.fields().get(TT_DEPTH_AT_START)).unwrap_or(None);
                run.queues.push(QueueEvent {
                    request_id: request_id.to_owned(),
                    queue: queue.to_owned(),
                    waited_from_unix_ms: span.started_at_unix_ms(),
                    waited_until_unix_ms: span.finished_at_unix_ms(),
                    wait_us: latency_us,
                    depth_at_start,
                });
                update_bounds(span, started_at_min, finished_at_max);
            }
        }
        _ => {
            let message = format!(
                "unknown `{TT_KIND}` value '{kind}' on span '{}'",
                span.name()
            );
            if options.strict_mode() {
                return Err(ImportError::StrictViolation(message));
            }
            warnings.push(ImportWarning::new(message));
        }
    }
    Ok(())
}

fn update_bounds(
    span: &SpanRecord,
    started_at_min: &mut Option<u64>,
    finished_at_max: &mut Option<u64>,
) {
    *started_at_min = Some(started_at_min.map_or(span.started_at_unix_ms(), |m| {
        m.min(span.started_at_unix_ms())
    }));
    *finished_at_max = Some(finished_at_max.map_or(span.finished_at_unix_ms(), |m| {
        m.max(span.finished_at_unix_ms())
    }));
}

fn get_string_field<'a>(
    fields: &'a std::collections::BTreeMap<String, FieldValue>,
    key: &'static str,
) -> Result<Option<&'a str>, String> {
    match fields.get(key) {
        None => Ok(None),
        Some(FieldValue::String(value)) => Ok(Some(value.as_str())),
        Some(other) => Err(format!("expected string, got {other:?}")),
    }
}

fn optional_string<'a>(
    fields: &'a std::collections::BTreeMap<String, FieldValue>,
    key: &'static str,
) -> Result<Option<&'a str>, ImportError> {
    get_string_field(fields, key).map_err(|reason| ImportError::InvalidField { field: key, reason })
}

fn required_string<'a>(
    span: &'a SpanRecord,
    key: &'static str,
    options: &ImportOptions,
    warnings: &mut Vec<ImportWarning>,
) -> Result<Option<&'a str>, ImportError> {
    if let Some(value) = optional_string(span.fields(), key)? {
        Ok(Some(value))
    } else {
        let message = format!("missing required `{key}` on span '{}'", span.name());
        if options.strict_mode() {
            Err(ImportError::MissingField(key))
        } else {
            warnings.push(ImportWarning::new(message));
            Ok(None)
        }
    }
}

fn parse_depth(value: Option<&FieldValue>) -> Result<Option<u64>, String> {
    match value {
        None | Some(FieldValue::Null) => Ok(None),
        Some(FieldValue::U64(v)) => Ok(Some(*v)),
        Some(FieldValue::I64(v)) if *v >= 0 => Ok(Some(
            u64::try_from(*v).map_err(|_| "must be non-negative".to_owned())?,
        )),
        Some(FieldValue::I64(_)) => Err("must be non-negative".to_owned()),
        Some(FieldValue::F64(_)) => Err("floating-point values are not supported".to_owned()),
        Some(other) => Err(format!("expected integer, got {other:?}")),
    }
}

fn parse_success(value: Option<&FieldValue>) -> Result<bool, String> {
    match value {
        None | Some(FieldValue::Null) => Ok(true),
        Some(FieldValue::Bool(v)) => Ok(*v),
        Some(FieldValue::String(v)) if v == "true" => Ok(true),
        Some(FieldValue::String(v)) if v == "false" => Ok(false),
        Some(other) => Err(format!(
            "expected bool or \"true\"/\"false\", got {other:?}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_only_conversion_creates_one_request_event() {
        let span = SpanRecord::new("req", 10, 20)
            .field(TT_KIND, "request")
            .field(TT_REQUEST_ID, "r1")
            .field(TT_ROUTE, "/a");
        let out = run_from_span_records([span], ImportOptions::new("svc")).expect("ok");
        assert_eq!(out.run().requests.len(), 1);
    }

    #[test]
    fn request_stage_and_queue_paths_work() {
        let request = SpanRecord::new("req", 10, 20)
            .field(TT_KIND, "request")
            .field(TT_REQUEST_ID, "r1")
            .field(TT_ROUTE, "/a");
        let stage = SpanRecord::new("stage", 12, 15)
            .field(TT_KIND, "stage")
            .field(TT_REQUEST_ID, "r1")
            .field(TT_STAGE, "db");
        let queue = SpanRecord::new("queue", 9, 12)
            .field(TT_KIND, "queue")
            .field(TT_REQUEST_ID, "r1")
            .field(TT_QUEUE, "conn")
            .field(TT_DEPTH_AT_START, 2_u64);
        let out =
            run_from_span_records([request, stage, queue], ImportOptions::new("svc")).expect("ok");
        assert_eq!(out.run().requests.len(), 1);
        assert_eq!(out.run().stages.len(), 1);
        assert_eq!(out.run().queues.len(), 1);
    }

    #[test]
    fn missing_optional_fields_use_defaults() {
        let request = SpanRecord::new("req", 10, 20)
            .field(TT_KIND, "request")
            .field(TT_REQUEST_ID, "r1")
            .field(TT_ROUTE, "/a");
        let stage = SpanRecord::new("stage", 10, 12)
            .field(TT_KIND, "stage")
            .field(TT_REQUEST_ID, "r1")
            .field(TT_STAGE, "db");
        let queue = SpanRecord::new("queue", 9, 12)
            .field(TT_KIND, "queue")
            .field(TT_REQUEST_ID, "r1")
            .field(TT_QUEUE, "q1");

        let out =
            run_from_span_records([request, stage, queue], ImportOptions::new("svc")).expect("ok");
        assert_eq!(out.run().requests[0].outcome, "ok");
        assert!(out.run().stages[0].success);
        assert_eq!(out.run().queues[0].depth_at_start, None);
    }

    #[test]
    fn missing_required_request_field_warns_and_skips_when_non_strict() {
        let request = SpanRecord::new("req", 10, 20)
            .field(TT_KIND, "request")
            .field(TT_REQUEST_ID, "r1");
        let out = run_from_span_records([request], ImportOptions::new("svc")).expect("ok");
        assert!(out.run().requests.is_empty());
        assert_eq!(out.warnings().len(), 1);
    }

    #[test]
    fn missing_required_request_field_errors_in_strict_mode() {
        let request = SpanRecord::new("req", 10, 20)
            .field(TT_KIND, "request")
            .field(TT_REQUEST_ID, "r1");
        let err = run_from_span_records([request], ImportOptions::new("svc").strict(true))
            .expect_err("strict should fail");
        assert!(matches!(err, ImportError::MissingField(TT_ROUTE)));
    }

    #[test]
    fn unknown_kind_warns_non_strict() {
        let span = SpanRecord::new("x", 1, 2)
            .field(TT_KIND, "mystery")
            .field(TT_REQUEST_ID, "r1");
        let out = run_from_span_records([span], ImportOptions::new("svc")).expect("ok");
        assert_eq!(out.warnings().len(), 1);
    }

    #[test]
    fn span_without_kind_is_ignored_silently() {
        let span = SpanRecord::new("x", 1, 2).field(TT_REQUEST_ID, "r1");
        let out = run_from_span_records([span], ImportOptions::new("svc")).expect("ok");
        assert!(out.warnings().is_empty());
        assert!(out.run().requests.is_empty());
    }

    #[test]
    fn inverted_timestamps_warn_or_error() {
        let span = SpanRecord::new("x", 10, 9)
            .field(TT_KIND, "request")
            .field(TT_REQUEST_ID, "r1")
            .field(TT_ROUTE, "/a");
        let out = run_from_span_records([span.clone()], ImportOptions::new("svc")).expect("ok");
        assert_eq!(out.warnings().len(), 1);
        let err = run_from_span_records([span], ImportOptions::new("svc").strict(true))
            .expect_err("strict");
        assert!(matches!(err, ImportError::StrictViolation(_)));
    }

    #[test]
    fn empty_input_returns_valid_run_with_zero_events() {
        let out =
            run_from_span_records(Vec::<SpanRecord>::new(), ImportOptions::new("svc")).expect("ok");
        assert!(out.run().requests.is_empty());
        assert!(out.run().stages.is_empty());
        assert!(out.run().queues.is_empty());
    }

    #[test]
    fn runtime_snapshots_and_inflight_are_empty() {
        let out =
            run_from_span_records(Vec::<SpanRecord>::new(), ImportOptions::new("svc")).expect("ok");
        assert!(out.run().runtime_snapshots.is_empty());
        assert!(out.run().inflight.is_empty());
    }
}
