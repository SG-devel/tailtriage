//! Public intake types for tracing-shaped span records.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};

/// The semantic category of a span record in tailtriage intake.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpanKind {
    /// Request-level span.
    Request,
    /// Stage-level span.
    Stage,
    /// Queue-level span.
    Queue,
}

/// Typed field values captured on imported span records.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FieldValue {
    String(String),
    Bool(bool),
    U64(u64),
    I64(i64),
    F64(f64),
    Null,
}

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
    #[must_use]
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }
    #[must_use]
    pub fn parent_id(mut self, parent_id: impl Into<String>) -> Self {
        self.parent_id = Some(parent_id.into());
        self
    }
    #[must_use]
    pub fn field(mut self, key: impl Into<String>, value: FieldValue) -> Self {
        self.fields.insert(key.into(), value);
        self
    }

    #[must_use]
    pub fn id_ref(&self) -> Option<&str> {
        self.id.as_deref()
    }
    #[must_use]
    pub fn parent_id_ref(&self) -> Option<&str> {
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportOptions {
    service_name: String,
    service_version: Option<String>,
    run_id: Option<String>,
    strict: bool,
}

impl ImportOptions {
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
    pub fn service_version_ref(&self) -> Option<&str> {
        self.service_version.as_deref()
    }
    #[must_use]
    pub fn run_id_ref(&self) -> Option<&str> {
        self.run_id.as_deref()
    }
    #[must_use]
    pub fn strict_mode(&self) -> bool {
        self.strict
    }
}

#[derive(Debug, Clone, PartialEq)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

impl Display for ImportWarning {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}
