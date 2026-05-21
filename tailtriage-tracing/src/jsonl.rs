use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;

use serde_json::Value;

use crate::{
    run_from_span_records, FieldValue, ImportError, ImportOptions, ImportedRun, SpanRecord, TT_KIND,
};

/// Imports newline-delimited JSON records from a reader into a converted run.
///
/// This parser accepts a normalized `{"span": {...}}` shape and a tolerant
/// close-event shape when explicit `started_at_unix_ms` and `finished_at_unix_ms`
/// (or `start_unix_ms`/`end_unix_ms`) are present.
///
/// # Errors
///
/// Returns [`ImportError::Io`] for reader I/O failures,
/// [`ImportError::MalformedJsonLine`] for malformed non-empty JSONL lines,
/// and existing field/conversion errors for malformed tailtriage span records
/// or strict conversion violations surfaced by [`run_from_span_records`].
pub fn import_jsonl_reader<R: Read>(
    reader: R,
    options: ImportOptions,
) -> Result<ImportedRun, ImportError> {
    let mut spans = Vec::new();
    let mut parse_warnings = Vec::new();
    let reader = BufReader::new(reader);
    let strict = options.strict_mode();

    for (line_no, line_result) in reader.lines().enumerate() {
        let line = line_result.map_err(|err| ImportError::Io {
            operation: "read jsonl line",
            context: format!("line {}", line_no + 1),
            reason: err.to_string(),
        })?;

        if line.trim().is_empty() {
            continue;
        }

        let value: Value =
            serde_json::from_str(&line).map_err(|err| ImportError::MalformedJsonLine {
                line: line_no + 1,
                reason: err.to_string(),
            })?;

        if let Some(span) = parse_record(line_no + 1, &value, strict, &mut parse_warnings)? {
            spans.push(span);
        }
    }

    let imported = run_from_span_records(spans, options)?;
    let (mut run, mut conversion_warnings) = imported.into_parts();
    attach_parse_warnings_to_lifecycle(&mut run, &parse_warnings);
    parse_warnings.append(&mut conversion_warnings);
    Ok(ImportedRun::new(run, parse_warnings))
}

fn attach_parse_warnings_to_lifecycle(
    run: &mut tailtriage_core::Run,
    parse_warnings: &[crate::ImportWarning],
) {
    for warning in parse_warnings {
        let message = warning.message();
        if !run
            .metadata
            .lifecycle_warnings
            .iter()
            .any(|existing| existing == message)
        {
            run.metadata.lifecycle_warnings.push(message.to_owned());
        }
    }
}

/// Imports newline-delimited JSON records from a filesystem path.
///
/// # Errors
///
/// Returns [`ImportError::Io`] when path open or line reads fail,
/// [`ImportError::MalformedJsonLine`] for malformed non-empty JSONL lines,
/// and existing field/conversion errors for malformed tailtriage-tagged records
/// or strict conversion violations.
pub fn import_jsonl_path(
    path: impl AsRef<Path>,
    options: ImportOptions,
) -> Result<ImportedRun, ImportError> {
    let path_ref = path.as_ref();
    let file = std::fs::File::open(path_ref).map_err(|err| ImportError::Io {
        operation: "open jsonl path",
        context: path_ref.display().to_string(),
        reason: err.to_string(),
    })?;
    import_jsonl_reader(file, options)
}

fn parse_record(
    line_no: usize,
    value: &Value,
    strict: bool,
    warnings: &mut Vec<crate::ImportWarning>,
) -> Result<Option<SpanRecord>, ImportError> {
    if let Ok(span) = serde_json::from_value::<SpanRecord>(value.clone()) {
        if span.format_ref().is_none() || span.format_ref() == Some("tailtriage.tracing-span.v1") {
            return Ok(Some(span));
        }
        let message = format!("line {line_no}: unsupported span format marker");
        if strict {
            return Err(ImportError::StrictViolation(message));
        }
        warnings.push(crate::ImportWarning::new(message));
        return Ok(None);
    }
    if let Some(field_name) = first_non_scalar_tailtriage_field(value) {
        let message = format!(
            "line {line_no}: invalid field '{field_name}': expected scalar tt.* value in JSONL record"
        );
        if strict {
            return Err(ImportError::StrictViolation(message));
        }
        warnings.push(crate::ImportWarning::new(message));
        return Ok(None);
    }

    let has_tt = value_has_tailtriage_field(value);
    if let Some(span_obj) = value.get("span").and_then(Value::as_object) {
        let is_normalized_shape = span_obj.contains_key("name")
            && (span_obj.contains_key("started_at_unix_ms")
                || span_obj.contains_key("start_unix_ms"))
            && (span_obj.contains_key("finished_at_unix_ms")
                || span_obj.contains_key("end_unix_ms"));
        if is_normalized_shape {
            if !has_tt {
                return Ok(None);
            }
            return match parse_normalized_span(value, span_obj) {
                Ok(span) => Ok(Some(span)),
                Err(err) => {
                    let message = format!("line {line_no}: {err}");
                    if strict {
                        Err(ImportError::StrictViolation(message))
                    } else {
                        warnings.push(crate::ImportWarning::new(message));
                        Ok(None)
                    }
                }
            };
        }
        if has_tt && !indicates_close_event(value) {
            let message = format!(
                "line {line_no}: invalid field `span`: tailtriage span must include name, started_at_unix_ms/start_unix_ms, and finished_at_unix_ms/end_unix_ms"
            );
            if strict {
                return Err(ImportError::StrictViolation(message));
            }
            warnings.push(crate::ImportWarning::new(message));
            return Ok(None);
        }
    }

    match parse_close_event_shape(value) {
        Ok(Some(result)) => Ok(Some(result)),
        Ok(None) if has_tt => {
            let message = format!(
                "line {line_no}: tailtriage JSONL record must use normalized span shape or supported close-event shape with explicit timestamps"
            );
            if strict {
                Err(ImportError::StrictViolation(message))
            } else {
                warnings.push(crate::ImportWarning::new(message));
                Ok(None)
            }
        }
        Ok(None) => Ok(None),
        Err(err) if has_tt => {
            let message = format!("line {line_no}: {err}");
            if strict {
                Err(ImportError::StrictViolation(message))
            } else {
                warnings.push(crate::ImportWarning::new(message));
                Ok(None)
            }
        }
        Err(err) => Err(err),
    }
}

