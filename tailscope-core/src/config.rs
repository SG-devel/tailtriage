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

/// Configuration used to initialize one tailscope capture run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Service/application name.
    pub service_name: String,
    /// Optional service version.
    pub service_version: Option<String>,
    /// Optional caller-provided run ID.
    pub run_id: Option<String>,
    /// Capture mode for this run.
    pub mode: CaptureMode,
    /// JSON artifact path for this run.
    pub output_path: PathBuf,
}

impl Config {
    /// Returns a baseline configuration for `service_name`.
    #[must_use]
    pub fn new(service_name: impl Into<String>) -> Self {
        Self {
            service_name: service_name.into(),
            service_version: None,
            run_id: None,
            mode: CaptureMode::Light,
            output_path: PathBuf::from("tailscope-run.json"),
        }
    }
}

/// Runtime request metadata captured by [`crate::Tailscope::request`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestMeta {
    /// Correlation ID for the request.
    pub request_id: String,
    /// Route name, operation, or endpoint.
    pub route: String,
    /// Optional semantic request kind.
    pub kind: Option<String>,
}

impl RequestMeta {
    /// Creates metadata for a request scope.
    #[must_use]
    pub fn new(request_id: impl Into<String>, route: impl Into<String>) -> Self {
        Self {
            request_id: request_id.into(),
            route: route.into(),
            kind: None,
        }
    }
}

/// Errors emitted while initializing tailscope capture.
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
