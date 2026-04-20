use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;

use crate::{LocalJsonSink, RunSink};

/// Capture mode used during a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureMode {
    /// Lower-runtime-cost mode for core-only capture categories (see `docs/runtime-cost.md`).
    ///
    /// Core-owned defaults in this mode are:
    ///
    /// - `max_requests = 100_000`
    /// - `max_stages = 200_000`
    /// - `max_queues = 200_000`
    /// - `max_inflight_snapshots = 200_000`
    /// - `max_runtime_snapshots = 100_000`
    Light,
    /// Higher-retention mode for incident investigation.
    ///
    /// Core-owned defaults in this mode are:
    ///
    /// - `max_requests = 300_000`
    /// - `max_stages = 600_000`
    /// - `max_queues = 600_000`
    /// - `max_inflight_snapshots = 600_000`
    /// - `max_runtime_snapshots = 300_000`
    Investigation,
}

impl CaptureMode {
    /// Returns core-owned default capture limits for this mode.
    ///
    /// These mode defaults only affect retention limits in `tailtriage-core`.
    /// They do not change event types or request lifecycle semantics, and they
    /// do not auto-start Tokio runtime sampling.
    #[must_use]
    pub const fn core_defaults(self) -> CaptureLimits {
        match self {
            Self::Light => CaptureLimits {
                max_requests: 100_000,
                max_stages: 200_000,
                max_queues: 200_000,
                max_inflight_snapshots: 200_000,
                // Runtime snapshot defaults are carried in core artifacts for schema
                // consistency and are used by integration crates as needed.
                max_runtime_snapshots: 100_000,
            },
            Self::Investigation => CaptureLimits {
                max_requests: 300_000,
                max_stages: 600_000,
                max_queues: 600_000,
                max_inflight_snapshots: 600_000,
                max_runtime_snapshots: 300_000,
            },
        }
    }
}

/// Limits that bound in-memory capture growth for each run section.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
        CaptureMode::Light.core_defaults()
    }
}

/// Field-level capture limit overrides applied on top of mode defaults.
///
/// This additive API preserves [`TailtriageBuilder::capture_limits`] as a
/// full-override path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CaptureLimitsOverride {
    /// Optional override for [`CaptureLimits::max_requests`].
    pub max_requests: Option<usize>,
    /// Optional override for [`CaptureLimits::max_stages`].
    pub max_stages: Option<usize>,
    /// Optional override for [`CaptureLimits::max_queues`].
    pub max_queues: Option<usize>,
    /// Optional override for [`CaptureLimits::max_inflight_snapshots`].
    pub max_inflight_snapshots: Option<usize>,
    /// Optional override for [`CaptureLimits::max_runtime_snapshots`].
    pub max_runtime_snapshots: Option<usize>,
}

impl CaptureLimitsOverride {
    /// Applies this override to an existing limit set and returns the result.
    #[must_use]
    pub const fn apply(self, base: CaptureLimits) -> CaptureLimits {
        CaptureLimits {
            max_requests: match self.max_requests {
                Some(value) => value,
                None => base.max_requests,
            },
            max_stages: match self.max_stages {
                Some(value) => value,
                None => base.max_stages,
            },
            max_queues: match self.max_queues {
                Some(value) => value,
                None => base.max_queues,
            },
            max_inflight_snapshots: match self.max_inflight_snapshots {
                Some(value) => value,
                None => base.max_inflight_snapshots,
            },
            max_runtime_snapshots: match self.max_runtime_snapshots {
                Some(value) => value,
                None => base.max_runtime_snapshots,
            },
        }
    }

    const fn merge(self, newer: Self) -> Self {
        Self {
            max_requests: match newer.max_requests {
                Some(value) => Some(value),
                None => self.max_requests,
            },
            max_stages: match newer.max_stages {
                Some(value) => Some(value),
                None => self.max_stages,
            },
            max_queues: match newer.max_queues {
                Some(value) => Some(value),
                None => self.max_queues,
            },
            max_inflight_snapshots: match newer.max_inflight_snapshots {
                Some(value) => Some(value),
                None => self.max_inflight_snapshots,
            },
            max_runtime_snapshots: match newer.max_runtime_snapshots {
                Some(value) => Some(value),
                None => self.max_runtime_snapshots,
            },
        }
    }
}

