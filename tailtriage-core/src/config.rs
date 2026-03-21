use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

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

#[derive(Clone)]
pub(crate) struct Config {
    pub service_name: String,
    pub service_version: Option<String>,
    pub run_id: Option<String>,
    pub mode: CaptureMode,
    pub sink: Arc<dyn RunSink + Send + Sync>,
    pub capture_limits: CaptureLimits,
    pub sampling: SamplingConfig,
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
            sampling: builder.sampling,
        }
    }
}

/// Errors emitted while building one tailtriage capture instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildError {
    /// Service name was empty.
    EmptyServiceName,
    /// Runtime sampling interval was zero.
    InvalidRuntimeSamplingInterval,
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyServiceName => write!(f, "service_name cannot be empty"),
            Self::InvalidRuntimeSamplingInterval => {
                write!(f, "runtime sampling interval must be greater than zero")
            }
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
    pub(crate) sampling: SamplingConfig,
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
            sampling: SamplingConfig::disabled(),
        }
    }

    #[must_use]
    pub fn light(mut self) -> Self {
        self.mode = CaptureMode::Light;
        self
    }

    #[must_use]
    pub fn investigation(mut self) -> Self {
        self.mode = CaptureMode::Investigation;
        self
    }

    #[must_use]
    pub fn output(mut self, output_path: impl AsRef<Path>) -> Self {
        self.sink = Arc::new(LocalJsonSink::new(output_path));
        self
    }

    #[must_use]
    pub fn sink<S>(mut self, sink: S) -> Self
    where
        S: RunSink + Send + Sync + 'static,
    {
        self.sink = Arc::new(sink);
        self
    }

    #[must_use]
    pub fn service_version(mut self, service_version: impl Into<String>) -> Self {
        self.service_version = Some(service_version.into());
        self
    }

    #[must_use]
    pub fn run_id(mut self, run_id: impl Into<String>) -> Self {
        self.run_id = Some(run_id.into());
        self
    }

    #[must_use]
    pub fn capture_limits(mut self, limits: CaptureLimits) -> Self {
        self.capture_limits = limits;
        self
    }

    #[must_use]
    pub fn sampling(mut self, sampling: SamplingConfig) -> Self {
        self.sampling = sampling;
        self
    }

    /// Builds one [`crate::Tailtriage`] collector for the configured service.
    ///
    /// # Errors
    ///
    /// Returns [`BuildError::EmptyServiceName`] when the configured service name is blank.
    pub fn build(self) -> Result<crate::Tailtriage, BuildError> {
        if self
            .sampling
            .runtime_interval()
            .is_some_and(|interval| interval.is_zero())
        {
            return Err(BuildError::InvalidRuntimeSamplingInterval);
        }
        crate::Tailtriage::from_config(Config::from_builder(&self))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RequestOptions {
    pub request_id: Option<String>,
}

impl RequestOptions {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn request_id(mut self, request_id: impl Into<String>) -> Self {
        self.request_id = Some(request_id.into());
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SamplingConfig {
    runtime_interval: Option<Duration>,
}

impl SamplingConfig {
    #[must_use]
    pub const fn disabled() -> Self {
        Self {
            runtime_interval: None,
        }
    }

    #[must_use]
    pub const fn runtime(interval: Duration) -> Self {
        Self {
            runtime_interval: Some(interval),
        }
    }

    #[must_use]
    pub const fn runtime_interval(&self) -> Option<Duration> {
        self.runtime_interval
    }
}
