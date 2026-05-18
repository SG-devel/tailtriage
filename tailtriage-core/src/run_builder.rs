use crate::{
    unix_time_ms, BuildError, CaptureLimits, CaptureMode, EffectiveCoreConfig,
    EffectiveTokioSamplerConfig, InFlightSnapshot, QueueEvent, RequestEvent, Run, RunEndReason,
    RunMetadata, RuntimeSnapshot, StageEvent, UnfinishedRequests,
};

/// Options for assembling a completed [`Run`] artifact.
///
/// This API is intended for completed evidence assembly (for example import or
/// conversion pipelines), not live request instrumentation.
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
    /// Creates a new options set for one assembled run.
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
    /// Sets capture mode recorded in metadata.
    #[must_use]
    pub fn mode(mut self, mode: CaptureMode) -> Self {
        self.mode = Some(mode);
        self
    }
    /// Sets effective capture limits recorded in metadata.
    #[must_use]
    pub fn capture_limits(mut self, capture_limits: CaptureLimits) -> Self {
        self.capture_limits = Some(capture_limits);
        self
    }
    /// Sets strict lifecycle flag recorded in effective core config.
    #[must_use]
    pub fn strict_lifecycle(mut self, strict_lifecycle: bool) -> Self {
        self.strict_lifecycle = strict_lifecycle;
        self
    }
    /// Sets capture start timestamp in unix milliseconds.
    #[must_use]
    pub fn started_at_unix_ms(mut self, started_at_unix_ms: u64) -> Self {
        self.started_at_unix_ms = Some(started_at_unix_ms);
        self
    }
    /// Sets capture finish timestamp in unix milliseconds.
    #[must_use]
    pub fn finished_at_unix_ms(mut self, finished_at_unix_ms: u64) -> Self {
        self.finished_at_unix_ms = Some(finished_at_unix_ms);
        self
    }
    /// Sets run finalization timestamp in unix milliseconds.
    #[must_use]
    pub fn finalized_at_unix_ms(mut self, finalized_at_unix_ms: u64) -> Self {
        self.finalized_at_unix_ms = Some(finalized_at_unix_ms);
        self
    }
    /// Sets optional host metadata.
    #[must_use]
    pub fn host(mut self, host: impl Into<String>) -> Self {
        self.host = Some(host.into());
        self
    }
    /// Sets optional process id metadata.
    #[must_use]
    pub fn pid(mut self, pid: u32) -> Self {
        self.pid = Some(pid);
        self
    }
    /// Sets optional run end reason metadata.
    #[must_use]
    pub fn run_end_reason(mut self, run_end_reason: RunEndReason) -> Self {
        self.run_end_reason = Some(run_end_reason);
        self
    }
}

/// Builder for assembling a completed [`Run`] artifact.
///
/// Unlike [`crate::Tailtriage`], this type does not perform live request
/// lifecycle tracking; it only assembles finalized evidence.
#[derive(Debug, Clone)]
pub struct RunBuilder {
    run: Run,
}

impl RunBuilder {
    /// Creates a new completed-run builder from options.
    ///
    /// # Errors
    ///
    /// Returns [`BuildError::EmptyServiceName`] when `service_name` is blank.
    pub fn new(options: RunBuilderOptions) -> Result<Self, BuildError> {
        if options.service_name.trim().is_empty() {
            return Err(BuildError::EmptyServiceName);
        }

        let mode = options.mode.unwrap_or(CaptureMode::Light);
        let capture_limits = options.capture_limits.unwrap_or(mode.core_defaults());
        let now = unix_time_ms();

        Ok(Self {
            run: Run::new(RunMetadata {
                run_id: options.run_id.unwrap_or_else(generate_run_id),
                service_name: options.service_name,
                service_version: options.service_version,
                started_at_unix_ms: options.started_at_unix_ms.unwrap_or(now),
                finished_at_unix_ms: options.finished_at_unix_ms.unwrap_or(now),
                finalized_at_unix_ms: Some(options.finalized_at_unix_ms.unwrap_or(now)),
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
            }),
        })
    }

    /// Appends one request event.
    pub fn push_request(&mut self, event: RequestEvent) {
        self.run.requests.push(event);
    }
    /// Appends one stage event.
    pub fn push_stage(&mut self, event: StageEvent) {
        self.run.stages.push(event);
    }
    /// Appends one queue event.
    pub fn push_queue(&mut self, event: QueueEvent) {
        self.run.queues.push(event);
    }
    /// Appends one in-flight snapshot.
    pub fn push_inflight_snapshot(&mut self, snapshot: InFlightSnapshot) {
        self.run.inflight.push(snapshot);
    }
    /// Appends one runtime snapshot.
    pub fn push_runtime_snapshot(&mut self, snapshot: RuntimeSnapshot) {
        self.run.runtime_snapshots.push(snapshot);
    }
    /// Adds one lifecycle warning string.
    pub fn add_lifecycle_warning(&mut self, warning: impl Into<String>) {
        self.run.metadata.lifecycle_warnings.push(warning.into());
    }
    /// Replaces unfinished request summary metadata.
    pub fn set_unfinished_requests(&mut self, unfinished: UnfinishedRequests) {
        self.run.metadata.unfinished_requests = unfinished;
    }
    /// Sets effective Tokio sampler configuration metadata.
    pub fn set_effective_tokio_sampler_config(&mut self, config: EffectiveTokioSamplerConfig) {
        self.run.metadata.effective_tokio_sampler_config = Some(config);
    }
    /// Sets run end reason only when metadata does not already have one.
    pub fn set_run_end_reason_if_absent(&mut self, reason: RunEndReason) {
        if self.run.metadata.run_end_reason.is_none() {
            self.run.metadata.run_end_reason = Some(reason);
        }
    }
    /// Finishes assembly and returns the completed run artifact.
    #[must_use]
    pub fn finish(self) -> Run {
        self.run
    }
}

fn generate_run_id() -> String {
    format!("run-{}", uuid::Uuid::new_v4())
}
