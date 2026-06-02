use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Semantic span kind used by tailtriage tracing intake.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum SpanKind {
    /// Request-level span.
    Request,
    /// Stage-level span.
    Stage,
    /// Queue-level span.
    Queue,
}

impl SpanKind {
    /// Parses a `tt.kind` field value into a semantic span kind.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "request" => Some(Self::Request),
            "stage" => Some(Self::Stage),
            "queue" => Some(Self::Queue),
            _ => None,
        }
    }
}

/// Supported scalar field values on imported spans.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
#[non_exhaustive]
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

/// A tracing-shaped finished span record ready for intake conversion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpanRecord {
    id: Option<String>,
    parent_id: Option<String>,
    name: String,
    fields: BTreeMap<String, FieldValue>,
    started_at_unix_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    started_at_run_us: Option<u64>,
    finished_at_unix_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    finished_at_run_us: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    duration_us: Option<u64>,
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
            started_at_run_us: None,
            finished_at_unix_ms,
            finished_at_run_us: None,
            duration_us: None,
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
    pub fn field(mut self, key: impl Into<String>, value: impl Into<FieldValue>) -> Self {
        self.fields.insert(key.into(), value.into());
        self
    }
    /// Sets the span start offset from recorder/run start in microseconds.
    #[must_use]
    pub fn started_at_run_us(mut self, started_at_run_us: u64) -> Self {
        self.started_at_run_us = Some(started_at_run_us);
        self
    }

    /// Sets the span finish offset from recorder/run start in microseconds.
    #[must_use]
    pub fn finished_at_run_us(mut self, finished_at_run_us: u64) -> Self {
        self.finished_at_run_us = Some(finished_at_run_us);
        self
    }

    /// Sets explicit span duration in microseconds.
    #[must_use]
    pub fn duration_us(mut self, duration_us: u64) -> Self {
        self.duration_us = Some(duration_us);
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
    /// Returns start offset from recorder/run start in microseconds when present.
    #[must_use]
    pub fn started_at_run_us_ref(&self) -> Option<u64> {
        self.started_at_run_us
    }
    /// Returns finish timestamp in unix milliseconds.
    #[must_use]
    pub fn finished_at_unix_ms(&self) -> u64 {
        self.finished_at_unix_ms
    }
    /// Returns finish offset from recorder/run start in microseconds when present.
    #[must_use]
    pub fn finished_at_run_us_ref(&self) -> Option<u64> {
        self.finished_at_run_us
    }
    /// Returns explicit span duration in microseconds when present.
    #[must_use]
    pub fn duration_us_ref(&self) -> Option<u64> {
        self.duration_us
    }
}

/// Import options for converting tracing-shaped spans into a run.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ImportOptions {
    service_name: String,
    service_version: Option<String>,
    run_id: Option<String>,
    strict: bool,
    mode: tailtriage_core::CaptureMode,
    capture_limits: Option<tailtriage_core::CaptureLimits>,
    capture_limits_override: tailtriage_core::CaptureLimitsOverride,
}

