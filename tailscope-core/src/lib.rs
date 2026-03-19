//! Core run schema and local JSON sink for tailscope.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufWriter, Error as IoError};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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
    /// Aggregated runtime local queue depth across worker threads.
    pub local_queue_depth: Option<u64>,
    /// Runtime blocking pool queue depth.
    pub blocking_queue_depth: Option<u64>,
    /// Runtime remote schedule count.
    pub remote_schedule_count: Option<u64>,
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

/// Runtime request metadata captured by [`Tailscope::request`].
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

/// Per-run collector that records request events and writes the final artifact.
#[derive(Debug)]
pub struct Tailscope {
    run: Mutex<Run>,
    inflight_counts: Mutex<HashMap<String, u64>>,
    sink: LocalJsonSink,
}

impl Tailscope {
    /// Initializes tailscope collection for one service run.
    ///
    /// # Errors
    ///
    /// Returns [`InitError::EmptyServiceName`] if `config.service_name` is blank.
    pub fn init(config: Config) -> Result<Self, InitError> {
        if config.service_name.trim().is_empty() {
            return Err(InitError::EmptyServiceName);
        }

        let now = unix_time_ms();
        let run = Run::new(RunMetadata {
            run_id: config.run_id.unwrap_or_else(generate_run_id),
            service_name: config.service_name,
            service_version: config.service_version,
            started_at_unix_ms: now,
            finished_at_unix_ms: now,
            mode: config.mode,
            host: None,
            pid: Some(std::process::id()),
        });

        Ok(Self {
            run: Mutex::new(run),
            inflight_counts: Mutex::new(HashMap::new()),
            sink: LocalJsonSink::new(config.output_path),
        })
    }

    /// Times one request future and records its completion as a [`RequestEvent`].
    ///
    /// `outcome` should represent your application-level request result (for example:
    /// `"ok"`, `"error"`, or `"timeout"`).
    pub async fn request<Fut, T>(
        &self,
        meta: RequestMeta,
        outcome: impl Into<String>,
        fut: Fut,
    ) -> T
    where
        Fut: std::future::Future<Output = T>,
    {
        let started_at_unix_ms = unix_time_ms();
        let started = Instant::now();
        let value = fut.await;
        let finished_at_unix_ms = unix_time_ms();

        let event = RequestEvent {
            request_id: meta.request_id,
            route: meta.route,
            kind: meta.kind,
            started_at_unix_ms,
            finished_at_unix_ms,
            latency_us: duration_to_us(started.elapsed()),
            outcome: outcome.into(),
        };

        lock_run(&self.run).requests.push(event);

        value
    }

    /// Returns a clone of the current in-memory run state.
    #[must_use]
    pub fn snapshot(&self) -> Run {
        lock_run(&self.run).clone()
    }

    /// Writes the current run to the configured sink.
    ///
    /// # Errors
    ///
    /// Returns [`SinkError`] if writing or serialization fails.
    pub fn flush(&self) -> Result<(), SinkError> {
        let mut guard = lock_run(&self.run);
        guard.metadata.finished_at_unix_ms = unix_time_ms();
        self.sink.write(&guard)
    }

    /// Returns the output file path used by the configured sink.
    #[must_use]
    pub fn output_path(&self) -> &Path {
        self.sink.path()
    }

    /// Creates an in-flight guard for `gauge`.
    ///
    /// The counter is incremented on creation and decremented when the returned
    /// guard is dropped.
    #[must_use]
    pub fn inflight(&self, gauge: impl Into<String>) -> InflightGuard<'_> {
        let gauge = gauge.into();
        let count = {
            let mut counts = lock_map(&self.inflight_counts);
            let entry = counts.entry(gauge.clone()).or_insert(0);
            *entry += 1;
            *entry
        };

        lock_run(&self.run).inflight.push(InFlightSnapshot {
            gauge: gauge.clone(),
            at_unix_ms: unix_time_ms(),
            count,
        });

        InflightGuard {
            tailscope: self,
            gauge,
        }
    }

    /// Returns a stage timing wrapper for one awaited operation.
    #[must_use]
    pub fn stage(&self, request_id: impl Into<String>, stage: impl Into<String>) -> StageTimer<'_> {
        StageTimer {
            tailscope: self,
            request_id: request_id.into(),
            stage: stage.into(),
        }
    }

    /// Returns a queue timing wrapper for one awaited operation.
    #[must_use]
    pub fn queue(&self, request_id: impl Into<String>, queue: impl Into<String>) -> QueueTimer<'_> {
        QueueTimer {
            tailscope: self,
            request_id: request_id.into(),
            queue: queue.into(),
            depth_at_start: None,
        }
    }

    /// Records one Tokio runtime metrics sample.
    pub fn record_runtime_snapshot(&self, snapshot: RuntimeSnapshot) {
        lock_run(&self.run).runtime_snapshots.push(snapshot);
    }
}