/// Stable, resolved core configuration used by one capture run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectiveCoreConfig {
    /// Selected capture mode.
    pub mode: CaptureMode,
    /// Effective resolved retention limits used for this run.
    pub capture_limits: CaptureLimits,
    /// Effective strict lifecycle behavior for this run.
    pub strict_lifecycle: bool,
}

#[derive(Clone)]
pub(crate) struct Config {
    pub service_name: String,
    pub service_version: Option<String>,
    pub run_id: Option<String>,
    pub mode: CaptureMode,
    pub sink: Arc<dyn RunSink + Send + Sync>,
    pub effective_core: EffectiveCoreConfig,
    pub strict_lifecycle: bool,
}

impl Config {
    pub(crate) fn from_builder(builder: &TailtriageBuilder) -> Self {
        let mode_defaults = builder.mode.core_defaults();
        let effective_limits = match builder.capture_limits {
            Some(full_override) => full_override,
            None => builder.capture_limits_override.apply(mode_defaults),
        };
        let effective_core = EffectiveCoreConfig {
            mode: builder.mode,
            capture_limits: effective_limits,
            strict_lifecycle: builder.strict_lifecycle,
        };

        Self {
            service_name: builder.service_name.clone(),
            service_version: builder.service_version.clone(),
            run_id: builder.run_id.clone(),
            mode: builder.mode,
            sink: Arc::clone(&builder.sink),
            effective_core,
            strict_lifecycle: builder.strict_lifecycle,
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
    pub(crate) capture_limits: Option<CaptureLimits>,
    pub(crate) capture_limits_override: CaptureLimitsOverride,
    pub(crate) strict_lifecycle: bool,
}

impl TailtriageBuilder {
    pub(crate) fn new(service_name: impl Into<String>) -> Self {
        Self {
            service_name: service_name.into(),
            service_version: None,
            run_id: None,
            mode: CaptureMode::Light,
            sink: Arc::new(LocalJsonSink::new("tailtriage-run.json")),
            capture_limits: None,
            capture_limits_override: CaptureLimitsOverride::default(),
            strict_lifecycle: false,
        }
    }

    /// Sets capture mode to [`CaptureMode::Light`].
    ///
    /// Light mode is the default and favors lower runtime cost in core-only capture categories (see `docs/runtime-cost.md`) with enough signal for first-pass triage.
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
        self.capture_limits = Some(limits);
        self
    }

    /// Applies field-level capture limit overrides on top of mode defaults.
    ///
    /// This additive override path does not change full-override behavior from
    /// [`Self::capture_limits`]. If both are provided, `capture_limits(...)`
    /// remains authoritative.
    #[must_use]
    pub fn capture_limits_override(mut self, overrides: CaptureLimitsOverride) -> Self {
        self.capture_limits_override = self.capture_limits_override.merge(overrides);
        self
    }

    /// Enables strict lifecycle validation on shutdown.
    ///
    /// When enabled, [`crate::Tailtriage::shutdown`] returns an error if unfinished
    /// requests remain pending.
    #[must_use]
    pub fn strict_lifecycle(mut self, strict_lifecycle: bool) -> Self {
        self.strict_lifecycle = strict_lifecycle;
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

/// Optional request start settings used by [`crate::Tailtriage::begin_request_with`].
///
/// When `request_id` is not provided, a request ID is generated automatically.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RequestOptions {
    /// Optional caller-provided request ID used for request correlation.
    pub request_id: Option<String>,
    /// Optional semantic request kind (for example `http` or `job`).
    pub kind: Option<String>,
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

    /// Sets an optional semantic kind recorded on completion.
    #[must_use]
    pub fn kind(mut self, kind: impl Into<String>) -> Self {
        self.kind = Some(kind.into());
        self
    }
}