impl ImportOptions {
    /// Creates options with required service name.
    pub fn new(service_name: impl Into<String>) -> Self {
        Self {
            service_name: service_name.into(),
            service_version: None,
            run_id: None,
            strict: false,
            mode: tailtriage_core::CaptureMode::Light,
            capture_limits: None,
            capture_limits_override: tailtriage_core::CaptureLimitsOverride::default(),
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

    /// Sets capture mode for import conversion semantics.
    #[must_use]
    pub fn mode(mut self, mode: tailtriage_core::CaptureMode) -> Self {
        self.mode = mode;
        self
    }

    /// Sets full capture limits override for import conversion semantics.
    #[must_use]
    pub fn capture_limits(mut self, capture_limits: tailtriage_core::CaptureLimits) -> Self {
        self.capture_limits = Some(capture_limits);
        self
    }

    /// Sets additive capture limits override for import conversion semantics.
    #[must_use]
    pub fn capture_limits_override(
        mut self,
        capture_limits_override: tailtriage_core::CaptureLimitsOverride,
    ) -> Self {
        self.capture_limits_override = capture_limits_override;
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

    /// Returns effective capture limits for import conversion semantics.
    #[must_use]
    pub fn resolved_capture_limits(&self) -> tailtriage_core::CaptureLimits {
        self.capture_limits.unwrap_or_else(|| {
            self.capture_limits_override
                .apply(self.mode.core_defaults())
        })
    }

    /// Returns capture mode setting.
    #[must_use]
    pub fn mode_value(&self) -> tailtriage_core::CaptureMode {
        self.mode
    }
}

/// Non-fatal warning produced while importing tracing-shaped spans.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
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
#[non_exhaustive]
pub struct ImportedRun {
    run: tailtriage_core::Run,
    warnings: Vec<ImportWarning>,
}

impl ImportedRun {
    /// Creates imported output from a converted run and non-fatal warnings.
    #[must_use]
    pub fn new(run: tailtriage_core::Run, warnings: Vec<ImportWarning>) -> Self {
        Self { run, warnings }
    }

    /// Returns converted run artifact.
    #[must_use]
    pub fn run(&self) -> &tailtriage_core::Run {
        &self.run
    }

    /// Returns non-fatal warnings emitted during conversion.
    #[must_use]
    pub fn warnings(&self) -> &[ImportWarning] {
        &self.warnings
    }

    /// Splits into converted run artifact and warnings.
    #[must_use]
    pub fn into_parts(self) -> (tailtriage_core::Run, Vec<ImportWarning>) {
        (self.run, self.warnings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TT_DEPTH_AT_START, TT_KIND, TT_SUCCESS};

    #[test]
    fn span_record_builder_stores_fields() {
        let record = SpanRecord::new("request", 10, 20)
            .id("span-1")
            .parent_id("parent-1")
            .field("tt.request_id", "req-1")
            .field(TT_KIND, "request")
            .field(TT_SUCCESS, true)
            .field(TT_DEPTH_AT_START, 7_u64);

        assert_eq!(record.id_ref(), Some("span-1"));
        assert_eq!(record.parent_id_ref(), Some("parent-1"));
        assert_eq!(record.name(), "request");
        assert_eq!(record.started_at_unix_ms(), 10);
        assert_eq!(record.finished_at_unix_ms(), 20);
        assert_eq!(record.fields().len(), 4);
    }

    #[test]
    fn field_value_from_conversions_work() {
        assert_eq!(
            FieldValue::from("request"),
            FieldValue::String("request".into())
        );
        assert_eq!(
            FieldValue::from(String::from("checkout")),
            FieldValue::String("checkout".into())
        );
        assert_eq!(FieldValue::from(true), FieldValue::Bool(true));
        assert_eq!(FieldValue::from(42_u64), FieldValue::U64(42));
        assert_eq!(FieldValue::from(-7_i64), FieldValue::I64(-7));
        assert_eq!(FieldValue::from(3.5_f64), FieldValue::F64(3.5));
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

    #[test]
    fn imported_run_accessors_and_into_parts_work() {
        let metadata = tailtriage_core::RunMetadata {
            run_id: "run-123".to_owned(),
            service_name: "checkout-service".to_owned(),
            service_version: Some("1.2.3".to_owned()),
            started_at_unix_ms: 10,
            finished_at_unix_ms: 20,
            finalized_at_unix_ms: None,
            mode: tailtriage_core::CaptureMode::Light,
            effective_core_config: None,
            effective_tokio_sampler_config: None,
            host: None,
            pid: None,
            lifecycle_warnings: Vec::new(),
            unfinished_requests: tailtriage_core::UnfinishedRequests::default(),
            run_end_reason: None,
        };
        let run = tailtriage_core::Run::new(metadata);
        let warnings = vec![ImportWarning::new("missing optional field")];
        let imported = ImportedRun::new(run.clone(), warnings.clone());

        assert_eq!(imported.run(), &run);
        assert_eq!(imported.warnings(), warnings.as_slice());

        let (parts_run, parts_warnings) = imported.into_parts();
        assert_eq!(parts_run, run);
        assert_eq!(parts_warnings, warnings);
    }

    #[test]
    fn import_options_default_resolution_uses_light_core_defaults() {
        let options = ImportOptions::new("svc");
        assert_eq!(options.mode_value(), tailtriage_core::CaptureMode::Light);
        assert_eq!(
            options.resolved_capture_limits(),
            tailtriage_core::CaptureMode::Light.core_defaults()
        );
    }

    #[test]
    fn import_options_mode_investigation_updates_defaults() {
        let options = ImportOptions::new("svc").mode(tailtriage_core::CaptureMode::Investigation);
        assert_eq!(
            options.resolved_capture_limits(),
            tailtriage_core::CaptureMode::Investigation.core_defaults()
        );
    }

    #[test]
    fn import_options_full_capture_limits_override_wins() {
        let full = tailtriage_core::CaptureLimits {
            max_requests: 3,
            max_stages: 4,
            max_queues: 5,
            max_inflight_snapshots: 6,
            max_runtime_snapshots: 7,
        };
        let additive = tailtriage_core::CaptureLimitsOverride {
            max_requests: Some(999),
            max_stages: Some(999),
            max_queues: Some(999),
            ..tailtriage_core::CaptureLimitsOverride::default()
        };
        let options = ImportOptions::new("svc")
            .mode(tailtriage_core::CaptureMode::Investigation)
            .capture_limits(full)
            .capture_limits_override(additive);
        assert_eq!(options.resolved_capture_limits(), full);
    }

    #[test]
    fn import_options_additive_capture_limits_override_applies_to_mode_defaults() {
        let additive = tailtriage_core::CaptureLimitsOverride {
            max_requests: Some(11),
            max_stages: Some(22),
            max_queues: Some(33),
            ..tailtriage_core::CaptureLimitsOverride::default()
        };
        let options = ImportOptions::new("svc")
            .mode(tailtriage_core::CaptureMode::Investigation)
            .capture_limits_override(additive);
        let expected = tailtriage_core::CaptureLimitsOverride {
            max_requests: Some(11),
            max_stages: Some(22),
            max_queues: Some(33),
            ..tailtriage_core::CaptureLimitsOverride::default()
        }
        .apply(tailtriage_core::CaptureMode::Investigation.core_defaults());
        assert_eq!(options.resolved_capture_limits(), expected);
    }
}