fn first_non_scalar_tailtriage_field(value: &Value) -> Option<String> {
    let is_non_scalar = |v: &Value| matches!(v, Value::Array(_) | Value::Object(_));
    let mut first: Option<String> = None;
    let mut consider = |key: &str, raw: &Value| {
        if key.starts_with("tt.") && is_non_scalar(raw) && first.is_none() {
            first = Some(key.to_owned());
        }
    };

    if let Some(map) = value.get("fields").and_then(Value::as_object) {
        for (k, v) in map {
            consider(k, v);
        }
    }
    if let Some(map) = value
        .get("span")
        .and_then(Value::as_object)
        .and_then(|span| span.get("fields"))
        .and_then(Value::as_object)
    {
        for (k, v) in map {
            consider(k, v);
        }
    }
    if let Some(map) = value.as_object() {
        for (k, v) in map {
            consider(k, v);
        }
    }

    first
}

fn value_has_tailtriage_field(value: &Value) -> bool {
    value
        .get("fields")
        .and_then(Value::as_object)
        .is_some_and(|fields| fields.keys().any(|k| k.starts_with("tt.")))
        || value
            .get("span")
            .and_then(Value::as_object)
            .and_then(|span| span.get("fields"))
            .and_then(Value::as_object)
            .is_some_and(|fields| fields.keys().any(|k| k.starts_with("tt.")))
        || value
            .as_object()
            .is_some_and(|obj| obj.keys().any(|k| k.starts_with("tt.")))
}

fn parse_normalized_span(
    value: &Value,
    span_obj: &serde_json::Map<String, Value>,
) -> Result<SpanRecord, ImportError> {
    let name = required_string(span_obj, "name")?;
    let id = optional_string(span_obj, "id")?;
    let parent_id = optional_string(span_obj, "parent_id")?;
    let started_at_unix_ms = required_timestamp(span_obj, "started_at_unix_ms", "start_unix_ms")?;
    let finished_at_unix_ms = required_timestamp(span_obj, "finished_at_unix_ms", "end_unix_ms")?;
    let duration_us = optional_duration_us(span_obj)?;

    let mut span = SpanRecord::new(name, started_at_unix_ms, finished_at_unix_ms);
    if let Some(id) = id {
        span = span.id(id);
    }
    if let Some(parent_id) = parent_id {
        span = span.parent_id(parent_id);
    }
    if let Some(duration_us) = duration_us {
        span = span.duration_us(duration_us);
    }

    let fields = extract_fields_for_span(value);
    for (k, v) in fields {
        span = span.field(k, v);
    }

    Ok(span)
}