fn lock_run(run: &Mutex<Run>) -> std::sync::MutexGuard<'_, Run> {
    match run.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn lock_map(map: &Mutex<HashMap<String, u64>>) -> std::sync::MutexGuard<'_, HashMap<String, u64>> {
    match map.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn duration_to_us(duration: Duration) -> u64 {
    duration.as_micros().try_into().unwrap_or(u64::MAX)
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before UNIX_EPOCH")
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn generate_run_id() -> String {
    format!("run-{}", unix_time_ms())
}

/// A sink that can persist a run artifact.
pub trait RunSink {
    /// Persists a run.
    ///
    /// # Errors
    ///
    /// Returns [`SinkError`] if the sink cannot write the run output, such as
    /// when file I/O fails or serialization cannot complete.
    fn write(&self, run: &Run) -> Result<(), SinkError>;
}

/// RAII guard tracking one in-flight unit for a named gauge.
#[derive(Debug)]
pub struct InflightGuard<'a> {
    tailscope: &'a Tailscope,
    gauge: String,
}

impl Drop for InflightGuard<'_> {
    fn drop(&mut self) {
        let count = {
            let mut counts = lock_map(&self.tailscope.inflight_counts);
            let entry = counts.entry(self.gauge.clone()).or_insert(0);
            if *entry > 0 {
                *entry -= 1;
            }
            *entry
        };

        lock_run(&self.tailscope.run)
            .inflight
            .push(InFlightSnapshot {
                gauge: self.gauge.clone(),
                at_unix_ms: unix_time_ms(),
                count,
            });
    }
}

/// Thin wrapper for recording stage latency around one await point.
#[derive(Debug)]
pub struct StageTimer<'a> {
    tailscope: &'a Tailscope,
    request_id: String,
    stage: String,
}

impl StageTimer<'_> {
    /// Awaits `fut`, records stage duration, and returns the original output.
    pub async fn await_on<Fut, T>(self, fut: Fut) -> T
    where
        Fut: std::future::Future<Output = T>,
    {
        let started_at_unix_ms = unix_time_ms();
        let started = Instant::now();
        let value = fut.await;
        let finished_at_unix_ms = unix_time_ms();

        lock_run(&self.tailscope.run).stages.push(StageEvent {
            request_id: self.request_id,
            stage: self.stage,
            started_at_unix_ms,
            finished_at_unix_ms,
            latency_us: duration_to_us(started.elapsed()),
            success: true,
        });

        value
    }
}

/// Thin wrapper for recording queue-wait latency around one await point.
#[derive(Debug)]
pub struct QueueTimer<'a> {
    tailscope: &'a Tailscope,
    request_id: String,
    queue: String,
    depth_at_start: Option<u64>,
}

