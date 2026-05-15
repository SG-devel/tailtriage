use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;

use serde_json::Value;

use crate::{FieldValue, ImportError, ImportOptions, ImportedRun, SpanRecord, TT_KIND};

pub(crate) fn import_jsonl_reader<R: Read>(
    reader: R,
    options: ImportOptions,
) -> Result<ImportedRun, ImportError> {
    let mut spans = Vec::new();
    let reader = BufReader::new(reader);

    for (line_no, line_result) in reader.lines().enumerate() {
        let line = line_result.map_err(|reason| ImportError::InvalidField {
            field: "jsonl",
            reason: format!("failed to read line {}: {reason}", line_no + 1),
        })?;
        if line.trim().is_empty() {
            continue;
        }

        let value: Value =
            serde_json::from_str(&line).map_err(|reason| ImportError::InvalidField {
                field: "jsonl",
                reason: format!("malformed JSON at line {}: {reason}", line_no + 1),
            })?;

        if let Some(span) = parse_span_record(&value) {
            spans.push(span);
        }
    }

    crate::run_from_span_records(spans, options)
}

pub(crate) fn import_jsonl_path(
    path: impl AsRef<Path>,
    options: ImportOptions,
) -> Result<ImportedRun, ImportError> {
    let path_ref = path.as_ref();
    let file = std::fs::File::open(path_ref).map_err(|reason| ImportError::InvalidField {
        field: "jsonl_path",
        reason: format!("failed to open '{}': {reason}", path_ref.display()),
    })?;
    import_jsonl_reader(file, options)
}

fn parse_span_record(value: &Value) -> Option<SpanRecord> {
    let span_obj = value.get("span").and_then(Value::as_object);

    let name = find_name(value, span_obj)?;
    let started_at_unix_ms =
        find_timestamp(value, span_obj, "started_at_unix_ms", "start_unix_ms")?;
    let finished_at_unix_ms =
        find_timestamp(value, span_obj, "finished_at_unix_ms", "end_unix_ms")?;

    let mut fields = BTreeMap::new();
    collect_fields(&mut fields, value.get("fields"));
    collect_fields(&mut fields, span_obj.and_then(|obj| obj.get("fields")));
    if let Some(tt_kind) = nested_string(value.get("fields"), "tt.kind") {
        fields.insert(TT_KIND.to_owned(), FieldValue::String(tt_kind.to_owned()));
    }
    if let Some(tt_kind) = nested_string(span_obj.and_then(|obj| obj.get("fields")), "tt.kind") {
        fields.insert(TT_KIND.to_owned(), FieldValue::String(tt_kind.to_owned()));
    }
    if let Some(tt_kind) = value.get("tt.kind").and_then(Value::as_str) {
        fields.insert(TT_KIND.to_owned(), FieldValue::String(tt_kind.to_owned()));
    }

    if !fields.contains_key(TT_KIND) {
        return None;
    }

    let id = span_obj
        .and_then(|obj| obj.get("id"))
        .and_then(Value::as_str);
    let parent_id = span_obj
        .and_then(|obj| obj.get("parent_id"))
        .and_then(Value::as_str);

    let mut span = SpanRecord::new(name, started_at_unix_ms, finished_at_unix_ms);
    if let Some(id) = id {
        span = span.id(id);
    }
    if let Some(parent_id) = parent_id {
        span = span.parent_id(parent_id);
    }
    for (key, value) in fields {
        span = span.field(key, value);
    }

    Some(span)
}

fn find_name<'a>(
    value: &'a Value,
    span_obj: Option<&'a serde_json::Map<String, Value>>,
) -> Option<&'a str> {
    span_obj
        .and_then(|obj| obj.get("name"))
        .and_then(Value::as_str)
        .or_else(|| value.get("span_name").and_then(Value::as_str))
        .or_else(|| value.get("name").and_then(Value::as_str))
}

fn find_timestamp(
    value: &Value,
    span_obj: Option<&serde_json::Map<String, Value>>,
    key: &str,
    alias: &str,
) -> Option<u64> {
    span_obj
        .and_then(|obj| obj.get(key).or_else(|| obj.get(alias)))
        .and_then(Value::as_u64)
        .or_else(|| {
            value
                .get(key)
                .or_else(|| value.get(alias))
                .and_then(Value::as_u64)
        })
}

fn nested_string<'a>(value: Option<&'a Value>, dotted_key: &str) -> Option<&'a str> {
    value
        .and_then(Value::as_object)
        .and_then(|obj| obj.get(dotted_key))
        .and_then(Value::as_str)
}