fn parse_close_event_shape(value: &Value) -> Result<Option<SpanRecord>, ImportError> {
    let fields = extract_fields_for_span(value);
    if !fields.contains_key(TT_KIND) {
        return Ok(None);
    }

    if !indicates_close_event(value) {
        return Ok(None);
    }

    let obj = value.as_object().ok_or_else(|| ImportError::InvalidField {
        field: "jsonl",
        reason: "line must be a JSON object".to_owned(),
    })?;
    let name = optional_string(obj, "span_name")?
        .or_else(|| {
            obj.get("span")
                .and_then(Value::as_object)
                .and_then(|s| s.get("name"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
        .or_else(|| optional_string(obj, "name").ok().flatten())
        .unwrap_or_else(|| "tracing.close".to_owned());

    let span_obj = obj.get("span").and_then(Value::as_object);
    let started_at_unix_ms =
        required_timestamp_obj_or_nested(obj, span_obj, "started_at_unix_ms", "start_unix_ms")?;
    let finished_at_unix_ms =
        required_timestamp_obj_or_nested(obj, span_obj, "finished_at_unix_ms", "end_unix_ms")?;

    let mut span = SpanRecord::new(name, started_at_unix_ms, finished_at_unix_ms);
    if let Some(id) = optional_string(obj, "id")? {
        span = span.id(id);
    }
    if let Some(parent_id) = optional_string(obj, "parent_id")? {
        span = span.parent_id(parent_id);
    }
    for (k, v) in fields {
        span = span.field(k, v);
    }
    Ok(Some(span))
}

fn indicates_close_event(value: &Value) -> bool {
    let is_close = |s: &str| {
        let lower = s.to_ascii_lowercase();
        lower.contains("close") || lower.contains("closed")
    };
    value
        .get("event")
        .and_then(Value::as_str)
        .is_some_and(is_close)
        || value
            .get("message")
            .and_then(Value::as_str)
            .is_some_and(is_close)
        || value
            .get("fields")
            .and_then(Value::as_object)
            .and_then(|o| o.get("message"))
            .and_then(Value::as_str)
            .is_some_and(is_close)
}

fn extract_fields_for_span(value: &Value) -> BTreeMap<String, FieldValue> {
    let mut out = BTreeMap::new();
    // Precedence for duplicate keys is: outer fields < span.fields < top-level tt.*.
    collect_fields_object(value.get("fields"), &mut out);
    collect_fields_object(value.get("span").and_then(|s| s.get("fields")), &mut out);
    collect_tt_top_level(value, &mut out);
    out
}

fn collect_fields_object(value: Option<&Value>, out: &mut BTreeMap<String, FieldValue>) {
    let Some(map) = value.and_then(Value::as_object) else {
        return;
    };
    for (k, v) in map {
        if let Some(scalar) = to_field_value(v) {
            out.insert(k.clone(), scalar);
        }
    }
}
fn collect_tt_top_level(value: &Value, out: &mut BTreeMap<String, FieldValue>) {
    let Some(map) = value.as_object() else {
        return;
    };
    for (k, v) in map {
        if k.starts_with("tt.") {
            if let Some(scalar) = to_field_value(v) {
                out.insert(k.clone(), scalar);
            }
        }
    }
}

fn to_field_value(value: &Value) -> Option<FieldValue> {
    match value {
        Value::String(s) => Some(FieldValue::String(s.clone())),
        Value::Bool(b) => Some(FieldValue::Bool(*b)),
        Value::Number(n) => {
            if let Some(u) = n.as_u64() {
                Some(FieldValue::U64(u))
            } else if let Some(i) = n.as_i64() {
                Some(FieldValue::I64(i))
            } else {
                n.as_f64().map(FieldValue::F64)
            }
        }
        Value::Null => Some(FieldValue::Null),
        _ => None,
    }
}

fn required_string(
    obj: &serde_json::Map<String, Value>,
    key: &'static str,
) -> Result<String, ImportError> {
    optional_string(obj, key)?.ok_or(ImportError::MissingField(key))
}
fn optional_string(
    obj: &serde_json::Map<String, Value>,
    key: &'static str,
) -> Result<Option<String>, ImportError> {
    match obj.get(key) {
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(ImportError::InvalidField {
            field: key,
            reason: "expected string".to_owned(),
        }),
        None => Ok(None),
    }
}
fn required_timestamp(
    obj: &serde_json::Map<String, Value>,
    primary: &'static str,
    alias: &'static str,
) -> Result<u64, ImportError> {
    required_timestamp_obj(obj, primary, alias)
}
fn required_timestamp_obj(
    obj: &serde_json::Map<String, Value>,
    primary: &'static str,
    alias: &'static str,
) -> Result<u64, ImportError> {
    if let Some(v) = obj.get(primary).or_else(|| obj.get(alias)) {
        return parse_u64(v, primary);
    }
    Err(ImportError::MissingField(primary))
}
fn required_timestamp_obj_or_nested(
    obj: &serde_json::Map<String, Value>,
    nested_obj: Option<&serde_json::Map<String, Value>>,
    primary: &'static str,
    alias: &'static str,
) -> Result<u64, ImportError> {
    if let Some(v) = obj.get(primary).or_else(|| obj.get(alias)) {
        return parse_u64(v, primary);
    }
    if let Some(v) = nested_obj.and_then(|nested| nested.get(primary).or_else(|| nested.get(alias)))
    {
        return parse_u64(v, primary);
    }
    Err(ImportError::MissingField(primary))
}
fn optional_duration_us(obj: &serde_json::Map<String, Value>) -> Result<Option<u64>, ImportError> {
    match obj.get("duration_us") {
        Some(Value::Number(n)) => n
            .as_u64()
            .ok_or_else(|| ImportError::InvalidField {
                field: "duration_us",
                reason: "expected unsigned integer microseconds as u64".to_owned(),
            })
            .map(Some),
        Some(_) => Err(ImportError::InvalidField {
            field: "duration_us",
            reason: "expected unsigned integer microseconds as u64".to_owned(),
        }),
        None => Ok(None),
    }
}

fn parse_u64(v: &Value, field: &'static str) -> Result<u64, ImportError> {
    v.as_u64().ok_or_else(|| ImportError::InvalidField {
        field,
        reason: "expected unix timestamp in milliseconds as u64".to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ImportOptions;
    use std::io::Cursor;

    #[test]
    fn normalized_jsonl_request_only() {
        let input = r#"{"span":{"name":"req","started_at_unix_ms":1,"finished_at_unix_ms":2,"fields":{"tt.kind":"request","tt.request_id":"r1","tt.route":"/a"}}}"#;
        let imported = import_jsonl_reader(Cursor::new(input), ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.run().requests.len(), 1);
    }

    #[test]
    fn normalized_jsonl_request_stage_queue() {
        let input = r#"
{"span":{"name":"req","started_at_unix_ms":10,"finished_at_unix_ms":20,"fields":{"tt.kind":"request","tt.request_id":"r1","tt.route":"/a"}}}
{"span":{"name":"st","started_at_unix_ms":11,"finished_at_unix_ms":18,"fields":{"tt.kind":"stage","tt.request_id":"r1","tt.stage":"db"}}}
{"span":{"name":"q","started_at_unix_ms":10,"finished_at_unix_ms":11,"fields":{"tt.kind":"queue","tt.request_id":"r1","tt.queue":"permits","tt.depth_at_start":3}}}
"#;
        let imported = import_jsonl_reader(Cursor::new(input), ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.run().requests.len(), 1);
        assert_eq!(imported.run().stages.len(), 1);
        assert_eq!(imported.run().queues.len(), 1);
        assert_eq!(imported.run().queues[0].queue, "permits");
    }

    #[test]
    fn unrelated_line_ignored() {
        let input = r#"{"message":"ordinary log","level":"info"}"#;
        let imported = import_jsonl_reader(Cursor::new(input), ImportOptions::new("svc")).unwrap();
        assert!(imported.run().requests.is_empty());
    }

    #[test]
    fn empty_lines_ignored() {
        let input = "\n\n";
        let imported = import_jsonl_reader(Cursor::new(input), ImportOptions::new("svc")).unwrap();
        assert!(imported.run().requests.is_empty());
    }

    #[test]
    fn malformed_json_returns_malformed_json_line_error() {
        let err =
            import_jsonl_reader(Cursor::new("{not-json}"), ImportOptions::new("svc")).unwrap_err();
        assert!(matches!(
            err,
            ImportError::MalformedJsonLine { line: 1, .. }
        ));
    }

    #[test]
    fn malformed_json_reports_correct_line_number() {
        let input = "{\"message\":\"ok\"}\n{not-json}";
        let err = import_jsonl_reader(Cursor::new(input), ImportOptions::new("svc")).unwrap_err();
        assert!(matches!(
            err,
            ImportError::MalformedJsonLine { line: 2, .. }
        ));
    }

    #[test]
    fn missing_required_fields_surface_conversion_warnings() {
        let input = r#"{"span":{"name":"req","started_at_unix_ms":1,"finished_at_unix_ms":2,"fields":{"tt.kind":"request","tt.request_id":"r1"}}}"#;
        let imported = import_jsonl_reader(Cursor::new(input), ImportOptions::new("svc")).unwrap();
        assert!(imported.run().requests.is_empty());
        assert_eq!(imported.warnings().len(), 1);
    }

    #[test]
    fn strict_mode_propagates_conversion_errors() {
        let input = r#"{"span":{"name":"req","started_at_unix_ms":1,"finished_at_unix_ms":2,"fields":{"tt.kind":"request","tt.request_id":"r1"}}}"#;
        let err = import_jsonl_reader(Cursor::new(input), ImportOptions::new("svc").strict(true))
            .unwrap_err();
        assert!(matches!(err, ImportError::StrictViolation(_)));
    }

    #[test]
    fn path_import_works() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("input.jsonl");
        std::fs::write(&path, r#"{"span":{"name":"req","started_at_unix_ms":1,"finished_at_unix_ms":2,"fields":{"tt.kind":"request","tt.request_id":"r1","tt.route":"/a"}}}"#).unwrap();
        let imported = import_jsonl_path(&path, ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.run().requests.len(), 1);
    }

    #[test]
    fn path_open_failure_returns_io_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing.jsonl");
        let err = import_jsonl_path(&path, ImportOptions::new("svc")).unwrap_err();
        match err {
            ImportError::Io {
                operation, context, ..
            } => {
                assert_eq!(operation, "open jsonl path");
                assert!(context.contains("missing.jsonl"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    struct BoomReader;

    impl Read for BoomReader {
        fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("boom"))
        }
    }

    #[test]
    fn reader_error_returns_io_error() {
        let err = import_jsonl_reader(BoomReader, ImportOptions::new("svc")).unwrap_err();
        assert!(matches!(
            err,
            ImportError::Io {
                operation: "read jsonl line",
                ..
            }
        ));
    }

    #[test]
    fn close_event_shape_with_explicit_timestamps_is_supported() {
        let input = r#"{"span":{"name":"req","started_at_unix_ms":1,"finished_at_unix_ms":10,"fields":{"tt.kind":"request","tt.request_id":"r1","tt.route":"/a"}}}
{"event":"close","span":{"name":"st","fields":{"tt.kind":"stage","tt.request_id":"r1","tt.stage":"db"}},"started_at_unix_ms":5,"finished_at_unix_ms":8}"#;
        let imported = import_jsonl_reader(Cursor::new(input), ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.run().stages.len(), 1);
        assert_eq!(imported.run().stages[0].stage, "db");
    }

    #[test]
    fn incomplete_normalized_tt_kind_span_warns_non_strict_and_errors_strict() {
        let input = r#"{"span":{"name":"req","fields":{"tt.kind":"request","tt.request_id":"r1","tt.route":"/a"}}}"#;
        let imported = import_jsonl_reader(Cursor::new(input), ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.run().requests.len(), 0);
        assert_eq!(imported.warnings().len(), 1);
        assert!(imported.warnings()[0].message().contains("line 1"));

        let err = import_jsonl_reader(Cursor::new(input), ImportOptions::new("svc").strict(true))
            .unwrap_err();
        assert!(matches!(err, ImportError::StrictViolation(_)));
    }

    #[test]
    fn close_event_shape_with_nested_span_timestamps_is_supported() {
        let input = r#"{"span":{"name":"req","started_at_unix_ms":1,"finished_at_unix_ms":10,"fields":{"tt.kind":"request","tt.request_id":"r1","tt.route":"/a"}}}
{"event":"close","span":{"name":"st","started_at_unix_ms":5,"finished_at_unix_ms":8,"fields":{"tt.kind":"stage","tt.request_id":"r1","tt.stage":"db"}}}"#;
        let imported = import_jsonl_reader(Cursor::new(input), ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.run().stages.len(), 1);
        assert_eq!(imported.run().stages[0].stage, "db");
    }

    #[test]
    fn non_strict_normalized_tt_span_invalid_timestamp_warns_and_skips() {
        let input = r#"{"span":{"name":"req","started_at_unix_ms":"bad","finished_at_unix_ms":2,"fields":{"tt.kind":"request","tt.request_id":"r1","tt.route":"/a"}}}"#;
        let imported = import_jsonl_reader(Cursor::new(input), ImportOptions::new("svc")).unwrap();
        assert!(imported.run().requests.is_empty());
        assert_eq!(imported.warnings().len(), 1);
        assert!(imported.warnings()[0].message().contains("line 1"));
    }

    #[test]
    fn strict_normalized_tt_span_invalid_timestamp_errors() {
        let input = r#"{"span":{"name":"req","started_at_unix_ms":"bad","finished_at_unix_ms":2,"fields":{"tt.kind":"request","tt.request_id":"r1","tt.route":"/a"}}}"#;
        let err = import_jsonl_reader(Cursor::new(input), ImportOptions::new("svc").strict(true))
            .unwrap_err();
        assert!(matches!(err, ImportError::StrictViolation(_)));
    }

    #[test]
    fn malformed_unrelated_normalized_spans_are_ignored() {
        let input = r#"
{"span":{"name":"other","id":123,"started_at_unix_ms":1,"finished_at_unix_ms":2}}
{"span":{"name":"other","parent_id":{},"started_at_unix_ms":1,"finished_at_unix_ms":2}}
{"span":{"name":123,"started_at_unix_ms":1,"finished_at_unix_ms":2}}
"#;
        let imported = import_jsonl_reader(Cursor::new(input), ImportOptions::new("svc")).unwrap();
        assert!(imported.run().requests.is_empty());
        assert!(imported.warnings().is_empty());
    }

    #[test]
    fn parse_warnings_are_persisted_to_run_lifecycle_warnings() {
        let input = r#"
{"span":{"name":"req","started_at_unix_ms":1,"finished_at_unix_ms":2,"fields":{"tt.kind":"request","tt.request_id":"r1","tt.route":"/ok","tt.outcome":"ok"}}}
{"span":{"name":"broken","fields":{"tt.kind":"request","tt.request_id":"r2","tt.route":"/broken"}}}
"#;
        let imported = import_jsonl_reader(Cursor::new(input), ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.run().requests.len(), 1);
        assert_eq!(imported.warnings().len(), 1);
        let warning_message = imported.warnings()[0].message();
        assert!(warning_message.contains("line 3"));
        assert!(imported
            .run()
            .metadata
            .lifecycle_warnings
            .iter()
            .any(|warning| warning == warning_message));
    }

    #[test]
    fn conversion_warnings_still_follow_existing_lifecycle_policy() {
        let input = r#"
{"span":{"name":"req","started_at_unix_ms":1,"finished_at_unix_ms":2,"fields":{"tt.kind":"request","tt.request_id":"r1","tt.route":"/ok","tt.outcome":"ok"}}}
{"span":{"name":"req2","started_at_unix_ms":3,"finished_at_unix_ms":4,"fields":{"tt.kind":"request","tt.request_id":"r2"}}}
"#;
        let imported = import_jsonl_reader(Cursor::new(input), ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.run().requests.len(), 1);
        assert_eq!(imported.warnings().len(), 1);
        let conversion_warning = imported.warnings()[0].message();
        assert!(conversion_warning.contains("tt.route"));
        assert!(imported
            .run()
            .metadata
            .lifecycle_warnings
            .iter()
            .any(|warning| warning == conversion_warning));
    }

    #[test]
    fn unknown_kind_warning_is_durable_once_with_valid_request_present() {
        let input = r#"
{"span":{"name":"req","started_at_unix_ms":1,"finished_at_unix_ms":2,"fields":{"tt.kind":"request","tt.request_id":"r1","tt.route":"/ok","tt.outcome":"ok"}}}
{"span":{"name":"unknown","started_at_unix_ms":3,"finished_at_unix_ms":4,"fields":{"tt.kind":"mystery"}}}
"#;
        let imported = import_jsonl_reader(Cursor::new(input), ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.run().requests.len(), 1);
        assert_eq!(imported.warnings().len(), 1);
        let warning_message = "unknown tt.kind 'mystery' in span 'unknown'";
        assert_eq!(imported.warnings()[0].message(), warning_message);
        let matches = imported
            .run()
            .metadata
            .lifecycle_warnings
            .iter()
            .filter(|warning| warning.as_str() == warning_message)
            .count();
        assert_eq!(matches, 1);
    }

    #[test]
    fn tt_fields_without_kind_warn_non_strict_and_error_strict() {
        let input = r#"{"span":{"name":"req","started_at_unix_ms":1,"finished_at_unix_ms":2,"fields":{"tt.request_id":"r1","tt.route":"/ok"}}}"#;
        let imported = import_jsonl_reader(Cursor::new(input), ImportOptions::new("svc")).unwrap();
        assert!(imported.run().requests.is_empty());
        assert!(imported.warnings().iter().any(|w| w
            .message()
            .contains("missing required field 'tt.kind' in span 'req'")));

        let err = import_jsonl_reader(Cursor::new(input), ImportOptions::new("svc").strict(true))
            .unwrap_err();
        assert!(matches!(err, ImportError::StrictViolation(_)));
    }

    #[test]
    fn non_scalar_span_fields_tt_kind_warns_non_strict_and_errors_strict() {
        let input = r#"{"span":{"name":"bad","started_at_unix_ms":1,"finished_at_unix_ms":2,"fields":{"tt.kind":{"bad":true}}}}"#;
        let imported = import_jsonl_reader(Cursor::new(input), ImportOptions::new("svc")).unwrap();
        assert!(imported.run().requests.is_empty());
        assert!(imported
            .warnings()
            .iter()
            .any(|w| w.message().contains("line 1") && w.message().contains("tt.kind")));
        let err = import_jsonl_reader(Cursor::new(input), ImportOptions::new("svc").strict(true))
            .unwrap_err();
        assert!(matches!(err, ImportError::StrictViolation(_)));
    }

    #[test]
    fn non_scalar_outer_fields_tt_kind_warns_non_strict_and_errors_strict() {
        let input = r#"{"span":{"name":"bad","started_at_unix_ms":1,"finished_at_unix_ms":2},"fields":{"tt.kind":{"bad":true}}}"#;
        let imported = import_jsonl_reader(Cursor::new(input), ImportOptions::new("svc")).unwrap();
        assert!(imported.run().requests.is_empty());
        assert!(imported
            .warnings()
            .iter()
            .any(|w| w.message().contains("line 1") && w.message().contains("tt.kind")));
        let err = import_jsonl_reader(Cursor::new(input), ImportOptions::new("svc").strict(true))
            .unwrap_err();
        assert!(matches!(err, ImportError::StrictViolation(_)));
    }

    #[test]
    fn non_scalar_top_level_tt_kind_warns_non_strict_and_errors_strict() {
        let input = r#"{"span":{"name":"bad","started_at_unix_ms":1,"finished_at_unix_ms":2},"tt.kind":{"bad":true}}"#;
        let imported = import_jsonl_reader(Cursor::new(input), ImportOptions::new("svc")).unwrap();
        assert!(imported.run().requests.is_empty());
        assert!(imported
            .warnings()
            .iter()
            .any(|w| w.message().contains("line 1") && w.message().contains("tt.kind")));
        let err = import_jsonl_reader(Cursor::new(input), ImportOptions::new("svc").strict(true))
            .unwrap_err();
        assert!(matches!(err, ImportError::StrictViolation(_)));
    }

    #[test]
    fn missing_optional_defaults_emit_aggregate_warnings_once() {
        let input = r#"
{"span":{"name":"req1","started_at_unix_ms":1,"finished_at_unix_ms":2,"fields":{"tt.kind":"request","tt.request_id":"r1","tt.route":"/ok"}}}
{"span":{"name":"req2","started_at_unix_ms":3,"finished_at_unix_ms":4,"fields":{"tt.kind":"request","tt.request_id":"r2","tt.route":"/ok2"}}}
{"span":{"name":"st1","started_at_unix_ms":1,"finished_at_unix_ms":2,"fields":{"tt.kind":"stage","tt.request_id":"r1","tt.stage":"db"}}}
{"span":{"name":"st2","started_at_unix_ms":3,"finished_at_unix_ms":4,"fields":{"tt.kind":"stage","tt.request_id":"r2","tt.stage":"cache"}}}
"#;
        let imported = import_jsonl_reader(Cursor::new(input), ImportOptions::new("svc")).unwrap();
        let msgs = imported
            .warnings()
            .iter()
            .map(|w| w.message().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(
            msgs.iter()
                .filter(|m| m.contains("missing optional 'tt.outcome'"))
                .count(),
            1
        );
        assert_eq!(
            msgs.iter()
                .filter(|m| m.contains("missing optional 'tt.success'"))
                .count(),
            1
        );
    }

    #[test]
    fn unrelated_malformed_normalized_span_does_not_create_lifecycle_warning() {
        let input = r#"
{"span":{"name":"req","started_at_unix_ms":1,"finished_at_unix_ms":2,"fields":{"tt.kind":"request","tt.request_id":"r1","tt.route":"/ok","tt.outcome":"ok"}}}
{"span":{"name":"other","id":123,"started_at_unix_ms":3,"finished_at_unix_ms":4}}
"#;
        let imported = import_jsonl_reader(Cursor::new(input), ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.run().requests.len(), 1);
        assert!(imported.warnings().is_empty());
        assert!(imported.run().metadata.lifecycle_warnings.is_empty());
    }

    #[test]
    fn strict_parse_warning_case_still_errors_without_run() {
        let input = r#"{"span":{"name":"req","fields":{"tt.kind":"request","tt.request_id":"r1","tt.route":"/a"}}}"#;
        let err = import_jsonl_reader(Cursor::new(input), ImportOptions::new("svc").strict(true))
            .unwrap_err();
        assert!(matches!(err, ImportError::StrictViolation(_)));
    }

    #[test]
    fn malformed_tt_normalized_spans_warn_non_strict_and_error_strict() {
        let malformed = [
            r#"{"span":{"name":"req","id":123,"started_at_unix_ms":1,"finished_at_unix_ms":2,"fields":{"tt.kind":"request"}}}"#,
            r#"{"span":{"name":"req","parent_id":{},"started_at_unix_ms":1,"finished_at_unix_ms":2,"fields":{"tt.kind":"request"}}}"#,
            r#"{"span":{"name":123,"started_at_unix_ms":1,"finished_at_unix_ms":2,"fields":{"tt.kind":"request"}}}"#,
        ];
        for line in malformed {
            let imported =
                import_jsonl_reader(Cursor::new(line), ImportOptions::new("svc")).unwrap();
            assert!(imported.run().requests.is_empty());
            assert_eq!(imported.warnings().len(), 1);
            let err =
                import_jsonl_reader(Cursor::new(line), ImportOptions::new("svc").strict(true))
                    .unwrap_err();
            assert!(matches!(err, ImportError::StrictViolation(_)));
        }
    }

    #[test]
    fn normalized_duration_us_overrides_derived_latency() {
        let input = r#"
{"span":{"name":"req","started_at_unix_ms":10,"finished_at_unix_ms":20,"duration_us":1234,"fields":{"tt.kind":"request","tt.request_id":"r1","tt.route":"/a"}}}
{"span":{"name":"st","started_at_unix_ms":11,"finished_at_unix_ms":18,"duration_us":1234,"fields":{"tt.kind":"stage","tt.request_id":"r1","tt.stage":"db"}}}
{"span":{"name":"q","started_at_unix_ms":10,"finished_at_unix_ms":11,"duration_us":1234,"fields":{"tt.kind":"queue","tt.request_id":"r1","tt.queue":"permits","tt.depth_at_start":3}}}
"#;
        let imported = import_jsonl_reader(Cursor::new(input), ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.run().requests[0].latency_us, 1234);
        assert_eq!(imported.run().stages[0].latency_us, 1234);
        assert_eq!(imported.run().queues[0].wait_us, 1234);
    }

    #[test]
    fn normalized_zero_duration_us_is_accepted_for_positive_timestamp_spans() {
        let input = r#"
{"span":{"name":"req","started_at_unix_ms":10,"finished_at_unix_ms":20,"duration_us":0,"fields":{"tt.kind":"request","tt.request_id":"r1","tt.route":"/a"}}}
{"span":{"name":"st","started_at_unix_ms":11,"finished_at_unix_ms":18,"duration_us":0,"fields":{"tt.kind":"stage","tt.request_id":"r1","tt.stage":"db"}}}
{"span":{"name":"q","started_at_unix_ms":10,"finished_at_unix_ms":11,"duration_us":0,"fields":{"tt.kind":"queue","tt.request_id":"r1","tt.queue":"permits","tt.depth_at_start":3}}}
"#;
        let imported = import_jsonl_reader(Cursor::new(input), ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.run().requests[0].latency_us, 0);
        assert_eq!(imported.run().stages[0].latency_us, 0);
        assert_eq!(imported.run().queues[0].wait_us, 0);
    }

    #[test]
    fn normalized_request_fields_import_from_outer_fields() {
        let input = r#"{"span":{"name":"req","started_at_unix_ms":1,"finished_at_unix_ms":2},"fields":{"tt.kind":"request","tt.request_id":"r1","tt.route":"/outer"}}"#;
        let imported = import_jsonl_reader(Cursor::new(input), ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.run().requests.len(), 1);
        assert_eq!(imported.run().requests[0].route, "/outer");
    }

    #[test]
    fn normalized_request_fields_import_from_top_level_tt_keys() {
        let input = r#"{"span":{"name":"req","started_at_unix_ms":1,"finished_at_unix_ms":2},"tt.kind":"request","tt.request_id":"r1","tt.route":"/top"}"#;
        let imported = import_jsonl_reader(Cursor::new(input), ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.run().requests.len(), 1);
        assert_eq!(imported.run().requests[0].route, "/top");
    }

    #[test]
    fn normalized_stage_fields_import_from_outer_fields() {
        let input = r#"{"span":{"name":"req","started_at_unix_ms":1,"finished_at_unix_ms":2},"fields":{"tt.kind":"request","tt.request_id":"r1","tt.route":"/a"}}
{"span":{"name":"st","started_at_unix_ms":1,"finished_at_unix_ms":2},"fields":{"tt.kind":"stage","tt.request_id":"r1","tt.stage":"db"}}"#;
        let imported = import_jsonl_reader(Cursor::new(input), ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.run().stages.len(), 1);
        assert_eq!(imported.run().stages[0].stage, "db");
    }

    #[test]
    fn normalized_stage_fields_import_from_top_level_tt_keys() {
        let input = r#"{"span":{"name":"req","started_at_unix_ms":1,"finished_at_unix_ms":2},"fields":{"tt.kind":"request","tt.request_id":"r1","tt.route":"/a"}}
{"span":{"name":"st","started_at_unix_ms":1,"finished_at_unix_ms":2},"tt.kind":"stage","tt.request_id":"r1","tt.stage":"cache"}"#;
        let imported = import_jsonl_reader(Cursor::new(input), ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.run().stages.len(), 1);
        assert_eq!(imported.run().stages[0].stage, "cache");
    }

    #[test]
    fn normalized_queue_fields_import_from_outer_fields() {
        let input = r#"{"span":{"name":"req","started_at_unix_ms":1,"finished_at_unix_ms":2},"fields":{"tt.kind":"request","tt.request_id":"r1","tt.route":"/a"}}
{"span":{"name":"q","started_at_unix_ms":1,"finished_at_unix_ms":2},"fields":{"tt.kind":"queue","tt.request_id":"r1","tt.queue":"permits","tt.depth_at_start":3}}"#;
        let imported = import_jsonl_reader(Cursor::new(input), ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.run().queues.len(), 1);
        assert_eq!(imported.run().queues[0].depth_at_start, Some(3));
    }

    #[test]
    fn normalized_queue_fields_import_from_top_level_tt_keys() {
        let input = r#"{"span":{"name":"req","started_at_unix_ms":1,"finished_at_unix_ms":2},"fields":{"tt.kind":"request","tt.request_id":"r1","tt.route":"/a"}}
{"span":{"name":"q","started_at_unix_ms":1,"finished_at_unix_ms":2},"tt.kind":"queue","tt.request_id":"r1","tt.queue":"permits","tt.depth_at_start":7}"#;
        let imported = import_jsonl_reader(Cursor::new(input), ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.run().queues.len(), 1);
        assert_eq!(imported.run().queues[0].depth_at_start, Some(7));
    }

    #[test]
    fn normalized_span_fields_still_imports() {
        let input = r#"{"span":{"name":"req","started_at_unix_ms":1,"finished_at_unix_ms":2,"fields":{"tt.kind":"request","tt.request_id":"r1","tt.route":"/span"}}}"#;
        let imported = import_jsonl_reader(Cursor::new(input), ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.run().requests.len(), 1);
        assert_eq!(imported.run().requests[0].route, "/span");
    }

    #[test]
    fn normalized_field_precedence_is_outer_then_span_then_top_level() {
        let input = r#"{"span":{"name":"req","started_at_unix_ms":1,"finished_at_unix_ms":2,"fields":{"tt.route":"/span","tt.kind":"request","tt.request_id":"r1"}},"fields":{"tt.route":"/outer"},"tt.route":"/top"}"#;
        let imported = import_jsonl_reader(Cursor::new(input), ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.run().requests[0].route, "/top");
    }

    #[test]
    fn normalized_duration_us_from_outer_fields_still_overrides_derived_latency() {
        let input = r#"{"span":{"name":"req","started_at_unix_ms":10,"finished_at_unix_ms":20,"duration_us":1234},"fields":{"tt.kind":"request","tt.request_id":"r1","tt.route":"/outer"}}"#;
        let imported = import_jsonl_reader(Cursor::new(input), ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.run().requests[0].latency_us, 1234);
    }

    #[test]
    fn normalized_duration_us_from_top_level_tt_keys_still_overrides_derived_latency() {
        let input = r#"{"span":{"name":"req","started_at_unix_ms":10,"finished_at_unix_ms":20,"duration_us":1234},"tt.kind":"request","tt.request_id":"r1","tt.route":"/top"}"#;
        let imported = import_jsonl_reader(Cursor::new(input), ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.run().requests[0].latency_us, 1234);
    }

    #[test]
    fn invalid_duration_us_on_tt_span_warns_or_errors() {
        let input = r#"{"span":{"name":"req","started_at_unix_ms":1,"finished_at_unix_ms":2,"duration_us":"bad","fields":{"tt.kind":"request","tt.request_id":"r1","tt.route":"/a"}}}"#;
        let imported = import_jsonl_reader(Cursor::new(input), ImportOptions::new("svc")).unwrap();
        assert!(imported.run().requests.is_empty());
        assert_eq!(imported.warnings().len(), 1);
        let err = import_jsonl_reader(Cursor::new(input), ImportOptions::new("svc").strict(true))
            .unwrap_err();
        assert!(matches!(err, ImportError::StrictViolation(_)));
    }

    #[test]
    fn invalid_duration_us_on_unrelated_normalized_span_is_ignored() {
        let input = r#"{"span":{"name":"other","started_at_unix_ms":1,"finished_at_unix_ms":2,"duration_us":"bad"}}"#;
        let imported = import_jsonl_reader(Cursor::new(input), ImportOptions::new("svc")).unwrap();
        assert!(imported.run().requests.is_empty());
        assert!(imported.warnings().is_empty());
    }

    #[test]
    fn non_strict_close_event_like_tt_span_missing_timestamps_warns_and_skips() {
        let input = r#"{"event":"close","span":{"name":"st","fields":{"tt.kind":"stage","tt.request_id":"r1","tt.stage":"db"}}}"#;
        let imported = import_jsonl_reader(Cursor::new(input), ImportOptions::new("svc")).unwrap();
        assert!(imported.run().stages.is_empty());
        assert_eq!(imported.warnings().len(), 1);
        assert!(imported.warnings()[0].message().contains("line 1"));
    }

    #[test]
    fn strict_close_event_like_tt_span_missing_timestamps_errors() {
        let input = r#"{"event":"close","span":{"name":"st","fields":{"tt.kind":"stage","tt.request_id":"r1","tt.stage":"db"}}}"#;
        let err = import_jsonl_reader(Cursor::new(input), ImportOptions::new("svc").strict(true))
            .unwrap_err();
        assert!(matches!(err, ImportError::StrictViolation(_)));
    }

    #[test]
    fn top_level_tt_record_without_span_shape_warns_non_strict() {
        let input = r#"{"tt.kind":"request","tt.request_id":"r1","tt.route":"/checkout"}"#;
        let imported = import_jsonl_reader(Cursor::new(input), ImportOptions::new("svc")).unwrap();
        assert!(imported.run().requests.is_empty());
        assert!(imported.run().stages.is_empty());
        assert!(imported.run().queues.is_empty());
        assert_eq!(imported.warnings().len(), 1);
        let msg = imported.warnings()[0].message();
        assert!(msg.contains("line 1"));
        assert!(msg.contains("normalized span shape") || msg.contains("explicit timestamps"));
        assert!(imported
            .run()
            .metadata
            .lifecycle_warnings
            .iter()
            .any(|w| w == msg));
    }

    #[test]
    fn top_level_tt_record_without_span_shape_errors_strict() {
        let input = r#"{"tt.kind":"request","tt.request_id":"r1","tt.route":"/checkout"}"#;
        let err = import_jsonl_reader(Cursor::new(input), ImportOptions::new("svc").strict(true))
            .unwrap_err();
        assert!(matches!(err, ImportError::StrictViolation(_)));
        assert!(
            err.to_string().contains("normalized span shape")
                || err.to_string().contains("explicit timestamps")
        );
    }

    #[test]
    fn unrelated_top_level_json_record_still_ignored() {
        let input = r#"{"message":"ordinary log","level":"info","request_id":"r1"}"#;
        let imported = import_jsonl_reader(Cursor::new(input), ImportOptions::new("svc")).unwrap();
        assert!(imported.run().requests.is_empty());
        assert!(imported.run().stages.is_empty());
        assert!(imported.run().queues.is_empty());
        assert!(imported.warnings().is_empty());
    }

    #[test]
    fn empty_service_name_is_rejected_for_jsonl_import() {
        let err = import_jsonl_reader(Cursor::new(""), ImportOptions::new("")).unwrap_err();
        assert!(matches!(err, ImportError::EmptyServiceName));
    }
}
