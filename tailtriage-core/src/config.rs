use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use crate::unix_time_ms;
use serde::{Deserialize, Serialize};

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
            Self::Other(value) => value.as_str(),
        }
    }

    #[must_use]
    pub fn into_string(self) -> String {
        match self {
            Self::Ok => "ok".to_owned(),
            Self::Error => "error".to_owned(),
            Self::Timeout => "timeout".to_owned(),
            Self::Cancelled => "cancelled".to_owned(),
            Self::Rejected => "rejected".to_owned(),
            Self::Other(value) => value,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SamplingConfig {
    runtime_interval: Option<Duration>,
}

impl SamplingConfig {
    #[must_use]
    pub fn disabled() -> Self {
        Self {
            runtime_interval: None,
        }
    }

    #[must_use]
    pub fn runtime(interval: Duration) -> Self {
        Self {
            runtime_interval: Some(interval),
        }
    }

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

pub(crate) fn generate_request_id(route: &str) -> String {
    let route_prefix = route
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect::<String>();
    let sequence = REQUEST_META_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("{route_prefix}-{}-{sequence}", unix_time_ms())
}

static REQUEST_META_SEQUENCE: AtomicU64 = AtomicU64::new(0);
