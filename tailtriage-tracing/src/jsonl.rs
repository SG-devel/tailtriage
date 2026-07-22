use std::io::{BufRead, BufReader, Read};
use std::path::Path;

use serde_json::Value;

use crate::{run_from_span_records, ImportError, ImportOptions, ImportedRun, SpanRecord};

const FORMAT_MARKER: &str = "tailtriage.tracing-span.v1";

/// Imports newline-delimited stable completed-span JSONL records from a reader into a converted run.
///
/// This parser accepts only records shaped as
/// `{"format":"tailtriage.tracing-span.v1","span":{...}}`. Empty or
/// whitespace-only lines are ignored.
///
/// # Errors
///
/// Returns [`ImportError::Io`] for reader I/O failures,
/// [`ImportError::MalformedJsonLine`] for malformed non-empty JSONL lines,
/// [`ImportError::ExpectedTailtriageWrapper`] for structural JSONL shape errors,
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
        let line_no = line_no + 1;
        let line = line_result.map_err(|err| ImportError::Io {
            operation: "read jsonl line",
            context: format!("line {line_no}"),
            reason: err.to_string(),
        })?;

        if line.trim().is_empty() {
            continue;
        }

        let value: Value =
            serde_json::from_str(&line).map_err(|err| ImportError::MalformedJsonLine {
                line: line_no,
                reason: err.to_string(),
            })?;

        if let Some(span) = parse_record(line_no, &value, strict, &mut parse_warnings)? {
            spans.push(span);
        }
    }

    let imported = run_from_span_records(spans, options)?;
    let (mut run, mut conversion_warnings, retained_sources) = imported.into_internal_parts();
    attach_parse_warnings_to_lifecycle(&mut run, &parse_warnings);
    parse_warnings.append(&mut conversion_warnings);
    Ok(ImportedRun::with_retained_sources(
        run,
        parse_warnings,
        retained_sources,
    ))
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

/// Imports newline-delimited stable completed-span JSONL records from a filesystem path.
///
/// # Errors
///
/// Returns [`ImportError::Io`] when path open or line reads fail,
/// [`ImportError::MalformedJsonLine`] for malformed non-empty JSONL lines,
/// [`ImportError::ExpectedTailtriageWrapper`] for structural JSONL shape errors,
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
    let obj = value
        .as_object()
        .ok_or_else(|| wrapper_error(line_no, "JSONL record must be an object"))?;

    let format = obj
        .get("format")
        .ok_or_else(|| wrapper_error(line_no, classify_missing_format(obj)))?;
    let Some(format_marker) = format.as_str() else {
        return Err(wrapper_error(
            line_no,
            "invalid field 'format': expected string format marker",
        ));
    };
    if format_marker != FORMAT_MARKER {
        return Err(wrapper_error(
            line_no,
            format!("unsupported span format marker '{format_marker}'"),
        ));
    }

    let span_value = obj.get("span").ok_or_else(|| {
        wrapper_error(
            line_no,
            "missing field 'span' for tailtriage.tracing-span.v1 wrapper",
        )
    })?;
    if !span_value.is_object() {
        return Err(wrapper_error(
            line_no,
            "invalid field 'span': expected completed span object for tailtriage.tracing-span.v1",
        ));
    }

    match serde_json::from_value::<SpanRecord>(span_value.clone()) {
        Ok(span) => Ok(Some(span)),
        Err(err) => {
            let message = format!("line {line_no}: invalid tailtriage.tracing-span.v1 span: {err}");
            if strict {
                Err(ImportError::StrictViolation(message))
            } else {
                warnings.push(crate::ImportWarning::new(message));
                Ok(None)
            }
        }
    }
}

fn classify_missing_format(obj: &serde_json::Map<String, Value>) -> &'static str {
    if looks_like_ordinary_fmt_json(obj) {
        "ordinary tracing formatter JSON is unsupported; expected stable wrapper shape {\"format\":\"tailtriage.tracing-span.v1\",\"span\":{...}}"
    } else if obj.contains_key("span") {
        "missing field 'format'; unversioned tracing span envelopes are unsupported"
    } else if obj.contains_key("name")
        || obj.contains_key("start_unix_ms")
        || obj.contains_key("end_unix_ms")
    {
        "missing field 'format'; raw or pre-stable tracing span records are unsupported"
    } else if obj.keys().any(|k| k.starts_with("tt.")) || obj.contains_key("fields") {
        "missing field 'format'; compatibility field placement is unsupported"
    } else {
        "missing field 'format' for tailtriage.tracing-span.v1 wrapper"
    }
}

fn looks_like_ordinary_fmt_json(obj: &serde_json::Map<String, Value>) -> bool {
    obj.contains_key("timestamp") && obj.contains_key("level") && obj.contains_key("target")
}

