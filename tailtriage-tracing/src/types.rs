use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Semantic span kind used by tailtriage tracing intake.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpanKind {
    /// Request-level span.
    Request,
    /// Stage-level span.
    Stage,
    /// Queue-level span.
    Queue,
}

/// Supported scalar field values on imported spans.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FieldValue {
    /// String field value.
    String(String),
    /// Boolean field value.
    Bool(bool),
    /// Unsigned 64-bit integer field value.
    U64(u64),
    /// Signed 64-bit integer field value.
    I64(i64),
    /// 64-bit floating-point field value.
    F64(f64),
    /// Null field value.
    Null,
}

/// A tracing-shaped finished span record ready for intake conversion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpanRecord {
    id: Option<String>,
    parent_id: Option<String>,
    name: String,
    fields: BTreeMap<String, FieldValue>,
    started_at_unix_ms: u64,
    finished_at_unix_ms: u64,
}

impl SpanRecord {
    /// Creates a new span record with required timing fields.
    pub fn new(name: impl Into<String>, started_at_unix_ms: u64, finished_at_unix_ms: u64) -> Self {
        Self {
            id: None,
            parent_id: None,
            name: name.into(),
            fields: BTreeMap::new(),
            started_at_unix_ms,
            finished_at_unix_ms,
        }
    }

    /// Sets a span identifier.
    #[must_use]
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Sets the optional parent span identifier.
    #[must_use]
    pub fn parent_id(mut self, parent_id: impl Into<String>) -> Self {
        self.parent_id = Some(parent_id.into());
        self
    }

    /// Adds or replaces a field.
    #[must_use]
    pub fn field(mut self, key: impl Into<String>, value: FieldValue) -> Self {
        self.fields.insert(key.into(), value);
        self
    }

    /// Returns span id if present.
    #[must_use]
    pub fn id_ref(&self) -> Option<&str> {
        self.id.as_deref()
    }
    /// Returns parent span id if present.
    #[must_use]
    pub fn parent_id_ref(&self) -> Option<&str> {
        self.parent_id.as_deref()
    }
    /// Returns span name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Returns all fields.
    #[must_use]
    pub fn fields(&self) -> &BTreeMap<String, FieldValue> {
        &self.fields
    }
    /// Returns start timestamp in unix milliseconds.
    #[must_use]
    pub fn started_at_unix_ms(&self) -> u64 {
        self.started_at_unix_ms
    }
    /// Returns finish timestamp in unix milliseconds.
    #[must_use]
    pub fn finished_at_unix_ms(&self) -> u64 {
        self.finished_at_unix_ms
    }
}

/// Import options for converting tracing-shaped spans into a run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportOptions {
    service_name: String,
    service_version: Option<String>,
    run_id: Option<String>,
    strict: bool,
}

impl ImportOptions {
    /// Creates options with required service name.
    pub fn new(service_name: impl Into<String>) -> Self {
        Self {
            service_name: service_name.into(),
            service_version: None,
            run_id: None,
            strict: false,
        }
    }

    /// Enables or disables strict import mode.
    #[must_use]
    pub fn strict(mut self, strict: bool) -> Self {
        self.strict = strict;
        self
    }

    /// Sets an explicit run id for imported output.
    #[must_use]
    pub fn run_id(mut self, run_id: impl Into<String>) -> Self {
        self.run_id = Some(run_id.into());
        self
    }

    /// Sets service version metadata.
    #[must_use]
    pub fn service_version(mut self, service_version: impl Into<String>) -> Self {
        self.service_version = Some(service_version.into());
        self
    }

    /// Returns service name.
    #[must_use]
    pub fn service_name(&self) -> &str {
        &self.service_name
    }
    /// Returns service version.
    #[must_use]
    pub fn service_version_ref(&self) -> Option<&str> {
        self.service_version.as_deref()
    }
    /// Returns run id.
    #[must_use]
    pub fn run_id_ref(&self) -> Option<&str> {
        self.run_id.as_deref()
    }
    /// Returns strict mode setting.
    #[must_use]
    pub fn strict_mode(&self) -> bool {
        self.strict
    }
}

/// Non-fatal warning produced while importing tracing-shaped spans.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportWarning {
    message: String,
}

impl ImportWarning {
    /// Creates a warning message.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Returns warning message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl core::fmt::Display for ImportWarning {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.message)
    }
}

/// Result of a completed import operation.
#[derive(Debug, Clone)]
pub struct ImportedRun {
    /// Converted run artifact.
    pub run: tailtriage_core::Run,
    /// Non-fatal warnings emitted during conversion.
    pub warnings: Vec<ImportWarning>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn span_record_builder_stores_fields() {
        let record = SpanRecord::new("request", 10, 20)
            .id("span-1")
            .parent_id("parent-1")
            .field("tt.request_id", FieldValue::String("req-1".to_owned()))
            .field("tt.success", FieldValue::Bool(true));

        assert_eq!(record.id_ref(), Some("span-1"));
        assert_eq!(record.parent_id_ref(), Some("parent-1"));
        assert_eq!(record.name(), "request");
        assert_eq!(record.started_at_unix_ms(), 10);
        assert_eq!(record.finished_at_unix_ms(), 20);
        assert_eq!(record.fields().len(), 2);
    }

    #[test]
    fn import_options_builder_sets_values() {
        let options = ImportOptions::new("checkout-service")
            .service_version("1.2.3")
            .run_id("run-123")
            .strict(true);

        assert_eq!(options.service_name(), "checkout-service");
        assert_eq!(options.service_version_ref(), Some("1.2.3"));
        assert_eq!(options.run_id_ref(), Some("run-123"));
        assert!(options.strict_mode());
    }

    #[test]
    fn import_warning_display_matches_message() {
        let warning = ImportWarning::new("dropped unknown field");
        assert_eq!(warning.to_string(), "dropped unknown field");
    }
}
