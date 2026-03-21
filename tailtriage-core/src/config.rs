use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use crate::unix_time_ms;
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

/// Optional request-level options accepted by [`crate::Tailtriage::request_with`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RequestOptions {
    pub request_id: Option<String>,
}

impl RequestOptions {
    /// Creates default request options.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets a caller-provided request identifier.
    #[must_use]
    pub fn request_id(mut self, request_id: impl Into<String>) -> Self {
        self.request_id = Some(request_id.into());
        self
    }
}

/// Runtime sampling configuration stored in [`crate::Tailtriage`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SamplingConfig {
    pub(crate) runtime_interval: Option<Duration>,
}

impl SamplingConfig {
    /// Disables runtime sampling.
    #[must_use]
    pub fn disabled() -> Self {
        Self {
            runtime_interval: None,
        }
    }

    /// Enables runtime sampling with `interval`.
    #[must_use]
    pub fn runtime(interval: Duration) -> Self {
        Self {
            runtime_interval: Some(interval),
        }
    }

    /// Returns the configured runtime sampling interval.
    #[must_use]
    pub fn runtime_interval(&self) -> Option<Duration> {
        self.runtime_interval
    }
}

impl Default for SamplingConfig {
    fn default() -> Self {
        Self::disabled()
    }
}

/// Logical request outcome persisted in run artifacts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Ok,
    Error,
    Timeout,
    Cancelled,
    Rejected,
    Other(String),
}

impl Outcome {
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Ok => "ok",
            Self::Error => "error",
            Self::Timeout => "timeout",
            Self::Cancelled => "cancelled",
            Self::Rejected => "rejected",
            Self::Other(value) => value.as_str(),
        }
    }
}

impl Serialize for Outcome {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Outcome {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Ok(match value.as_str() {
            "ok" => Self::Ok,
            "error" => Self::Error,
            "timeout" => Self::Timeout,
            "cancelled" => Self::Cancelled,
            "rejected" => Self::Rejected,
            _ => Self::Other(value),
        })
    }
}

/// Errors emitted while building one tailtriage instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildError {
    EmptyServiceName,
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

pub(crate) fn default_output_path() -> PathBuf {
    PathBuf::from("tailtriage-run.json")
}

pub(crate) fn generate_request_id(route: &str) -> String {
    let route_prefix = route
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect::<String>();
    let sequence = REQUEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("{route_prefix}-{}-{sequence}", unix_time_ms())
}

static REQUEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);