fn collect_fields(target: &mut BTreeMap<String, FieldValue>, value: Option<&Value>) {
    let Some(object) = value.and_then(Value::as_object) else {
        return;
    };
    for (key, raw) in object {
        if let Some(field_value) = to_field_value(raw) {
            target.insert(key.clone(), field_value);
        }
    }
}

fn to_field_value(value: &Value) -> Option<FieldValue> {
    match value {
        Value::String(v) => Some(FieldValue::String(v.clone())),
        Value::Bool(v) => Some(FieldValue::Bool(*v)),
        Value::Number(v) => {
            if let Some(u) = v.as_u64() {
                Some(FieldValue::U64(u))
            } else if let Some(i) = v.as_i64() {
                Some(FieldValue::I64(i))
            } else {
                v.as_f64().map(FieldValue::F64)
            }
        }
        Value::Null => Some(FieldValue::Null),
        Value::Array(_) | Value::Object(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{import_jsonl_path, import_jsonl_reader, ImportOptions};

    #[test]
    fn normalized_jsonl_request_only() {
        let input = r#"{"span":{"name":"req","started_at_unix_ms":1,"finished_at_unix_ms":2,"fields":{"tt.kind":"request","tt.request_id":"r1","tt.route":"/a"}}}"#;
        let imported = import_jsonl_reader(input.as_bytes(), ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.run().requests.len(), 1);
    }

    #[test]
    fn normalized_jsonl_request_stage_queue() {
        let input = [
            r#"{"span":{"name":"req","started_at_unix_ms":1,"finished_at_unix_ms":5,"fields":{"tt.kind":"request","tt.request_id":"r1","tt.route":"/a"}}}"#,
            r#"{"span":{"name":"st","start_unix_ms":2,"end_unix_ms":3,"fields":{"tt.kind":"stage","tt.request_id":"r1","tt.stage":"db"}}}"#,
            r#"{"span":{"name":"q","started_at_unix_ms":3,"finished_at_unix_ms":4,"fields":{"tt.kind":"queue","tt.request_id":"r1","tt.queue":"permits"}}}"#,
        ]
        .join("\n");
        let imported = import_jsonl_reader(input.as_bytes(), ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.run().requests.len(), 1);
        assert_eq!(imported.run().stages.len(), 1);
        assert_eq!(imported.run().queues.len(), 1);
    }

    #[test]
    fn unrelated_json_line_ignored_and_empty_lines_ignored() {
        let input = "\n{\"level\":\"info\",\"message\":\"hello\"}\n\n";
        let imported = import_jsonl_reader(input.as_bytes(), ImportOptions::new("svc")).unwrap();
        assert!(imported.run().requests.is_empty());
        assert!(imported.warnings().is_empty());
    }

    #[test]
    fn malformed_json_returns_error() {
        let err =
            import_jsonl_reader("{not json}\n".as_bytes(), ImportOptions::new("svc")).unwrap_err();
        assert!(matches!(
            err,
            ImportError::InvalidField { field: "jsonl", .. }
        ));
    }

    #[test]
    fn missing_required_fields_warns_via_conversion_core() {
        let input = r#"{"span":{"name":"req","started_at_unix_ms":1,"finished_at_unix_ms":2,"fields":{"tt.kind":"request","tt.request_id":"r1"}}}"#;
        let imported = import_jsonl_reader(input.as_bytes(), ImportOptions::new("svc")).unwrap();
        assert!(imported.run().requests.is_empty());
        assert_eq!(imported.warnings().len(), 1);
    }

    #[test]
    fn strict_mode_propagates_conversion_errors() {
        let input = r#"{"span":{"name":"req","started_at_unix_ms":1,"finished_at_unix_ms":2,"fields":{"tt.kind":"request","tt.request_id":"r1"}}}"#;
        assert!(
            import_jsonl_reader(input.as_bytes(), ImportOptions::new("svc").strict(true)).is_err()
        );
    }

    #[test]
    fn path_based_import_works() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("spans.jsonl");
        std::fs::write(
            &path,
            r#"{"span":{"name":"req","started_at_unix_ms":1,"finished_at_unix_ms":2,"fields":{"tt.kind":"request","tt.request_id":"r1","tt.route":"/"}}}"#,
        )
        .unwrap();
        let imported = import_jsonl_path(&path, ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.run().requests.len(), 1);
    }
}
