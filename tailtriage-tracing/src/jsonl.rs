use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;

use serde_json::Value;

use crate::{
    run_from_span_records, FieldValue, ImportError, ImportOptions, ImportedRun, SpanRecord, TT_KIND,
};

/// Imports newline-delimited JSON span records and converts them into a triage run.
///
/// Empty lines are ignored. JSON parsing is line-oriented and malformed JSON lines
/// return an error immediately.
///
/// # Errors
///
/// Returns [`ImportError`] when input reading fails, a line is malformed JSON, or
/// a tailtriage-tagged span record is malformed according to import strictness rules.
pub fn import_jsonl_reader<R: Read>(
    reader: R,
    options: ImportOptions,
) -> Result<ImportedRun, ImportError> {
    let mut spans = Vec::new();
    for (line_no, line) in BufReader::new(reader).lines().enumerate() {
        let line = line.map_err(|error| ImportError::InvalidField {
            field: "jsonl",
            reason: format!("failed reading line {}: {error}", line_no + 1),
        })?;
        if line.trim().is_empty() {
            continue;
        }
        let value: Value =
            serde_json::from_str(&line).map_err(|error| ImportError::InvalidField {
                field: "jsonl",
                reason: format!("malformed JSON at line {}: {error}", line_no + 1),
            })?;
        if let Some(span) = parse_record(&value)? {
            spans.push(span);
        }
    }

    run_from_span_records(spans, options)
}

/// Opens a JSONL file and imports tracing-shaped span records into a triage run.
///
/// # Errors
///
/// Returns [`ImportError`] if the path cannot be opened or if reader-based import fails.
pub fn import_jsonl_path(
    path: impl AsRef<Path>,
    options: ImportOptions,
) -> Result<ImportedRun, ImportError> {
    let file = std::fs::File::open(path.as_ref()).map_err(|error| ImportError::InvalidField {
        field: "jsonl",
        reason: format!("failed opening {}: {error}", path.as_ref().display()),
    })?;
    import_jsonl_reader(file, options)
}

fn parse_record(value: &Value) -> Result<Option<SpanRecord>, ImportError> {
    if let Some(span) = parse_normalized_span(value)? {
        return Ok(Some(span));
    }

    parse_close_event_span(value)
}

fn parse_normalized_span(value: &Value) -> Result<Option<SpanRecord>, ImportError> {
    let Some(span_obj) = value.get("span").and_then(Value::as_object) else {
        return Ok(None);
    };
    parse_span_object(span_obj)
}

fn parse_close_event_span(value: &Value) -> Result<Option<SpanRecord>, ImportError> {
    if !is_close_event(value) {
        return Ok(None);
    }

    let Some(root) = value.as_object() else {
        return Ok(None);
    };

    let mut merged = serde_json::Map::new();
    for (key, val) in root {
        merged.insert(key.clone(), val.clone());
    }
    if let Some(span_obj) = value.get("span").and_then(Value::as_object) {
        for (key, val) in span_obj {
            merged.entry(key.clone()).or_insert_with(|| val.clone());
        }
    }
    parse_span_object(&merged)
}

fn parse_span_object(
    span_obj: &serde_json::Map<String, Value>,
) -> Result<Option<SpanRecord>, ImportError> {
    let fields = extract_fields(span_obj);
    if !fields.contains_key(TT_KIND) {
        return Ok(None);
    }

    let name = span_obj
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("unknown");

    let started = extract_u64(span_obj, &["started_at_unix_ms", "start_unix_ms"]);
    let finished = extract_u64(span_obj, &["finished_at_unix_ms", "end_unix_ms"]);

    let (Some(started_at_unix_ms), Some(finished_at_unix_ms)) = (started, finished) else {
        return Err(ImportError::InvalidField {
            field: "timestamps",
            reason: format!("tailtriage span '{name}' missing unix-ms start or finish timestamp"),
        });
    };

    let mut span = SpanRecord::new(name, started_at_unix_ms, finished_at_unix_ms);
    if let Some(id) = span_obj.get("id").and_then(Value::as_str) {
        span = span.id(id);
    }
    if let Some(parent_id) = span_obj.get("parent_id").and_then(Value::as_str) {
        span = span.parent_id(parent_id);
    }
    for (key, value) in fields {
        span = span.field(key, value);
    }
    Ok(Some(span))
}

fn is_close_event(value: &Value) -> bool {
    let event = value.get("event").and_then(Value::as_str);
    let message = value.get("message").and_then(Value::as_str);
    event.is_some_and(is_close_keyword) || message.is_some_and(is_close_keyword)
}

fn is_close_keyword(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("close") || lower.contains("closed")
}

fn extract_fields(span_obj: &serde_json::Map<String, Value>) -> BTreeMap<String, FieldValue> {
    let mut fields = BTreeMap::new();

    if let Some(obj) = span_obj.get("fields").and_then(Value::as_object) {
        flatten_object_fields(&mut fields, "", obj);
    }

    if let Some(top_kind) = span_obj.get(TT_KIND).and_then(value_to_field) {
        fields.insert(TT_KIND.to_owned(), top_kind);
    }

    fields
}

