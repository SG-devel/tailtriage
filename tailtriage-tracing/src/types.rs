use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

/// Supported `tt.kind` values for span records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpanKind {
    /// A request-level span.
    Request,
    /// A stage-level span.
    Stage,
    /// A queue-level span.
    Queue,
}

/// Scalar field value captured on an imported span.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FieldValue {
    /// UTF-8 string field value.
    String(String),
    /// Boolean field value.
    Bool(bool),
    /// Unsigned integer field value.
    U64(u64),
    /// Signed integer field value.
    I64(i64),
    /// Floating-point field value.
    F64(f64),
    /// Explicit null value.
    Null,
}

/// Minimal span-shaped record for future tracing intake conversion.
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

    /// Adds or replaces a field and returns the updated record.
    #[must_use]
    pub fn field(mut self, key: impl Into<String>, value: FieldValue) -> Self {
        self.fields.insert(key.into(), value);
        self
    }

    /// Sets the optional span id.
    #[must_use]
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Sets the optional parent span id.
    #[must_use]
    pub fn parent_id(mut self, parent_id: impl Into<String>) -> Self {
        self.parent_id = Some(parent_id.into());
        self
    }

    /// Returns the optional span id.
    #[must_use]
    pub fn id_ref(&self) -> Option<&str> {
        self.id.as_deref()
    }
    /// Returns the optional parent span id.
    #[must_use]
    pub fn parent_id_ref(&self) -> Option<&str> {
        self.parent_id.as_deref()
    }
    /// Returns the span name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Returns all span fields.
    #[must_use]
    pub fn fields(&self) -> &BTreeMap<String, FieldValue> {
        &self.fields
    }
    /// Returns span start time in Unix milliseconds.
    #[must_use]
    pub fn started_at_unix_ms(&self) -> u64 {
        self.started_at_unix_ms
    }
    /// Returns span end time in Unix milliseconds.
    #[must_use]
    pub fn finished_at_unix_ms(&self) -> u64 {
        self.finished_at_unix_ms
    }
}

/// Import configuration for converting span-shaped input into a run artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportOptions {
    service_name: String,
    service_version: Option<String>,
    run_id: Option<String>,
    strict: bool,
}

impl ImportOptions {
    /// Creates options with a required service name.
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

    /// Sets an explicit run id.
    #[must_use]
    pub fn run_id(mut self, run_id: impl Into<String>) -> Self {
        self.run_id = Some(run_id.into());
        self
    }

    /// Sets an optional service version label.
    #[must_use]
    pub fn service_version(mut self, service_version: impl Into<String>) -> Self {
        self.service_version = Some(service_version.into());
        self
    }

    /// Returns the configured service name.
    #[must_use]
    pub fn service_name(&self) -> &str {
        &self.service_name
    }
    /// Returns the configured optional service version.
    #[must_use]
    pub fn service_version_ref(&self) -> Option<&str> {
        self.service_version.as_deref()
    }
    /// Returns the configured optional run id.
    #[must_use]
    pub fn run_id_ref(&self) -> Option<&str> {
        self.run_id.as_deref()
    }
    /// Returns whether strict mode is enabled.
    #[must_use]
    pub fn strict_enabled(&self) -> bool {
        self.strict
    }
}

/// Warning surfaced by import workflows while still producing output.
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
    /// Returns the warning text.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for ImportWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

/// Result of a future tracing import conversion.
#[derive(Debug, Clone)]
pub struct ImportedRun {
    run: tailtriage_core::Run,
    warnings: Vec<ImportWarning>,
}

impl ImportedRun {
    /// Creates an imported run value from a run and warnings.
    #[must_use]
    pub fn new(run: tailtriage_core::Run, warnings: Vec<ImportWarning>) -> Self {
        Self { run, warnings }
    }

    /// Returns the converted run artifact.
    #[must_use]
    pub fn run(&self) -> &tailtriage_core::Run {
        &self.run
    }

    /// Returns non-fatal import warnings.
    #[must_use]
    pub fn warnings(&self) -> &[ImportWarning] {
        &self.warnings
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn span_record_builder_stores_fields() {
        let record = SpanRecord::new("request", 10, 20)
            .id("span-1")
            .parent_id("root")
            .field("tt.request_id", FieldValue::String("req-1".to_string()))
            .field("tt.success", FieldValue::Bool(true));

        assert_eq!(record.id_ref(), Some("span-1"));
        assert_eq!(record.parent_id_ref(), Some("root"));
        assert_eq!(record.fields().len(), 2);
    }

    #[test]
    fn import_options_builder_sets_flags() {
        let options = ImportOptions::new("checkout")
            .service_version("1.2.3")
            .run_id("run-123")
            .strict(true);

        assert_eq!(options.service_name(), "checkout");
        assert_eq!(options.service_version_ref(), Some("1.2.3"));
        assert_eq!(options.run_id_ref(), Some("run-123"));
        assert!(options.strict_enabled());
    }

    #[test]
    fn import_warning_display_matches_message() {
        let warning = ImportWarning::new("missing parent_id");
        assert_eq!(warning.to_string(), "missing parent_id");
    }
}
