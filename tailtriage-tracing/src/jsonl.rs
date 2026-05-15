use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;

use serde_json::Value;

use crate::{FieldValue, ImportError, ImportOptions, ImportedRun, SpanRecord, TT_KIND};

/// Imports newline-delimited JSON span records from a reader.
///
/// The parser accepts normalized records and close-event records that include
/// explicit unix-ms start and finish timestamps.
///
/// # Errors
///
/// Returns [`ImportError::Io`] for read failures, [`ImportError::MalformedJsonLine`]
/// for malformed JSON lines, and conversion errors propagated from
/// [`crate::run_from_span_records`].
pub fn import_jsonl_reader<R: Read>(
    reader: R,
    options: ImportOptions,
) -> Result<ImportedRun, ImportError> {
    let mut spans = Vec::new();
    for line in BufReader::new(reader).lines() {
        let line = line.map_err(|e| ImportError::Io(e.to_string()))?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let value: Value =
            serde_json::from_str(trimmed).map_err(|e| ImportError::MalformedJsonLine {
                message: e.to_string(),
            })?;
        if let Some(span) = parse_line(&value)? {
            spans.push(span);
        }
    }

    crate::run_from_span_records(spans, options)
}

/// Imports newline-delimited JSON span records from a filesystem path.
///
/// # Errors
///
/// Returns [`ImportError::Io`] when opening the file fails, and propagates
/// parsing/conversion errors from [`import_jsonl_reader`].
pub fn import_jsonl_path(
    path: impl AsRef<Path>,
    options: ImportOptions,
) -> Result<ImportedRun, ImportError> {
    let file = std::fs::File::open(path.as_ref()).map_err(|e| ImportError::Io(e.to_string()))?;
    import_jsonl_reader(file, options)
}

fn parse_line(value: &Value) -> Result<Option<SpanRecord>, ImportError> {
    let Some(object) = value.as_object() else {
        return Ok(None);
    };

    if let Some(span) = object.get("span").and_then(Value::as_object) {
        if let Some(record) = parse_span_object(span)? {
            return Ok(Some(record));
        }
    }

    let is_close = object
        .get("event")
        .and_then(Value::as_str)
        .is_some_and(|v| v.eq_ignore_ascii_case("close"))
        || object
            .get("message")
            .and_then(Value::as_str)
            .is_some_and(|v| v.eq_ignore_ascii_case("close"));

    if !is_close {
        return Ok(None);
    }

    if let Some(record) = parse_close_event(object)? {
        return Ok(Some(record));
    }

    Ok(None)
}

fn parse_span_object(
    span: &serde_json::Map<String, Value>,
) -> Result<Option<SpanRecord>, ImportError> {
    let mut fields = extract_fields(None, Some(span), None);
    if !fields.contains_key(TT_KIND) {
        return Ok(None);
    }

    let name = span
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_owned();
    let started = read_ts(span, "started_at_unix_ms", "start_unix_ms")?;
    let finished = read_ts(span, "finished_at_unix_ms", "end_unix_ms")?;

    let (Some(started_at_unix_ms), Some(finished_at_unix_ms)) = (started, finished) else {
        return Err(ImportError::InvalidField {
            field: "span",
            reason: "tailtriage span missing start/end unix-ms timestamps".to_owned(),
        });
    };

    let mut record = SpanRecord::new(name, started_at_unix_ms, finished_at_unix_ms);
    if let Some(id) = span.get("id").and_then(Value::as_str) {
        record = record.id(id);
    }
    if let Some(parent_id) = span.get("parent_id").and_then(Value::as_str) {
        record = record.parent_id(parent_id);
    }
    for (k, v) in &mut fields {
        record = record.field(k.as_str(), v.clone());
    }
    Ok(Some(record))
}

fn parse_close_event(
    object: &serde_json::Map<String, Value>,
) -> Result<Option<SpanRecord>, ImportError> {
    let span_obj = object.get("span").and_then(Value::as_object);
    let fields_obj = object.get("fields").and_then(Value::as_object);
    let mut fields = extract_fields(fields_obj, span_obj, Some(object));
    if !fields.contains_key(TT_KIND) {
        return Ok(None);
    }

    let name = span_obj
        .and_then(|s| s.get("name"))
        .and_then(Value::as_str)
        .or_else(|| object.get("span_name").and_then(Value::as_str))
        .unwrap_or("unknown")
        .to_owned();

    let started = if let Some(s) = span_obj {
        read_ts(s, "started_at_unix_ms", "start_unix_ms")?
    } else {
        None
    }
    .or(read_ts(object, "started_at_unix_ms", "start_unix_ms")?);
    let finished = if let Some(s) = span_obj {
        read_ts(s, "finished_at_unix_ms", "end_unix_ms")?
    } else {
        None
    }
    .or(read_ts(object, "finished_at_unix_ms", "end_unix_ms")?);

    let (Some(started_at_unix_ms), Some(finished_at_unix_ms)) = (started, finished) else {
        return Err(ImportError::InvalidField {
            field: "span",
            reason: "tailtriage close event missing start/end unix-ms timestamps".to_owned(),
        });
    };

    let mut record = SpanRecord::new(name, started_at_unix_ms, finished_at_unix_ms);
    for (k, v) in &mut fields {
        record = record.field(k.as_str(), v.clone());
    }
    Ok(Some(record))
}

fn extract_fields(
    fields_obj: Option<&serde_json::Map<String, Value>>,
    span_obj: Option<&serde_json::Map<String, Value>>,
    top: Option<&serde_json::Map<String, Value>>,
) -> BTreeMap<String, FieldValue> {
    let mut out = BTreeMap::new();
    if let Some(fields) = fields_obj {
        append_values(&mut out, fields);
    }
    if let Some(span) = span_obj {
        if let Some(fields) = span.get("fields").and_then(Value::as_object) {
            append_values(&mut out, fields);
        }
    }
    if let Some(top) = top {
        append_values(&mut out, top);
    }
    out
}

fn append_values(out: &mut BTreeMap<String, FieldValue>, map: &serde_json::Map<String, Value>) {
    for (key, value) in map {
        if key == "fields" || key == "span" || key == "event" || key == "message" {
            continue;
        }
        if let Some(converted) = value_to_field(value) {
            out.insert(key.to_owned(), converted);
        }
    }
}

fn value_to_field(value: &Value) -> Option<FieldValue> {
    match value {
        Value::String(v) => Some(FieldValue::String(v.clone())),
        Value::Bool(v) => Some(FieldValue::Bool(*v)),
        Value::Number(v) => v
            .as_u64()
            .map(FieldValue::U64)
            .or_else(|| v.as_i64().map(FieldValue::I64))
            .or_else(|| v.as_f64().map(FieldValue::F64)),
        Value::Null => Some(FieldValue::Null),
        Value::Array(_) | Value::Object(_) => None,
    }
}

fn read_ts(
    map: &serde_json::Map<String, Value>,
    key: &'static str,
    alias: &'static str,
) -> Result<Option<u64>, ImportError> {
    map.get(key)
        .or_else(|| map.get(alias))
        .map(|v| match v.as_u64() {
            Some(ts) => Ok(ts),
            None => Err(ImportError::InvalidField {
                field: key,
                reason: "expected unix timestamp in milliseconds as u64".to_owned(),
            }),
        })
        .transpose()
}
