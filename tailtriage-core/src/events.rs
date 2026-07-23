use serde::{Deserialize, Serialize};

const fn default_completed() -> bool {
    true
}
#[allow(clippy::trivially_copy_pass_by_ref)]
const fn is_completed(value: &bool) -> bool {
    *value
}

use crate::{CaptureMode, EffectiveCoreConfig};

/// Current schema version for `Run` JSON artifacts.
pub const SCHEMA_VERSION: u64 = 2;

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
    /// Identifier for the run.
    ///
    /// When not supplied by the caller, `tailtriage-core` generates a UUID-based
    /// identifier.
    pub run_id: String,
    /// Service/application name.
    pub service_name: String,
    /// Optional service version.
    pub service_version: Option<String>,
    /// Timestamp (milliseconds since epoch UTC) when collection started.
    pub started_at_unix_ms: u64,
    /// Finalization timestamp (milliseconds since epoch UTC) for completed artifacts.
    ///
    /// This is `None` for active in-memory snapshots. Completed Run JSON
    /// artifacts use a numeric finalization timestamp.
    #[serde(default)]
    pub finalized_at_unix_ms: Option<u64>,
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
    /// Hostname captured at run creation when available as valid UTF-8.
    pub host: Option<String>,
    /// Process identifier if available.
    pub pid: Option<u32>,
    /// Lifecycle warnings generated during shutdown validation.
    #[serde(default)]
    pub lifecycle_warnings: Vec<String>,
    /// Incomplete request summary captured at shutdown.
    #[serde(default)]
    pub unfinished_requests: UnfinishedRequests,
    /// Why the run lifecycle ended.
    ///
    /// This field may be `None` for older artifacts and for runs that do not
    /// record an explicit end reason (including direct `tailtriage-core` runs today).
    #[serde(default)]
    pub run_end_reason: Option<RunEndReason>,
}

/// Run lifecycle end reason recorded in artifact metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunEndReason {
    /// Run ended because capture was disarmed manually.
    ManualDisarm,
    /// Run ended because process/controller shutdown finalized capture.
    Shutdown,
    /// Run auto-sealed after hitting capture limits.
    AutoSealOnLimitsHit,
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
///
/// Duration fields are authoritative for elapsed-time analysis; Unix-ms
/// timestamps are wall-clock anchors for correlation, readability, and coarse
/// grouping, and may be coarse or move with system clock changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestEvent {
    /// Per-run identity for one completed logical request/work item.
    ///
    /// This ID must be unique among completed requests in one [`Run`].
    /// External trace/correlation IDs that can repeat across retries, fanout
    /// branches, batch items, or attempts should be expanded with attempt,
    /// span, branch, or item information before becoming a tailtriage
    /// `request_id`. Users remain responsible for meaningful instrumentation
    /// and request-boundary semantics.
    pub request_id: String,
    /// Route name, operation, or endpoint.
    pub route: String,
    /// Semantic request kind.
    pub kind: Option<String>,
    /// Request start timestamp (milliseconds since epoch UTC).
    pub started_at_unix_ms: u64,
    /// Request start offset from run start, measured with a monotonic clock.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at_run_us: Option<u64>,
    /// Request completion timestamp (milliseconds since epoch UTC).
    pub finished_at_unix_ms: u64,
    /// Request completion offset from run start, measured with a monotonic clock.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at_run_us: Option<u64>,
    /// Total request latency in microseconds.
    pub latency_us: u64,
    /// Logical outcome such as "ok", "error", or "timeout".
    pub outcome: String,
}

/// Timing record for one named stage.
///
/// Duration fields are authoritative for elapsed-time analysis; Unix-ms
/// timestamps are wall-clock anchors for correlation, readability, and coarse
/// grouping, and may be coarse or move with system clock changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StageEvent {
    /// Parent tailtriage request ID for the same logical request/work item.
    ///
    /// This must match a completed [`RequestEvent::request_id`] and must not
    /// be reused for evidence from a different logical request.
    pub request_id: String,
    /// Stage identifier.
    pub stage: String,
    /// Stage start timestamp (milliseconds since epoch UTC).
    pub started_at_unix_ms: u64,
    /// Stage start offset from run start, measured with a monotonic clock.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at_run_us: Option<u64>,
    /// Stage completion timestamp (milliseconds since epoch UTC).
    pub finished_at_unix_ms: u64,
    /// Stage completion offset from run start, measured with a monotonic clock.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at_run_us: Option<u64>,
    /// Stage latency in microseconds.
    pub latency_us: u64,
    /// Whether the stage completed successfully (`Result::is_ok()` for
    /// `StageTimer::await_on`, always `true` for `StageTimer::await_value`).
    ///
    /// For partial events (`completed == false`), this is forced to `false` and
    /// is not an operation result. Consumers that need completion-aware
    /// interpretation must inspect [`StageEvent::completed`].
    pub success: bool,
    /// Whether the instrumented stage future completed normally.
    ///
    /// Older schema-v2 JSON without this field deserializes as completed.
    /// Completed events omit the field when serialized; partial events serialize
    /// `completed: false`.
    #[serde(default = "default_completed", skip_serializing_if = "is_completed")]
    pub completed: bool,
}

