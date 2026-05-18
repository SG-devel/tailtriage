use crate::{
    unix_time_ms, BuildError, CaptureLimits, CaptureMode, EffectiveCoreConfig,
    EffectiveTokioSamplerConfig, InFlightSnapshot, QueueEvent, RequestEvent, Run, RunEndReason,
    RunMetadata, RuntimeSnapshot, StageEvent, UnfinishedRequests,
};

/// Options for assembling a completed [`Run`] artifact in `tailtriage-core`.
///
/// This API is intended for completed evidence assembly flows (for example,
/// import/conversion paths) rather than normal live request instrumentation.
#[derive(Debug, Clone)]
pub struct RunBuilderOptions {
    service_name: String,
    service_version: Option<String>,
    run_id: Option<String>,
    mode: Option<CaptureMode>,
    capture_limits: Option<CaptureLimits>,
    strict_lifecycle: bool,
    started_at_unix_ms: Option<u64>,
    finished_at_unix_ms: Option<u64>,
    finalized_at_unix_ms: Option<u64>,
    host: Option<String>,
    pid: Option<u32>,
    run_end_reason: Option<RunEndReason>,
}

impl RunBuilderOptions {
    /// Creates options for a completed run assembly with a required service name.
    #[must_use]
    pub fn new(service_name: impl Into<String>) -> Self {
        Self {
            service_name: service_name.into(),
            service_version: None,
            run_id: None,
            mode: None,
            capture_limits: None,
            strict_lifecycle: false,
            started_at_unix_ms: None,
            finished_at_unix_ms: None,
            finalized_at_unix_ms: None,
            host: None,
            pid: None,
            run_end_reason: None,
        }
    }

    /// Sets an optional service version.
    #[must_use]
    pub fn service_version(mut self, service_version: impl Into<String>) -> Self {
        self.service_version = Some(service_version.into());
        self
    }
    /// Sets an explicit run identifier.
    #[must_use]
    pub fn run_id(mut self, run_id: impl Into<String>) -> Self {
        self.run_id = Some(run_id.into());
        self
    }
    /// Sets the capture mode used for effective core configuration.
    #[must_use]
    pub fn mode(mut self, mode: CaptureMode) -> Self {
        self.mode = Some(mode);
        self
    }
    /// Sets explicit effective capture limits.
    #[must_use]
    pub fn capture_limits(mut self, capture_limits: CaptureLimits) -> Self {
        self.capture_limits = Some(capture_limits);
        self
    }
    /// Sets strict lifecycle value recorded in effective core configuration.
    #[must_use]
    pub fn strict_lifecycle(mut self, strict_lifecycle: bool) -> Self {
        self.strict_lifecycle = strict_lifecycle;
        self
    }
    /// Sets explicit start timestamp in unix milliseconds.
    #[must_use]
    pub fn started_at_unix_ms(mut self, started_at_unix_ms: u64) -> Self {
        self.started_at_unix_ms = Some(started_at_unix_ms);
        self
    }
    /// Sets explicit finish timestamp in unix milliseconds.
    #[must_use]
    pub fn finished_at_unix_ms(mut self, finished_at_unix_ms: u64) -> Self {
        self.finished_at_unix_ms = Some(finished_at_unix_ms);
        self
    }
    /// Sets explicit finalization timestamp in unix milliseconds.
    #[must_use]
    pub fn finalized_at_unix_ms(mut self, finalized_at_unix_ms: u64) -> Self {
        self.finalized_at_unix_ms = Some(finalized_at_unix_ms);
        self
    }
    /// Sets an optional hostname.
    #[must_use]
    pub fn host(mut self, host: impl Into<String>) -> Self {
        self.host = Some(host.into());
        self
    }
    /// Sets an optional process identifier.
    #[must_use]
    pub fn pid(mut self, pid: u32) -> Self {
        self.pid = Some(pid);
        self
    }
    /// Sets an explicit run end reason.
    #[must_use]
    pub fn run_end_reason(mut self, run_end_reason: RunEndReason) -> Self {
        self.run_end_reason = Some(run_end_reason);
        self
    }
}

