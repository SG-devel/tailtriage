//! Core run schema and local JSON sink for tailscope.

use std::fs::File;
use std::io::{BufWriter, Error as IoError};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// A full output artifact for one tailscope capture run.
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
        }
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

/// Capture mode used during a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureMode {
    /// Low-overhead mode.
    Light,
    /// Higher-detail mode for incident investigation.
    Investigation,
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
    /// Whether the stage returned a successful result.
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
    /// Runtime blocking pool queue depth.
    pub blocking_queue_depth: Option<u64>,
    /// Runtime worker thread count.
    pub worker_threads: Option<u64>,
}

/// A sink that can persist a run artifact.
pub trait RunSink {
    /// Persists a run.
    fn write(&self, run: &Run) -> Result<(), SinkError>;
}

/// Local file sink that writes one JSON document per run.
#[derive(Debug, Clone)]
pub struct LocalJsonSink {
    path: PathBuf,
}

impl LocalJsonSink {
    /// Creates a local JSON sink for `path`.
    #[must_use]
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }

    /// Returns the target file path used by this sink.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl RunSink for LocalJsonSink {
    fn write(&self, run: &Run) -> Result<(), SinkError> {
        let file = File::create(&self.path).map_err(SinkError::Io)?;
        let mut writer = BufWriter::new(file);
        serde_json::to_writer_pretty(&mut writer, run).map_err(SinkError::Serialize)?;
        Ok(())
    }
}

/// Errors emitted while writing run artifacts.
#[derive(Debug)]
pub enum SinkError {
    /// Underlying I/O failure.
    Io(IoError),
    /// Serialization failure.
    Serialize(serde_json::Error),
}

impl std::fmt::Display for SinkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(err) => write!(f, "I/O error while writing run output: {err}"),
            Self::Serialize(err) => {
                write!(f, "serialization error while writing run output: {err}")
            }
        }
    }
}

impl std::error::Error for SinkError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            Self::Serialize(err) => Some(err),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{
        CaptureMode, InFlightSnapshot, LocalJsonSink, QueueEvent, RequestEvent, Run, RunMetadata,
        RunSink, RuntimeSnapshot, StageEvent,
    };

    fn sample_run() -> Run {
        let metadata = RunMetadata {
            run_id: "run_123".to_owned(),
            service_name: "payments".to_owned(),
            service_version: Some("1.2.3".to_owned()),
            started_at_unix_ms: 1_000,
            finished_at_unix_ms: 3_000,
            mode: CaptureMode::Light,
            host: Some("devbox".to_owned()),
            pid: Some(4242),
        };

        let mut run = Run::new(metadata);

        run.requests.push(RequestEvent {
            request_id: "req-1".to_owned(),
            route: "/invoice".to_owned(),
            kind: Some("create_invoice".to_owned()),
            started_at_unix_ms: 1_100,
            finished_at_unix_ms: 1_400,
            latency_us: 300_000,
            outcome: "ok".to_owned(),
        });

        run.stages.push(StageEvent {
            request_id: "req-1".to_owned(),
            stage: "persist_invoice".to_owned(),
            started_at_unix_ms: 1_220,
            finished_at_unix_ms: 1_350,
            latency_us: 130_000,
            success: true,
        });

        run.queues.push(QueueEvent {
            request_id: "req-1".to_owned(),
            queue: "invoice_worker".to_owned(),
            waited_from_unix_ms: 1_110,
            waited_until_unix_ms: 1_200,
            wait_us: 90_000,
            depth_at_start: Some(8),
        });

        run.inflight.push(InFlightSnapshot {
            gauge: "invoice_requests".to_owned(),
            at_unix_ms: 1_300,
            count: 12,
        });

        run.runtime_snapshots.push(RuntimeSnapshot {
            at_unix_ms: 1_350,
            alive_tasks: Some(240),
            global_queue_depth: Some(45),
            blocking_queue_depth: Some(6),
            worker_threads: Some(8),
        });

        run
    }

    #[test]
    fn run_serializes_all_mvp_sections() {
        let run = sample_run();
        let value = serde_json::to_value(&run).expect("run should serialize");

        assert!(value.get("metadata").is_some());
        assert!(value.get("requests").is_some());
        assert!(value.get("stages").is_some());
        assert!(value.get("queues").is_some());
        assert!(value.get("inflight").is_some());
        assert!(value.get("runtime_snapshots").is_some());
        assert_eq!(value["metadata"]["mode"], "light");
        assert_eq!(value["requests"][0]["route"], "/invoice");
    }

    #[test]
    fn local_json_sink_writes_valid_json_file() {
        let run = sample_run();
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after UNIX_EPOCH")
            .as_nanos();
        let output_path = std::env::temp_dir().join(format!("tailscope-run-{unique}.json"));

        let sink = LocalJsonSink::new(&output_path);
        sink.write(&run).expect("sink write should succeed");

        let saved = std::fs::read_to_string(&output_path).expect("json file should exist");
        let round_trip: Run = serde_json::from_str(&saved).expect("json should deserialize");

        assert_eq!(round_trip, run);

        std::fs::remove_file(&output_path).expect("temporary file should be removable");
    }
}
