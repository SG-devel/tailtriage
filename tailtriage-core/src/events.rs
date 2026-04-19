use serde::{Deserialize, Serialize};

use crate::{CaptureMode, EffectiveCoreConfig};

/// Current schema version for `Run` JSON artifacts.
pub const SCHEMA_VERSION: u64 = 1;

/// Logical request outcome categories used by the public API.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Request completed successfully.
    Ok,
    /// Request completed with an error.
    Error,
    /// Request exceeded a timeout threshold.
    Timeout,
    /// Request was cancelled before completion.
    Cancelled,
    /// Request was rejected before normal execution.
    Rejected,
    /// Caller-provided custom outcome label.
    Other(String),
}

impl Outcome {
    /// Returns the canonical string label for this outcome.
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

    /// Converts this outcome into an owned string label.
    #[must_use]
    pub fn into_string(self) -> String {
        match self {
            Self::Ok => "ok".to_string(),
            Self::Error => "error".to_string(),
            Self::Timeout => "timeout".to_string(),
            Self::Cancelled => "cancelled".to_string(),
            Self::Rejected => "rejected".to_string(),
            Self::Other(value) => value,
        }
    }
}

/// A full output artifact for one tailtriage capture run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Run {
    /// Run artifact schema version.
    pub schema_version: u64,
    /// Metadata for the capture session.
    pub metadata: RunMetadata,
    /// Request timing events.
    pub requests: Vec<RequestEvent>,
    /// Stage timing events.
    pub stages: Vec<StageEvent>,
    /// Queue wait timing events.
    pub queues: Vec<QueueEvent>,
    /// In-flight gauge changes over time.
    pub inflight: Vec<InFlightSnapshot>,
    /// Tokio runtime metrics snapshots.
    pub runtime_snapshots: Vec<RuntimeSnapshot>,
    /// Capture truncation summary for bounded collection.
    #[serde(default)]
    pub truncation: TruncationSummary,
}

impl Run {
    /// Creates an empty run with the provided metadata.
    #[must_use]
    pub fn new(metadata: RunMetadata) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            metadata,
            requests: Vec::new(),
            stages: Vec::new(),
            queues: Vec::new(),
            inflight: Vec::new(),
            runtime_snapshots: Vec::new(),
            truncation: TruncationSummary::default(),
        }
    }
}

/// Per-section counters indicating dropped samples due to capture limits.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TruncationSummary {
    /// Whether any capture limit was reached during this run.
    #[serde(default)]
    pub limits_hit: bool,
    /// Number of request events dropped after `max_requests` was reached.
    pub dropped_requests: u64,
    /// Number of stage events dropped after `max_stages` was reached.
    pub dropped_stages: u64,
    /// Number of queue events dropped after `max_queues` was reached.
    pub dropped_queues: u64,
    /// Number of in-flight snapshots dropped after `max_inflight_snapshots` was reached.
    pub dropped_inflight_snapshots: u64,
    /// Number of runtime snapshots dropped after `max_runtime_snapshots` was reached.
    pub dropped_runtime_snapshots: u64,
}

impl TruncationSummary {
    /// Returns true when any capture section was truncated.
    #[must_use]
    pub const fn is_truncated(&self) -> bool {
        self.limits_hit
            || self.dropped_requests > 0
            || self.dropped_stages > 0
            || self.dropped_queues > 0
            || self.dropped_inflight_snapshots > 0
            || self.dropped_runtime_snapshots > 0
    }
}

/// Top-level metadata for one capture run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunMetadata {
    /// A unique identifier for the run.
    pub run_id: String,
    /// Service/application name.
    pub service_name: String,
    /// Optional service version.
    pub service_version: Option<String>,
    /// Timestamp (milliseconds since epoch UTC) when collection started.
    pub started_at_unix_ms: u64,
    /// Timestamp (milliseconds since epoch UTC) when collection ended.
    pub finished_at_unix_ms: u64,
    /// Capture mode, such as "light" or "investigation".
    pub mode: CaptureMode,
    /// Effective resolved core configuration after applying mode defaults and overrides.
    ///
    /// This field may be `None` for older artifacts that predate effective config capture.
    #[serde(default)]
    pub effective_core_config: Option<EffectiveCoreConfig>,
    /// Effective resolved Tokio runtime sampler configuration for this run.
    ///
    /// This field is set only when a Tokio sampler is configured and started.
    /// It may be `None` for runs without Tokio sampling and for older artifacts.
    #[serde(default)]
    pub effective_tokio_sampler_config: Option<EffectiveTokioSamplerConfig>,
    /// Hostname if available.
    pub host: Option<String>,
    /// Process identifier if available.
    pub pid: Option<u32>,
    /// Lifecycle warnings generated during shutdown validation.
    #[serde(default)]
    pub lifecycle_warnings: Vec<String>,
    /// Incomplete request summary captured at shutdown.
    #[serde(default)]
    pub unfinished_requests: UnfinishedRequests,
}