fn flatten_object_fields(
    out: &mut BTreeMap<String, FieldValue>,
    prefix: &str,
    obj: &serde_json::Map<String, Value>,
) {
    for (key, value) in obj {
        let full_key = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };

        if let Some(child) = value.as_object() {
            flatten_object_fields(out, &full_key, child);
        } else if let Some(field) = value_to_field(value) {
            out.insert(full_key, field);
        }
    }
}

fn value_to_field(value: &Value) -> Option<FieldValue> {
    match value {
        Value::String(v) => Some(FieldValue::String(v.clone())),
        Value::Bool(v) => Some(FieldValue::Bool(*v)),
        Value::Number(v) => {
            if let Some(i) = v.as_i64() {
                Some(FieldValue::I64(i))
            } else {
                v.as_f64().map(FieldValue::F64)
            }
        }
        _ => None,
    }
}

fn extract_u64(obj: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<u64> {
    keys.iter()
        .find_map(|key| obj.get(*key).and_then(Value::as_u64))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn normalized_request_only_imports() {
        let input = r#"{"span":{"name":"req","started_at_unix_ms":1,"finished_at_unix_ms":2,"fields":{"tt.kind":"request","tt.request_id":"r1","tt.route":"/"}}}"#;
        let imported =
            import_jsonl_reader(input.as_bytes(), ImportOptions::new("svc")).expect("ok");
        assert_eq!(imported.run().requests.len(), 1);
    }

    #[test]
    fn normalized_request_stage_queue_imports() {
        let input = concat!(
            "{\"span\":{\"name\":\"req\",\"started_at_unix_ms\":1,\"finished_at_unix_ms\":8,\"fields\":{\"tt.kind\":\"request\",\"tt.request_id\":\"r1\",\"tt.route\":\"/\"}}}\n",
            "{\"span\":{\"name\":\"st\",\"started_at_unix_ms\":2,\"finished_at_unix_ms\":5,\"fields\":{\"tt.kind\":\"stage\",\"tt.request_id\":\"r1\",\"tt.stage\":\"db\"}}}\n",
            "{\"span\":{\"name\":\"q\",\"start_unix_ms\":1,\"end_unix_ms\":2,\"fields\":{\"tt.kind\":\"queue\",\"tt.request_id\":\"r1\",\"tt.queue\":\"permit\"}}}\n"
        );
        let run = import_jsonl_reader(input.as_bytes(), ImportOptions::new("svc"))
            .unwrap()
            .run()
            .clone();
        assert_eq!(run.requests.len(), 1);
        assert_eq!(run.stages.len(), 1);
        assert_eq!(run.queues.len(), 1);
    }

    #[test]
    fn unrelated_line_ignored() {
        let input = "{\"message\":\"hello\"}\n";
        let imported = import_jsonl_reader(input.as_bytes(), ImportOptions::new("svc")).unwrap();
        assert!(imported.run().requests.is_empty());
    }

    #[test]
    fn empty_lines_ignored() {
        let input = "\n\n";
        let imported = import_jsonl_reader(input.as_bytes(), ImportOptions::new("svc")).unwrap();
        assert!(imported.run().requests.is_empty());
    }

    #[test]
    fn malformed_json_returns_error() {
        let input = "{not-json}\n";
        let err = import_jsonl_reader(input.as_bytes(), ImportOptions::new("svc")).unwrap_err();
        assert!(matches!(
            err,
            ImportError::InvalidField { field: "jsonl", .. }
        ));
    }

    #[test]
    fn missing_required_fields_warns_from_conversion_core() {
        let input = "{\"span\":{\"name\":\"req\",\"started_at_unix_ms\":1,\"finished_at_unix_ms\":2,\"fields\":{\"tt.kind\":\"request\"}}}\n";
        let imported = import_jsonl_reader(input.as_bytes(), ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.run().requests.len(), 0);
        assert!(!imported.warnings().is_empty());
    }

    #[test]
    fn strict_mode_propagates_conversion_errors() {
        let input = "{\"span\":{\"name\":\"req\",\"started_at_unix_ms\":1,\"finished_at_unix_ms\":2,\"fields\":{\"tt.kind\":\"request\"}}}\n";
        let err = import_jsonl_reader(input.as_bytes(), ImportOptions::new("svc").strict(true))
            .unwrap_err();
        assert!(matches!(err, ImportError::StrictViolation(_)));
    }

    #[test]
    fn path_import_works() {
        let file = NamedTempFile::new().expect("tmp");
        std::fs::write(
            file.path(),
            "{\"span\":{\"name\":\"req\",\"started_at_unix_ms\":1,\"finished_at_unix_ms\":2,\"fields\":{\"tt.kind\":\"request\",\"tt.request_id\":\"r1\",\"tt.route\":\"/\"}}}\n",
        )
        .expect("write");
        let imported = import_jsonl_path(file.path(), ImportOptions::new("svc")).unwrap();
        assert_eq!(imported.run().requests.len(), 1);
    }
}