/// Builder for assembling a completed [`Run`] value from already-captured evidence.
#[derive(Debug, Clone)]
pub struct RunBuilder {
    run: Run,
}

impl RunBuilder {
    /// Creates a completed-run builder from [`RunBuilderOptions`].
    ///
    /// # Errors
    ///
    /// Returns [`BuildError::EmptyServiceName`] when the configured service name is blank.
    pub fn new(options: RunBuilderOptions) -> Result<Self, BuildError> {
        if options.service_name.trim().is_empty() {
            return Err(BuildError::EmptyServiceName);
        }

        let mode = options.mode.unwrap_or(CaptureMode::Light);
        let now_ms = unix_time_ms();
        let started_at_unix_ms = options.started_at_unix_ms.unwrap_or(now_ms);
        let finished_at_unix_ms = options.finished_at_unix_ms.unwrap_or(now_ms);
        let finalized_at_unix_ms = Some(options.finalized_at_unix_ms.unwrap_or(now_ms));
        let capture_limits = options
            .capture_limits
            .unwrap_or_else(|| mode.core_defaults());

        let metadata = RunMetadata {
            run_id: options.run_id.unwrap_or_else(generate_run_id),
            service_name: options.service_name,
            service_version: options.service_version,
            started_at_unix_ms,
            finished_at_unix_ms,
            finalized_at_unix_ms,
            mode,
            effective_core_config: Some(EffectiveCoreConfig {
                mode,
                capture_limits,
                strict_lifecycle: options.strict_lifecycle,
            }),
            effective_tokio_sampler_config: None,
            host: options.host,
            pid: options.pid,
            lifecycle_warnings: Vec::new(),
            unfinished_requests: UnfinishedRequests::default(),
            run_end_reason: options.run_end_reason,
        };

        Ok(Self {
            run: Run::new(metadata),
        })
    }

    /// Appends a request event.
    pub fn push_request(&mut self, event: RequestEvent) {
        self.run.requests.push(event);
    }
    /// Appends a stage event.
    pub fn push_stage(&mut self, event: StageEvent) {
        self.run.stages.push(event);
    }
    /// Appends a queue event.
    pub fn push_queue(&mut self, event: QueueEvent) {
        self.run.queues.push(event);
    }
    /// Appends an in-flight snapshot.
    pub fn push_inflight_snapshot(&mut self, snapshot: InFlightSnapshot) {
        self.run.inflight.push(snapshot);
    }
    /// Appends a runtime snapshot.
    pub fn push_runtime_snapshot(&mut self, snapshot: RuntimeSnapshot) {
        self.run.runtime_snapshots.push(snapshot);
    }
    /// Adds a lifecycle warning string.
    pub fn add_lifecycle_warning(&mut self, warning: impl Into<String>) {
        self.run.metadata.lifecycle_warnings.push(warning.into());
    }
    /// Sets unfinished request summary metadata.
    pub fn set_unfinished_requests(&mut self, unfinished_requests: UnfinishedRequests) {
        self.run.metadata.unfinished_requests = unfinished_requests;
    }
    /// Sets effective Tokio runtime sampler metadata.
    pub fn set_effective_tokio_sampler_config(&mut self, config: EffectiveTokioSamplerConfig) {
        self.run.metadata.effective_tokio_sampler_config = Some(config);
    }
    /// Sets run end reason only when absent.
    pub fn set_run_end_reason_if_absent(&mut self, run_end_reason: RunEndReason) {
        if self.run.metadata.run_end_reason.is_none() {
            self.run.metadata.run_end_reason = Some(run_end_reason);
        }
    }
    /// Finishes assembly and returns a completed/finalized [`Run`].
    #[must_use]
    pub fn finish(self) -> Run {
        self.run
    }
}

fn generate_run_id() -> String {
    format!("run-{}", uuid::Uuid::new_v4())
}