impl StageEvent {
    /// Creates a completed stage event with required identity, wall-clock
    /// interval, duration, and success fields.
    #[must_use]
    pub fn new(
        request_id: impl Into<String>,
        stage: impl Into<String>,
        started_at_unix_ms: u64,
        finished_at_unix_ms: u64,
        latency_us: u64,
        success: bool,
    ) -> Self {
        Self {
            request_id: request_id.into(),
            stage: stage.into(),
            started_at_unix_ms,
            started_at_run_us: None,
            finished_at_unix_ms,
            finished_at_run_us: None,
            latency_us,
            success,
            completed: true,
        }
    }

    /// Adds monotonic run-relative start and finish offsets.
    #[must_use]
    pub const fn with_run_interval(
        mut self,
        started_at_run_us: Option<u64>,
        finished_at_run_us: Option<u64>,
    ) -> Self {
        self.started_at_run_us = started_at_run_us;
        self.finished_at_run_us = finished_at_run_us;
        self
    }

    /// Marks this stage event as a partial observation and forces `success` to
    /// `false` because no completed operation result was observed.
    #[must_use]
    pub const fn into_partial(mut self) -> Self {
        self.completed = false;
        self.success = false;
        self
    }
}

/// Queue wait measurement for a request waiting on a queue/permit.
///
/// Duration fields are authoritative for elapsed-time analysis; Unix-ms
/// timestamps are wall-clock anchors for correlation, readability, and coarse
/// grouping, and may be coarse or move with system clock changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueEvent {
    /// Parent tailtriage request ID for the same logical request/work item.
    ///
    /// This must match a completed [`RequestEvent::request_id`] and must not
    /// be reused for evidence from a different logical request.
    pub request_id: String,
    /// Queue identifier.
    pub queue: String,
    /// Queue wait start timestamp (milliseconds since epoch UTC).
    pub waited_from_unix_ms: u64,
    /// Queue wait start offset from run start, measured with a monotonic clock.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waited_from_run_us: Option<u64>,
    /// Queue wait end timestamp (milliseconds since epoch UTC).
    pub waited_until_unix_ms: u64,
    /// Queue wait end offset from run start, measured with a monotonic clock.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waited_until_run_us: Option<u64>,
    /// Total wait time in microseconds.
    pub wait_us: u64,
    /// Queue depth sample captured at wait start, if known.
    pub depth_at_start: Option<u64>,
    /// Whether the instrumented queue future completed normally.
    ///
    /// Older schema-v2 JSON without this field deserializes as completed.
    /// Completed events omit the field when serialized; partial events serialize
    /// `completed: false`.
    #[serde(default = "default_completed", skip_serializing_if = "is_completed")]
    pub completed: bool,
}

impl QueueEvent {
    /// Creates a completed queue event with required identity, wall-clock
    /// interval, and duration fields.
    #[must_use]
    pub fn new(
        request_id: impl Into<String>,
        queue: impl Into<String>,
        waited_from_unix_ms: u64,
        waited_until_unix_ms: u64,
        wait_us: u64,
    ) -> Self {
        Self {
            request_id: request_id.into(),
            queue: queue.into(),
            waited_from_unix_ms,
            waited_from_run_us: None,
            waited_until_unix_ms,
            waited_until_run_us: None,
            wait_us,
            depth_at_start: None,
            completed: true,
        }
    }

    /// Adds monotonic run-relative wait start and finish offsets.
    #[must_use]
    pub const fn with_run_interval(
        mut self,
        waited_from_run_us: Option<u64>,
        waited_until_run_us: Option<u64>,
    ) -> Self {
        self.waited_from_run_us = waited_from_run_us;
        self.waited_until_run_us = waited_until_run_us;
        self
    }

    /// Adds the queue depth sample captured at wait start.
    #[must_use]
    pub const fn with_depth_at_start(mut self, depth_at_start: u64) -> Self {
        self.depth_at_start = Some(depth_at_start);
        self
    }

    /// Marks this queue event as a partial observation.
    #[must_use]
    pub const fn into_partial(mut self) -> Self {
        self.completed = false;
        self
    }
}

/// Point-in-time in-flight gauge reading.
///
/// Snapshot timestamps are Unix-ms wall-clock anchors for correlation,
/// readability, and coarse temporal grouping; runtime/in-flight attribution
/// based on those anchors can be approximate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InFlightSnapshot {
    /// Gauge name.
    pub gauge: String,
    /// Timestamp (milliseconds since epoch UTC).
    pub at_unix_ms: u64,
    /// Snapshot offset from run start, measured with a monotonic clock.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at_run_us: Option<u64>,
    /// Number of in-flight units.
    pub count: u64,
}

/// Point-in-time runtime metrics sample.
///
/// Snapshot timestamps are Unix-ms wall-clock anchors for correlation,
/// readability, and coarse temporal grouping; runtime/in-flight attribution
/// based on those anchors can be approximate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeSnapshot {
    /// Timestamp (milliseconds since epoch UTC).
    pub at_unix_ms: u64,
    /// Snapshot offset from run start, measured with a monotonic clock.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at_run_us: Option<u64>,
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
