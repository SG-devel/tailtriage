use std::path::PathBuf;

use serde::{Deserialize, Serialize};

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
    pub max_requests: usize,
    pub max_stages: usize,
    pub max_queues: usize,
    pub max_inflight_snapshots: usize,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Config {
    pub service_name: String,
    pub service_version: Option<String>,
    pub run_id: Option<String>,
    pub mode: CaptureMode,
    pub output_path: PathBuf,
    pub capture_limits: CaptureLimits,
}

impl Config {
    #[must_use]
    pub(crate) fn from_builder(builder: TailtriageBuilder) -> Self {
        Self {
            service_name: builder.service_name,
            service_version: builder.service_version,
            run_id: builder.run_id,
            mode: builder.mode,
            output_path: builder.output_path,
            capture_limits: builder.capture_limits,
        }
    }
}

/// Errors emitted while initializing tailtriage capture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InitError {
    /// Service name was empty.
    EmptyServiceName,
}

impl std::fmt::Display for InitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyServiceName => write!(f, "service_name cannot be empty"),
        }
    }
}

impl std::error::Error for InitError {}

/// Builder for creating one [`crate::Tailtriage`] capture run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TailtriageBuilder {
    pub(crate) service_name: String,
    pub(crate) service_version: Option<String>,
    pub(crate) run_id: Option<String>,
    pub(crate) mode: CaptureMode,
    pub(crate) output_path: PathBuf,
    pub(crate) capture_limits: CaptureLimits,
}

impl TailtriageBuilder {
    #[must_use]
    pub(crate) fn new(service_name: impl Into<String>) -> Self {
        Self {
            service_name: service_name.into(),
            service_version: None,
            run_id: None,
            mode: CaptureMode::Light,
            output_path: PathBuf::from("tailtriage-run.json"),
            capture_limits: CaptureLimits::default(),
        }
    }

    /// Configures low-overhead light mode.
    #[must_use]
    pub fn light(mut self) -> Self {
        self.mode = CaptureMode::Light;
        self
    }

    /// Configures higher-detail investigation mode.
    #[must_use]
    pub fn investigation(mut self) -> Self {
        self.mode = CaptureMode::Investigation;
        self
    }

    /// Sets an optional semantic service version.
    #[must_use]
    pub fn service_version(mut self, service_version: impl Into<String>) -> Self {
        self.service_version = Some(service_version.into());
        self
    }

    /// Sets a caller-provided run ID.
    #[must_use]
    pub fn run_id(mut self, run_id: impl Into<String>) -> Self {
        self.run_id = Some(run_id.into());
        self
    }

    /// Sets the output JSON path for this run.
    #[must_use]
    pub fn output(mut self, output_path: impl Into<PathBuf>) -> Self {
        self.output_path = output_path.into();
        self
    }

    /// Overrides bounded capture limits.
    #[must_use]
    pub fn capture_limits(mut self, capture_limits: CaptureLimits) -> Self {
        self.capture_limits = capture_limits;
        self
    }

    /// Builds a [`crate::Tailtriage`] instance.
    ///
    /// # Errors
    ///
    /// Returns [`InitError::EmptyServiceName`] when `service_name` is blank.
    pub fn build(self) -> Result<crate::Tailtriage, InitError> {
        crate::Tailtriage::from_config(Config::from_builder(self))
    }
}
