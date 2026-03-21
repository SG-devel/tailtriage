use serde::{Deserialize, Serialize};

use crate::CaptureMode;

/// A full output artifact for one tailtriage capture run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Run {
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
        self.dropped_requests > 0
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
    /// Hostname if available.
    pub host: Option<String>,
    /// Process identifier if available.
    pub pid: Option<u32>,
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