/// Stable, resolved Tokio runtime sampler configuration used by one run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectiveTokioSamplerConfig {
    /// Capture mode selected in `tailtriage-core` that Tokio can inherit from.
    pub inherited_mode: CaptureMode,
    /// Optional explicit Tokio-side mode override.
    pub explicit_mode_override: Option<CaptureMode>,
    /// Effective mode used to resolve Tokio sampler defaults.
    pub resolved_mode: CaptureMode,
    /// Effective runtime sampler cadence in milliseconds.
    pub resolved_sampler_cadence_ms: u64,
    /// Effective runtime snapshot retention used by Tokio sampler.
    pub resolved_runtime_snapshot_retention: usize,
}

/// Summary of unfinished requests detected at shutdown.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct UnfinishedRequests {
    /// Count of requests still pending when shutdown ran.
    pub count: u64,
    /// Small sample of unfinished requests for debugging.
    pub sample: Vec<UnfinishedRequestSample>,
}

/// One unfinished request sample captured for lifecycle warnings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnfinishedRequestSample {
    /// Correlation ID for the unfinished request.
    pub request_id: String,
    /// Route or operation name associated with the unfinished request.
    pub route: String,
}

/// Per-request timing and status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestEvent {
    /// Correlation ID for the request.
    pub request_id: String,
    /// Route name, operation, or endpoint.
    pub route: String,
    /// Semantic request kind.
    pub kind: Option<String>,
    /// Request start timestamp (milliseconds since epoch UTC).
    pub started_at_unix_ms: u64,
    /// Request completion timestamp (milliseconds since epoch UTC).
    pub finished_at_unix_ms: u64,
    /// Total request latency in microseconds.
    pub latency_us: u64,
    /// Logical outcome such as "ok", "error", or "timeout".
    pub outcome: String,
}

/// Timing record for one named stage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StageEvent {
    /// Parent request ID.
    pub request_id: String,
    /// Stage identifier.
    pub stage: String,
    /// Stage start timestamp (milliseconds since epoch UTC).
    pub started_at_unix_ms: u64,
    /// Stage completion timestamp (milliseconds since epoch UTC).
    pub finished_at_unix_ms: u64,
    /// Stage latency in microseconds.
    pub latency_us: u64,
    /// Whether the stage completed successfully (`Result::is_ok()` for
    /// `StageTimer::await_on`, always `true` for `StageTimer::await_value`).
    pub success: bool,
}

/// Queue wait measurement for a request waiting on a queue/permit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueEvent {
    /// Parent request ID.
    pub request_id: String,
    /// Queue identifier.
    pub queue: String,
    /// Queue wait start timestamp (milliseconds since epoch UTC).
    pub waited_from_unix_ms: u64,
    /// Queue wait end timestamp (milliseconds since epoch UTC).
    pub waited_until_unix_ms: u64,
    /// Total wait time in microseconds.
    pub wait_us: u64,
    /// Queue depth sample captured at wait start, if known.
    pub depth_at_start: Option<u64>,
}

/// Point-in-time in-flight gauge reading.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InFlightSnapshot {
    /// Gauge name.
    pub gauge: String,
    /// Timestamp (milliseconds since epoch UTC).
    pub at_unix_ms: u64,
    /// Number of in-flight units.
    pub count: u64,
}

/// Point-in-time runtime metrics sample.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeSnapshot {
    /// Timestamp (milliseconds since epoch UTC).
    pub at_unix_ms: u64,
    /// Number of alive tasks.
    pub alive_tasks: Option<u64>,
    /// Runtime global queue depth.
    pub global_queue_depth: Option<u64>,
    /// Aggregated runtime local queue depth across worker threads.
    pub local_queue_depth: Option<u64>,
    /// Runtime blocking pool queue depth.
    pub blocking_queue_depth: Option<u64>,
    /// Runtime remote schedule count.
    pub remote_schedule_count: Option<u64>,
}
