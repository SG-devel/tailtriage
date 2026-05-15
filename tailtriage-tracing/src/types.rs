use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Canonical kind for a span-like record used during import.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpanKind {
    /// Request lifecycle span.
    Request,
    /// Application stage span.
    Stage,
    /// Queue wait span.
    Queue,
}

/// Scalar field value captured from a span-like record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FieldValue {
    /// String value.
    String(String),
    /// Boolean value.
    Bool(bool),
    /// Unsigned integer value.
    U64(u64),
    /// Signed integer value.
    I64(i64),
    /// Floating-point value.
    F64(f64),
    /// Explicit null.
    Null,
}

impl From<&str> for FieldValue {
    fn from(value: &str) -> Self {
        Self::String(value.to_owned())
    }
}
impl From<String> for FieldValue {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}
impl From<bool> for FieldValue {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}
impl From<u64> for FieldValue {
    fn from(value: u64) -> Self {
        Self::U64(value)
    }
}
impl From<i64> for FieldValue {
    fn from(value: i64) -> Self {
        Self::I64(value)
    }
}
impl From<f64> for FieldValue {
    fn from(value: f64) -> Self {
        Self::F64(value)
    }
}

/// Span-like record that future import paths will convert into tailtriage events.
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
    /// Create a span-like record with no id, parent id, or fields.
    #[must_use]
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

    /// Add or replace a field on this record.
    #[must_use]
    pub fn field(mut self, key: impl Into<String>, value: impl Into<FieldValue>) -> Self {
        self.fields.insert(key.into(), value.into());
        self
    }

    #[must_use]
    pub fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }
    #[must_use]
    pub fn parent_id(&self) -> Option<&str> {
        self.parent_id.as_deref()
    }
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
    #[must_use]
    pub fn fields(&self) -> &BTreeMap<String, FieldValue> {
        &self.fields
    }
    #[must_use]
    pub fn started_at_unix_ms(&self) -> u64 {
        self.started_at_unix_ms
    }
    #[must_use]
    pub fn finished_at_unix_ms(&self) -> u64 {
        self.finished_at_unix_ms
    }
}

/// Import configuration for converting trace-shaped input into a run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportOptions {
    service_name: String,
    service_version: Option<String>,
    run_id: Option<String>,
    strict: bool,
}

impl ImportOptions {
    /// Create options with a required service name.
    #[must_use]
    pub fn new(service_name: impl Into<String>) -> Self {
        Self {
            service_name: service_name.into(),
            service_version: None,
            run_id: None,
            strict: false,
        }
    }

    #[must_use]
    pub fn strict(mut self, strict: bool) -> Self {
        self.strict = strict;
        self
    }
    #[must_use]
    pub fn run_id(mut self, run_id: impl Into<String>) -> Self {
        self.run_id = Some(run_id.into());
        self
    }
    #[must_use]
    pub fn service_version(mut self, service_version: impl Into<String>) -> Self {
        self.service_version = Some(service_version.into());
        self
    }

    #[must_use]
    pub fn service_name(&self) -> &str {
        &self.service_name
    }
    #[must_use]
    pub fn service_version_value(&self) -> Option<&str> {
        self.service_version.as_deref()
    }
    #[must_use]
    pub fn run_id_value(&self) -> Option<&str> {
        self.run_id.as_deref()
    }
    #[must_use]
    pub fn strict_mode(&self) -> bool {
        self.strict
    }
}

/// Non-fatal import note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportWarning {
    message: String,
}
impl ImportWarning {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}
impl std::fmt::Display for ImportWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

/// Result container for conversion output.
#[derive(Debug, Clone)]
pub struct ImportedRun {
    run: tailtriage_core::Run,
    warnings: Vec<ImportWarning>,
}

impl ImportedRun {
    #[must_use]
    pub fn new(run: tailtriage_core::Run, warnings: Vec<ImportWarning>) -> Self {
        Self { run, warnings }
    }
    #[must_use]
    pub fn run(&self) -> &tailtriage_core::Run {
        &self.run
    }
    #[must_use]
    pub fn warnings(&self) -> &[ImportWarning] {
        &self.warnings
    }
}
