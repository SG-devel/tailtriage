use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use crate::unix_time_ms;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureMode {
    Light,
    Investigation,
}

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
            Self::Other(value) => value,
        }
    }
}

impl Serialize for Outcome {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Outcome {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        let outcome = match value.as_str() {
            "ok" => Self::Ok,
            "error" => Self::Error,
            "timeout" => Self::Timeout,
            "cancelled" => Self::Cancelled,
            "rejected" => Self::Rejected,
            _ => Self::Other(value),
        };
        Ok(outcome)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
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

impl Default for SamplingConfig {
    fn default() -> Self {
        Self::disabled()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildError {
    EmptyServiceName,
    InvalidRuntimeSamplingInterval,
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyServiceName => write!(f, "service name cannot be empty"),
            Self::InvalidRuntimeSamplingInterval => {
                write!(f, "runtime sampling interval must be greater than zero")
            }
        }
    }
}

impl std::error::Error for BuildError {}

#[derive(Debug, Clone)]
pub(crate) struct BuildConfig {
    pub service_name: String,
    pub service_version: Option<String>,
    pub run_id: Option<String>,
    pub mode: CaptureMode,
    pub output_path: PathBuf,
    pub capture_limits: CaptureLimits,
    pub sampling: SamplingConfig,
}

impl BuildConfig {
    pub(crate) fn new(service_name: impl Into<String>) -> Self {
        Self {
            service_name: service_name.into(),
            service_version: None,
            run_id: None,
            mode: CaptureMode::Light,
            output_path: PathBuf::from("tailtriage-run.json"),
            capture_limits: CaptureLimits::default(),
            sampling: SamplingConfig::disabled(),
        }
    }
}

pub(crate) fn generate_request_id(route: &str) -> String {
    let route_prefix = route
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect::<String>();
    let sequence = REQUEST_META_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("{route_prefix}-{}-{sequence}", unix_time_ms())
}

static REQUEST_META_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub service_name: String,
    pub service_version: Option<String>,
    pub run_id: Option<String>,
    pub mode: CaptureMode,
    pub output_path: PathBuf,
    pub capture_limits: CaptureLimits,
    pub sampling: SamplingConfig,
}

impl Config {
    #[must_use]
    pub fn new(service_name: impl Into<String>) -> Self {
        Self {
            service_name: service_name.into(),
            service_version: None,
            run_id: None,
            mode: CaptureMode::Light,
            output_path: PathBuf::from("tailtriage-run.json"),
            capture_limits: CaptureLimits::default(),
            sampling: SamplingConfig::disabled(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestMeta {
    pub request_id: String,
    pub route: String,
    pub kind: Option<String>,
}

impl RequestMeta {
    #[must_use]
    pub fn new(request_id: impl Into<String>, route: impl Into<String>) -> Self {
        Self {
            request_id: request_id.into(),
            route: route.into(),
            kind: None,
        }
    }

    #[must_use]
    pub fn for_route(route: impl Into<String>) -> Self {
        let route = route.into();
        Self {
            request_id: generate_request_id(route.as_str()),
            route,
            kind: None,
        }
    }

    #[must_use]
    pub fn with_kind(mut self, kind: impl Into<String>) -> Self {
        self.kind = Some(kind.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InitError {
    EmptyServiceName,
}

impl std::fmt::Display for InitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyServiceName => write!(f, "service name cannot be empty"),
        }
    }
}

impl std::error::Error for InitError {}
