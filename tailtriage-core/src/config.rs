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

/// Configuration used to initialize one tailtriage capture run.
///
/// This is the main integration entry point for setup: create a config, tune
/// capture limits when needed, then call [`crate::Tailtriage::init`].
///
/// # Example
/// ```
/// use tailtriage_core::{Config, Tailtriage};
///
/// let mut config = Config::new("checkout-service");
/// config.service_version = Some("1.4.2".to_string());
/// config.output_path = std::env::temp_dir().join("tailtriage-run.json");
///
/// let tailtriage = Tailtriage::init(config)?;
/// assert_eq!(tailtriage.output_path().file_name().unwrap(), "tailtriage-run.json");
/// # Ok::<(), tailtriage_core::InitError>(())
/// ```
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
    /// Bounded capture limits for each event/sample section.
    pub capture_limits: CaptureLimits,
    /// Optional Tokio runtime sampling interval.
    pub runtime_sampling_interval: Option<Duration>,
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
            output_path: PathBuf::from("tailtriage-run.json"),
            capture_limits: CaptureLimits::default(),
            runtime_sampling_interval: None,
        }
    }
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

/// Runtime request metadata captured by [`crate::Tailtriage::request`].
///
/// Use [`RequestMeta::new`] when you already have a stable request ID from your
/// framework or gateway. Use [`RequestMeta::for_route`] when you need a local,
/// readable ID for light-touch instrumentation.
///
/// # Example
/// ```
/// use tailtriage_core::RequestMeta;
///
/// let meta = RequestMeta::for_route("/checkout").with_kind("http");
/// assert_eq!(meta.route, "/checkout");
/// assert_eq!(meta.kind.as_deref(), Some("http"));
/// ```
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

    /// Creates metadata with an auto-generated request ID for `route`.
    ///
    /// The generated ID keeps a readable route prefix and appends the current
    /// unix timestamp with a process-local sequence number.
    #[must_use]
    pub fn for_route(route: impl Into<String>) -> Self {
        let route = route.into();
        let route_prefix = route
            .chars()
            .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
            .collect::<String>();
        let sequence = REQUEST_META_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let request_id = format!("{route_prefix}-{}-{sequence}", unix_time_ms());

        Self {
            request_id,
            route,
            kind: None,
        }
    }

    /// Sets a semantic request kind for this request metadata.
    #[must_use]
    pub fn with_kind(mut self, kind: impl Into<String>) -> Self {
        self.kind = Some(kind.into());
        self
    }
}

/// Errors emitted while initializing tailtriage capture.
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

/// Backward-compatible alias for earlier API naming.
pub type InitError = BuildError;

static REQUEST_META_SEQUENCE: AtomicU64 = AtomicU64::new(0);
