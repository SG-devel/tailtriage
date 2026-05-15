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
    let reader = BufReader::new(reader);

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

        if let Some(span) = parse_record(&value)? {
            spans.push(span);
        }
    }

    run_from_span_records(spans, options)
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

fn parse_record(value: &Value) -> Result<Option<SpanRecord>, ImportError> {
    if let Some(span_obj) = value.get("span").and_then(Value::as_object) {
        let has_tt_kind = span_obj
            .get("fields")
            .and_then(Value::as_object)
            .is_some_and(|fields| fields.contains_key(TT_KIND));
        let has_name = span_obj.contains_key("name");
        let has_start =
            span_obj.contains_key("started_at_unix_ms") || span_obj.contains_key("start_unix_ms");
        let has_finish =
            span_obj.contains_key("finished_at_unix_ms") || span_obj.contains_key("end_unix_ms");
        if has_tt_kind && !(has_name && has_start && has_finish) && !indicates_close_event(value) {
            return Err(ImportError::InvalidField {
                field: "span",
                reason: "tailtriage span with fields.tt.kind must include name, started_at_unix_ms/start_unix_ms, and finished_at_unix_ms/end_unix_ms".to_owned(),
            });
        }
        if has_name && has_start && has_finish {
            return parse_normalized_span(span_obj).map(Some);
        }
    }

    parse_close_event_shape(value)
}

fn parse_normalized_span(
    span_obj: &serde_json::Map<String, Value>,
) -> Result<SpanRecord, ImportError> {
    let name = required_string(span_obj, "name")?;
    let id = optional_string(span_obj, "id")?;
    let parent_id = optional_string(span_obj, "parent_id")?;
    let started_at_unix_ms = required_timestamp(span_obj, "started_at_unix_ms", "start_unix_ms")?;
    let finished_at_unix_ms = required_timestamp(span_obj, "finished_at_unix_ms", "end_unix_ms")?;

    let mut span = SpanRecord::new(name, started_at_unix_ms, finished_at_unix_ms);
    if let Some(id) = id {
        span = span.id(id);
    }
    if let Some(parent_id) = parent_id {
        span = span.parent_id(parent_id);
    }

    let fields = extract_fields_for_span(&Value::Object(span_obj.clone()));
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
        let input = r#"{"event":"close","span":{"name":"st","fields":{"tt.kind":"stage","tt.request_id":"r1","tt.stage":"db"}},"started_at_unix_ms":5,"finished_at_unix_ms":8}"#;
        let imported = import_jsonl_reader(Cursor::new(input), ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.run().stages.len(), 1);
        assert_eq!(imported.run().stages[0].stage, "db");
    }

    #[test]
    fn incomplete_normalized_tt_kind_span_errors_in_strict_and_non_strict_modes() {
        let input = r#"{"span":{"name":"req","fields":{"tt.kind":"request","tt.request_id":"r1","tt.route":"/a"}}}"#;
        let err = import_jsonl_reader(Cursor::new(input), ImportOptions::new("svc")).unwrap_err();
        assert!(matches!(
            err,
            ImportError::InvalidField { field: "span", .. }
        ));

        let err = import_jsonl_reader(Cursor::new(input), ImportOptions::new("svc").strict(true))
            .unwrap_err();
        assert!(matches!(
            err,
            ImportError::InvalidField { field: "span", .. }
        ));
    }

    #[test]
    fn close_event_shape_with_nested_span_timestamps_is_supported() {
        let input = r#"{"event":"close","span":{"name":"st","started_at_unix_ms":5,"finished_at_unix_ms":8,"fields":{"tt.kind":"stage","tt.request_id":"r1","tt.stage":"db"}}}"#;
        let imported = import_jsonl_reader(Cursor::new(input), ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.run().stages.len(), 1);
        assert_eq!(imported.run().stages[0].stage, "db");
    }
}