fn wrapper_error(line_no: usize, reason: impl Into<String>) -> ImportError {
    ImportError::ExpectedTailtriageWrapper {
        reason: format!("line {line_no}: {}", reason.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FieldValue, ImportOptions};
    use std::io::Cursor;

    fn stable_request(name: &str, request_id: &str) -> String {
        format!(
            r#"{{"format":"tailtriage.tracing-span.v1","span":{{"name":"{name}","started_at_unix_ms":1,"finished_at_unix_ms":2,"fields":{{"tt.kind":"request","tt.request_id":"{request_id}","tt.route":"/a"}}}}}}"#
        )
    }

    fn assert_wrapper_rejected(input: &str, line: usize, text: &str) {
        let err = import_jsonl_reader(Cursor::new(input), ImportOptions::new("svc"))
            .expect_err("record should be structurally rejected");
        match err {
            ImportError::ExpectedTailtriageWrapper { reason } => {
                assert!(reason.contains(&format!("line {line}")), "{reason}");
                assert!(reason.contains(text), "{reason}");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn stable_wrapper_fixture_imports() {
        let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/tailtriage-span-v1.jsonl");
        let imported = import_jsonl_path(fixture, ImportOptions::new("svc")).unwrap();
        assert!(!imported.run().requests.is_empty());
    }

    #[test]
    fn rejects_raw_top_level_span_record() {
        assert_wrapper_rejected(
            r#"{"name":"request","started_at_unix_ms":1,"finished_at_unix_ms":2,"fields":{"tt.kind":"request","tt.request_id":"r1","tt.route":"/"}}"#,
            1,
            "raw or pre-stable",
        );
    }

    #[test]
    fn rejects_unversioned_span_envelope() {
        assert_wrapper_rejected(
            r#"{"span":{"name":"request","started_at_unix_ms":1,"finished_at_unix_ms":2,"fields":{"tt.kind":"request","tt.request_id":"r1","tt.route":"/"}}}"#,
            1,
            "unversioned",
        );
    }

    #[test]
    fn rejects_start_unix_ms_and_end_unix_ms_aliases() {
        assert_wrapper_rejected(
            r#"{"name":"request","start_unix_ms":1,"end_unix_ms":2,"fields":{"tt.kind":"request","tt.request_id":"r1","tt.route":"/"}}"#,
            1,
            "raw or pre-stable",
        );
    }

    #[test]
    fn rejects_top_level_tt_fields() {
        assert_wrapper_rejected(
            r#"{"span":{"name":"request","started_at_unix_ms":1,"finished_at_unix_ms":2},"tt.kind":"request","tt.request_id":"r1","tt.route":"/"}"#,
            1,
            "unversioned",
        );
    }

    #[test]
    fn rejects_outer_fields_compatibility_input() {
        assert_wrapper_rejected(
            r#"{"span":{"name":"request","started_at_unix_ms":1,"finished_at_unix_ms":2},"fields":{"tt.kind":"request","tt.request_id":"r1","tt.route":"/"}}"#,
            1,
            "unversioned",
        );
    }

    #[test]
    fn rejects_mixed_compatibility_field_locations() {
        assert_wrapper_rejected(
            r#"{"span":{"name":"request","started_at_unix_ms":1,"finished_at_unix_ms":2,"fields":{"tt.kind":"request"}},"fields":{"tt.request_id":"r1"},"tt.route":"/"}"#,
            1,
            "unversioned",
        );
    }

    #[test]
    fn rejects_ordinary_tracing_formatter_json() {
        assert_wrapper_rejected(
            r#"{"timestamp":"2026-01-01T00:00:00Z","level":"INFO","target":"svc","fields":{"message":"hello"}}"#,
            1,
            "ordinary tracing formatter JSON is unsupported",
        );
    }

    #[test]
    fn rejects_missing_format() {
        assert_wrapper_rejected(r#"{"message":"x"}"#, 1, "missing field 'format'");
    }

    #[test]
    fn rejects_non_string_format() {
        assert_wrapper_rejected(r#"{"format":1,"span":{}}"#, 1, "invalid field 'format'");
    }

    #[test]
    fn rejects_unsupported_format_marker() {
        assert_wrapper_rejected(
            r#"{"format":"other","span":{}}"#,
            1,
            "unsupported span format marker 'other'",
        );
    }

    #[test]
    fn rejects_missing_span() {
        assert_wrapper_rejected(
            r#"{"format":"tailtriage.tracing-span.v1"}"#,
            1,
            "missing field 'span'",
        );
    }

    #[test]
    fn rejects_non_object_span() {
        assert_wrapper_rejected(
            r#"{"format":"tailtriage.tracing-span.v1","span":"bad"}"#,
            1,
            "invalid field 'span'",
        );
    }

    #[test]
    fn rejects_malformed_json_with_line_number() {
        let err =
            import_jsonl_reader(Cursor::new("\n{bad"), ImportOptions::new("svc")).unwrap_err();
        match err {
            ImportError::MalformedJsonLine { line, .. } => assert_eq!(line, 2),
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn rejects_non_object_json_value() {
        assert_wrapper_rejected("[]", 1, "JSONL record must be an object");
    }

    #[test]
    fn structural_errors_are_fatal_even_non_strict() {
        let input = format!(
            "{}\n\n{{\"name\":\"legacy\",\"started_at_unix_ms\":1,\"finished_at_unix_ms\":2}}\n{}",
            stable_request("req", "r1"),
            stable_request("stage", "r2")
        );
        let err = import_jsonl_reader(Cursor::new(input), ImportOptions::new("svc").strict(false))
            .unwrap_err();
        match err {
            ImportError::ExpectedTailtriageWrapper { reason } => {
                assert!(reason.contains("line 3"));
                assert!(reason.contains("raw or pre-stable"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn valid_only_multiline_preserves_source_order() {
        let input = format!(
            "{}\n{}",
            stable_request("req-a", "r1"),
            stable_request("req-b", "r2")
        );
        let imported = import_jsonl_reader(Cursor::new(input), ImportOptions::new("svc")).unwrap();
        let retained = imported.retained_sources();
        assert_eq!(retained.len(), 2);
        assert_eq!(retained[0].name(), "req-a");
        assert_eq!(retained[1].name(), "req-b");
    }

    #[test]
    fn stable_wrapper_duration_us_is_authoritative_when_wall_timestamps_disagree() {
        let input = r#"{"format":"tailtriage.tracing-span.v1","span":{"name":"request","started_at_unix_ms":1700000000000,"started_at_run_us":0,"finished_at_unix_ms":1700000000001,"finished_at_run_us":1000,"duration_us":50000,"fields":{"tt.kind":"request","tt.request_id":"req-1","tt.route":"/checkout","tt.outcome":"ok"}}}"#;
        let imported = import_jsonl_reader(Cursor::new(input), ImportOptions::new("svc"))
            .expect("non-strict import should retain mismatched duration");
        assert_eq!(imported.run().requests.len(), 1);
        assert_eq!(imported.run().requests[0].latency_us, 50_000);
        assert!(imported
            .warnings()
            .iter()
            .any(|warning| warning.message().contains("duration_mismatch")));
        let err = import_jsonl_reader(Cursor::new(input), ImportOptions::new("svc").strict(true))
            .expect_err("strict import should reject mismatched duration");
        assert!(err.to_string().contains("duration_mismatch"));
    }

    #[test]
    fn invalid_contained_span_warns_non_strict_and_errors_strict() {
        let input = r#"{"format":"tailtriage.tracing-span.v1","span":{"name":"req","started_at_unix_ms":"bad","finished_at_unix_ms":2,"fields":{"tt.kind":"request","tt.request_id":"r1","tt.route":"/a"}}}"#;
        let imported = import_jsonl_reader(Cursor::new(input), ImportOptions::new("svc")).unwrap();
        assert!(imported.run().requests.is_empty());
        assert!(imported
            .warnings()
            .iter()
            .any(|w| w.message().contains("line 1")));
        let err = import_jsonl_reader(Cursor::new(input), ImportOptions::new("svc").strict(true))
            .unwrap_err();
        assert!(err.to_string().contains("line 1"));
    }

    #[test]
    fn stable_writer_output_reimports_with_explicit_evidence() {
        let span = SpanRecord::new("http.request", 1000, 1100)
            .id("span-id")
            .parent_id("parent-id")
            .started_at_run_us(10)
            .finished_at_run_us(100_010)
            .duration_us(100_000)
            .field(crate::TT_KIND, "request")
            .field(crate::TT_REQUEST_ID, "req-1")
            .field(crate::TT_ROUTE, "/checkout")
            .field("custom", "kept");
        let jsonl =
            serde_json::json!({"format":"tailtriage.tracing-span.v1","span":span}).to_string();
        let imported = import_jsonl_reader(Cursor::new(jsonl), ImportOptions::new("svc")).unwrap();
        let retained = imported.retained_sources();
        assert_eq!(retained.len(), 1);
        let source = &retained[0];
        assert_eq!(source.id_ref(), Some("span-id"));
        assert_eq!(source.parent_id_ref(), Some("parent-id"));
        assert_eq!(source.name(), "http.request");
        assert_eq!(source.started_at_unix_ms(), 1000);
        assert_eq!(source.finished_at_unix_ms(), 1100);
        assert_eq!(source.started_at_run_us_ref(), Some(10));
        assert_eq!(source.finished_at_run_us_ref(), Some(100_010));
        assert_eq!(source.duration_us_ref(), Some(100_000));
        assert_eq!(
            source.fields().get(crate::TT_KIND),
            Some(&FieldValue::String("request".to_owned()))
        );
        assert_eq!(
            source.fields().get("custom"),
            Some(&FieldValue::String("kept".to_owned()))
        );
    }
}