impl QueueTimer<'_> {
    /// Sets the queue depth sample captured at wait start.
    #[must_use]
    pub fn with_depth_at_start(mut self, depth_at_start: u64) -> Self {
        self.depth_at_start = Some(depth_at_start);
        self
    }

    /// Awaits `fut`, records queue wait duration, and returns the original output.
    pub async fn await_on<Fut, T>(self, fut: Fut) -> T
    where
        Fut: std::future::Future<Output = T>,
    {
        let waited_from_unix_ms = unix_time_ms();
        let started = Instant::now();
        let value = fut.await;
        let waited_until_unix_ms = unix_time_ms();

        lock_run(&self.tailscope.run).queues.push(QueueEvent {
            request_id: self.request_id,
            queue: self.queue,
            waited_from_unix_ms,
            waited_until_unix_ms,
            wait_us: duration_to_us(started.elapsed()),
            depth_at_start: self.depth_at_start,
        });

        value
    }
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
    use std::future::ready;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{
        CaptureMode, Config, InFlightSnapshot, InitError, LocalJsonSink, QueueEvent, RequestEvent,
        RequestMeta, Run, RunMetadata, RunSink, RuntimeSnapshot, StageEvent, Tailscope,
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
            waited_from_unix_ms: 1_105,
            waited_until_unix_ms: 1_120,
            wait_us: 15_000,
            depth_at_start: Some(7),
        });

        run.inflight.push(InFlightSnapshot {
            gauge: "invoice_requests".to_owned(),
            at_unix_ms: 1_200,
            count: 42,
        });

        run.runtime_snapshots.push(RuntimeSnapshot {
            at_unix_ms: 1_250,
            alive_tasks: Some(130),
            global_queue_depth: Some(18),
            local_queue_depth: Some(12),
            blocking_queue_depth: Some(4),
            remote_schedule_count: Some(44),
        });

        run
    }

    #[test]
    fn run_round_trips_with_json() {
        let run = sample_run();

        let encoded = serde_json::to_string_pretty(&run).expect("run should serialize");
        let decoded: Run = serde_json::from_str(&encoded).expect("run should deserialize");

        assert_eq!(decoded, run);
    }

    #[test]
    fn local_json_sink_writes_pretty_json_file() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time before epoch")
            .as_nanos();

        let path = std::env::temp_dir().join(format!("tailscope_core_run_{nanos}.json"));
        let sink = LocalJsonSink::new(&path);

        let run = sample_run();
        sink.write(&run).expect("sink should write run JSON");

        let written = std::fs::read_to_string(&path).expect("written file should exist");
        assert!(
            written.contains("\n  \"metadata\": {\n"),
            "expected pretty JSON formatting"
        );

        let decoded: Run = serde_json::from_str(&written).expect("written JSON should parse");
        assert_eq!(decoded, run);

        std::fs::remove_file(path).expect("temp run file should be removable");
    }

    #[test]
    fn init_rejects_blank_service_name() {
        let mut config = Config::new("payments");
        config.service_name = "   ".to_owned();

        let err = Tailscope::init(config).expect_err("blank service_name should fail");
        assert_eq!(err, InitError::EmptyServiceName);
    }

    #[test]
    fn request_records_timing_and_outcome() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time before epoch")
            .as_nanos();

        let mut config = Config::new("payments");
        config.output_path =
            std::env::temp_dir().join(format!("tailscope_core_scope_{nanos}.json"));

        let tailscope = Tailscope::init(config).expect("init should succeed");
        let mut request = RequestMeta::new("req-42", "/invoice");
        request.kind = Some("create_invoice".to_owned());

        let result = futures_executor::block_on(tailscope.request(request, "ok", ready(7_u32)));
        assert_eq!(result, 7);

        let snapshot = tailscope.snapshot();
        assert_eq!(snapshot.requests.len(), 1);

        let event = &snapshot.requests[0];
        assert_eq!(event.request_id, "req-42");
        assert_eq!(event.route, "/invoice");
        assert_eq!(event.kind.as_deref(), Some("create_invoice"));
        assert_eq!(event.outcome, "ok");
        assert!(event.finished_at_unix_ms >= event.started_at_unix_ms);
    }

    #[test]
    fn flush_writes_current_snapshot() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time before epoch")
            .as_nanos();

        let output_path = std::env::temp_dir().join(format!("tailscope_core_flush_{nanos}.json"));
        let mut config = Config::new("payments");
        config.output_path = output_path.clone();

        let tailscope = Tailscope::init(config).expect("init should succeed");
        tailscope.flush().expect("flush should write run file");

        let bytes = std::fs::metadata(&output_path)
            .expect("flush output should exist")
            .len();
        assert!(bytes > 0);

        std::fs::remove_file(output_path).expect("temp run file should be removable");
    }

    #[test]
    fn inflight_guard_records_increment_and_decrement() {
        let mut config = Config::new("payments");
        config.output_path = std::env::temp_dir().join("tailscope_core_inflight_test.json");

        let tailscope = Tailscope::init(config).expect("init should succeed");

        {
            let _guard = tailscope.inflight("invoice_requests");
            let snapshot = tailscope.snapshot();
            assert_eq!(snapshot.inflight.len(), 1);
            assert_eq!(snapshot.inflight[0].gauge, "invoice_requests");
            assert_eq!(snapshot.inflight[0].count, 1);
        }

        let snapshot = tailscope.snapshot();
        assert_eq!(snapshot.inflight.len(), 2);
        assert_eq!(snapshot.inflight[1].gauge, "invoice_requests");
        assert_eq!(snapshot.inflight[1].count, 0);
    }

    #[test]
    fn stage_wrapper_records_stage_event() {
        let mut config = Config::new("payments");
        config.output_path = std::env::temp_dir().join("tailscope_core_stage_test.json");

        let tailscope = Tailscope::init(config).expect("init should succeed");

        let result = futures_executor::block_on(
            tailscope
                .stage("req-22", "fetch_customer")
                .await_on(ready(11_u32)),
        );
        assert_eq!(result, 11);

        let snapshot = tailscope.snapshot();
        assert_eq!(snapshot.stages.len(), 1);
        let event = &snapshot.stages[0];
        assert_eq!(event.request_id, "req-22");
        assert_eq!(event.stage, "fetch_customer");
        assert!(event.finished_at_unix_ms >= event.started_at_unix_ms);
    }

    #[test]
    fn queue_wrapper_records_wait_event() {
        let mut config = Config::new("payments");
        config.output_path = std::env::temp_dir().join("tailscope_core_queue_test.json");

        let tailscope = Tailscope::init(config).expect("init should succeed");

        let result = futures_executor::block_on(
            tailscope
                .queue("req-22", "invoice_worker")
                .with_depth_at_start(3)
                .await_on(ready(11_u32)),
        );
        assert_eq!(result, 11);

        let snapshot = tailscope.snapshot();
        assert_eq!(snapshot.queues.len(), 1);
        let event = &snapshot.queues[0];
        assert_eq!(event.request_id, "req-22");
        assert_eq!(event.queue, "invoice_worker");
        assert_eq!(event.depth_at_start, Some(3));
        assert!(event.waited_until_unix_ms >= event.waited_from_unix_ms);
    }
}
