use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;

use crate::{LocalJsonSink, RunSink};

/// Capture mode used during a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureMode {
    /// Low-overhead mode.
    Light,
    /// Higher-detail mode for incident investigation.
    Investigation,
}

/// Limits that bound in-memory capture growth for each run section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureLimits {
    /// Maximum number of request events retained in-memory for the run.
    pub max_requests: usize,
    /// Maximum number of stage events retained in-memory for the run.
    pub max_stages: usize,
    /// Maximum number of queue events retained in-memory for the run.
    pub max_queues: usize,
    /// Maximum number of in-flight snapshots retained in-memory for the run.
    pub max_inflight_snapshots: usize,
    /// Maximum number of runtime snapshots retained in-memory for the run.
    pub max_runtime_snapshots: usize,
}

impl Default for CaptureLimits {
    fn default() -> Self {
        Self {
            max_requests: 100_000,
            max_stages: 200_000,
            max_queues: 200_000,
            max_inflight_snapshots: 200_000,
            max_runtime_snapshots: 100_000,
        }
    }
}

#[derive(Clone)]
pub(crate) struct Config {
    pub service_name: String,
    pub service_version: Option<String>,
    pub run_id: Option<String>,
    pub mode: CaptureMode,
    pub sink: Arc<dyn RunSink + Send + Sync>,
    pub capture_limits: CaptureLimits,
}

impl Config {
    pub(crate) fn from_builder(builder: &TailtriageBuilder) -> Self {
        Self {
            service_name: builder.service_name.clone(),
            service_version: builder.service_version.clone(),
            run_id: builder.run_id.clone(),
            mode: builder.mode,
            sink: Arc::clone(&builder.sink),
            capture_limits: builder.capture_limits,
        }
    }
}

/// Errors emitted while building one tailtriage capture instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildError {
    /// Service name was empty.
    EmptyServiceName,
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyServiceName => write!(f, "service_name cannot be empty"),
        }
    }
}

impl std::error::Error for BuildError {}

/// Builder for constructing a [`crate::Tailtriage`] run.
#[derive(Clone)]
pub struct TailtriageBuilder {
    pub(crate) service_name: String,
    pub(crate) service_version: Option<String>,
    pub(crate) run_id: Option<String>,
    pub(crate) mode: CaptureMode,
    pub(crate) sink: Arc<dyn RunSink + Send + Sync>,
    pub(crate) capture_limits: CaptureLimits,
}

impl TailtriageBuilder {
    pub(crate) fn new(service_name: impl Into<String>) -> Self {
        Self {
            service_name: service_name.into(),
            service_version: None,
            run_id: None,
            mode: CaptureMode::Light,
            sink: Arc::new(LocalJsonSink::new("tailtriage-run.json")),
            capture_limits: CaptureLimits::default(),
        }
    }

    /// Sets capture mode to [`CaptureMode::Light`].
    ///
    /// Light mode is the default and favors low overhead with enough signal for
    /// first-pass triage.
    #[must_use]
    pub fn light(mut self) -> Self {
        self.mode = CaptureMode::Light;
        self
    }

    /// Sets capture mode to [`CaptureMode::Investigation`].
    ///
    /// Use this mode when you need more detailed evidence during an incident.
    #[must_use]
    pub fn investigation(mut self) -> Self {
        self.mode = CaptureMode::Investigation;
        self
    }

    /// Writes run output to a local JSON file sink at `output_path`.
    ///
    /// The default output path is `tailtriage-run.json`.
    #[must_use]
    pub fn output(mut self, output_path: impl AsRef<Path>) -> Self {
        self.sink = Arc::new(LocalJsonSink::new(output_path));
        self
    }

    /// Uses a custom run sink implementation.
    #[must_use]
    pub fn sink<S>(mut self, sink: S) -> Self
    where
        S: RunSink + Send + Sync + 'static,
    {
        self.sink = Arc::new(sink);
        self
    }

    /// Sets an optional service version recorded in run metadata.
    #[must_use]
    pub fn service_version(mut self, service_version: impl Into<String>) -> Self {
        self.service_version = Some(service_version.into());
        self
    }

    /// Sets an explicit run identifier for metadata.
    ///
    /// If not set, `tailtriage` generates a run ID automatically.
    #[must_use]
    pub fn run_id(mut self, run_id: impl Into<String>) -> Self {
        self.run_id = Some(run_id.into());
        self
    }

    /// Overrides default capture limits for bounded in-memory collection.
    #[must_use]
    pub fn capture_limits(mut self, limits: CaptureLimits) -> Self {
        self.capture_limits = limits;
        self
    }

    /// Builds one [`crate::Tailtriage`] collector for the configured service.
    ///
    /// # Errors
    ///
    /// Returns [`BuildError::EmptyServiceName`] when the configured service name is blank.
    pub fn build(self) -> Result<crate::Tailtriage, BuildError> {
        crate::Tailtriage::from_config(Config::from_builder(&self))
    }
}

/// Optional request start settings used by [`crate::Tailtriage::request_with`].
///
/// When `request_id` is not provided, a request ID is generated automatically.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RequestOptions {
    /// Optional caller-provided request ID used for request correlation.
    pub request_id: Option<String>,
}

impl RequestOptions {
    /// Creates default request options with autogenerated request IDs.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets an explicit request ID for the next request context.
    #[must_use]
    pub fn request_id(mut self, request_id: impl Into<String>) -> Self {
        self.request_id = Some(request_id.into());
        self
    }
}
