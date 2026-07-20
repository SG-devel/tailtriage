use crate::{
    collector::generate_run_id, unix_time_ms, BuildError, CaptureLimits, CaptureMode,
    EffectiveCoreConfig, EffectiveTokioSamplerConfig, InFlightSnapshot, QueueEvent, RequestEvent,
    Run, RunEndReason, RunMetadata, RuntimeSnapshot, StageEvent, UnfinishedRequests,
};
use core::fmt;

/// Options for assembling a completed [`Run`] artifact.
///
/// This API is for completed evidence assembly (for example, import/conversion
/// paths), not live request instrumentation. Normal live instrumentation should
/// use [`crate::Tailtriage::builder`].
///
/// When omitted:
/// - mode defaults to [`CaptureMode::Light`]
/// - capture limits default to `mode.core_defaults()`
/// - host and pid remain `None`
/// - run id uses the same core run-id generator as live capture
/// - timestamps are filled from one captured current unix-ms value
///
/// `finalized_at_unix_ms` is always `Some(...)` for [`RunBuilder`] output.
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
    /// Creates options with a required service name.
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

    /// Sets optional service version metadata.
    #[must_use]
    pub fn service_version(mut self, service_version: impl Into<String>) -> Self {
        self.service_version = Some(service_version.into());
        self
    }
    /// Sets a caller-provided run identifier.
    #[must_use]
    pub fn run_id(mut self, run_id: impl Into<String>) -> Self {
        self.run_id = Some(run_id.into());
        self
    }
    /// Sets capture mode.
    #[must_use]
    pub fn mode(mut self, mode: CaptureMode) -> Self {
        self.mode = Some(mode);
        self
    }
    /// Sets effective capture limits for this completed run artifact.
    #[must_use]
    pub fn capture_limits(mut self, capture_limits: CaptureLimits) -> Self {
        self.capture_limits = Some(capture_limits);
        self
    }
    /// Sets strict lifecycle flag recorded in effective core config.
    #[must_use]
    pub const fn strict_lifecycle(mut self, strict_lifecycle: bool) -> Self {
        self.strict_lifecycle = strict_lifecycle;
        self
    }
    /// Sets start timestamp in unix milliseconds.
    #[must_use]
    pub const fn started_at_unix_ms(mut self, started_at_unix_ms: u64) -> Self {
        self.started_at_unix_ms = Some(started_at_unix_ms);
        self
    }
    /// Sets finish timestamp in unix milliseconds.
    #[must_use]
    pub const fn finished_at_unix_ms(mut self, finished_at_unix_ms: u64) -> Self {
        self.finished_at_unix_ms = Some(finished_at_unix_ms);
        self
    }
    /// Sets finalization timestamp in unix milliseconds.
    #[must_use]
    pub const fn finalized_at_unix_ms(mut self, finalized_at_unix_ms: u64) -> Self {
        self.finalized_at_unix_ms = Some(finalized_at_unix_ms);
        self
    }
    /// Sets optional host metadata.
    #[must_use]
    pub fn host(mut self, host: impl Into<String>) -> Self {
        self.host = Some(host.into());
        self
    }
    /// Sets optional process identifier metadata.
    #[must_use]
    pub const fn pid(mut self, pid: u32) -> Self {
        self.pid = Some(pid);
        self
    }
    /// Sets optional run-end reason metadata.
    #[must_use]
    pub const fn run_end_reason(mut self, run_end_reason: RunEndReason) -> Self {
        self.run_end_reason = Some(run_end_reason);
        self
    }
}

/// Validation error returned when a pushed event or snapshot has invalid shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunBuilderEventError {
    /// A field value on an event/snapshot failed validation.
    InvalidEvent {
        /// Event/snapshot type name.
        event: &'static str,
        /// Invalid field name.
        field: &'static str,
        /// Human-readable validation reason.
        reason: String,
    },
}

impl fmt::Display for RunBuilderEventError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidEvent {
                event,
                field,
                reason,
            } => {
                write!(f, "invalid {event}.{field}: {reason}")
            }
        }
    }
}

impl std::error::Error for RunBuilderEventError {}

/// Advanced/import API for completed-run artifact assembly.
///
/// [`RunBuilder`] assembles a finalized [`Run`] from already measured evidence.
/// It validates event shape and timestamp ordering, but does not perform live
/// request lifecycle tracking.
///
/// Elapsed duration fields such as request latency, stage latency, and queue
/// wait are accepted as authoritative completed evidence. [`RunBuilder`] does
/// not synthesize or repair durations from wall-clock timestamps.
///
/// Push methods use first-N retention through the same bounded
/// retention/truncation helper used by the live collector. Overflow items are
/// dropped and reflected in [`Run::truncation`].
#[derive(Debug)]
pub struct RunBuilder {
    run: Run,
    capture_limits: CaptureLimits,
}

impl RunBuilder {
    /// Creates a new completed-run builder from [`RunBuilderOptions`].
    ///
    /// # Errors
    ///
    /// Returns [`BuildError::EmptyServiceName`] when the service name is blank.
    ///
    /// Returns [`BuildError::InvalidRunTimeBounds`] when finished timestamp is
    /// earlier than start timestamp.
    ///
    /// Returns [`BuildError::InvalidFinalizationTime`] when finalization
    /// timestamp is earlier than finished timestamp.
    pub fn new(options: RunBuilderOptions) -> Result<Self, BuildError> {
        if options.service_name.trim().is_empty() {
            return Err(BuildError::EmptyServiceName);
        }

        let mode = options.mode.unwrap_or(CaptureMode::Light);
        let capture_limits = options
            .capture_limits
            .unwrap_or_else(|| mode.core_defaults());
        let ts = unix_time_ms();
        let started_at_unix_ms = options.started_at_unix_ms.unwrap_or(ts);
        let finished_at_unix_ms = options.finished_at_unix_ms.unwrap_or(ts);
        let finalized_at_unix_ms_value =
            options.finalized_at_unix_ms.unwrap_or(finished_at_unix_ms);
        let finalized_at_unix_ms = Some(finalized_at_unix_ms_value);

        if finished_at_unix_ms < started_at_unix_ms {
            return Err(BuildError::InvalidRunTimeBounds {
                started_at_unix_ms,
                finished_at_unix_ms,
            });
        }

        if finalized_at_unix_ms_value < finished_at_unix_ms {
            return Err(BuildError::InvalidFinalizationTime {
                finished_at_unix_ms,
                finalized_at_unix_ms: finalized_at_unix_ms_value,
            });
        }

        Ok(Self {
            run: Run::new(RunMetadata {
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
            }),
            capture_limits,
        })
    }

    /// Appends a request event.
    ///
    /// The event is retained only while request capture-limit capacity remains;
    /// otherwise it is dropped and `truncation.dropped_requests` is updated.
    ///
    /// # Errors
    ///
    /// Returns [`RunBuilderEventError`] when the event has invalid shape.
    pub fn push_request(&mut self, event: RequestEvent) -> Result<(), RunBuilderEventError> {
        validate_request_event(&event)?;
        let _ = crate::retention::push_request_bounded(&mut self.run, self.capture_limits, event);
        Ok(())
    }
    /// Appends a stage event.
    ///
    /// The event is retained only while stage capture-limit capacity remains;
    /// otherwise it is dropped and `truncation.dropped_stages` is updated.
    ///
    /// # Errors
    ///
    /// Returns [`RunBuilderEventError`] when the event has invalid shape.
    pub fn push_stage(&mut self, event: StageEvent) -> Result<(), RunBuilderEventError> {
        validate_stage_event(&event)?;
        let _ = crate::retention::push_stage_bounded(&mut self.run, self.capture_limits, event);
        Ok(())
    }
    /// Appends a queue event.
    ///
    /// The event is retained only while queue capture-limit capacity remains;
    /// otherwise it is dropped and `truncation.dropped_queues` is updated.
    ///
    /// # Errors
    ///
    /// Returns [`RunBuilderEventError`] when the event has invalid shape.
    pub fn push_queue(&mut self, event: QueueEvent) -> Result<(), RunBuilderEventError> {
        validate_queue_event(&event)?;
        let _ = crate::retention::push_queue_bounded(&mut self.run, self.capture_limits, event);
        Ok(())
    }
    /// Appends an in-flight snapshot.
    ///
    /// The snapshot is retained only while in-flight snapshot capture-limit
    /// capacity remains; otherwise it is dropped and
    /// `truncation.dropped_inflight_snapshots` is updated.
    ///
    /// # Errors
    ///
    /// Returns [`RunBuilderEventError`] when the snapshot has invalid shape.
    pub fn push_inflight_snapshot(
        &mut self,
        snapshot: InFlightSnapshot,
    ) -> Result<(), RunBuilderEventError> {
        validate_inflight_snapshot(&snapshot)?;
        let _ = crate::retention::push_inflight_snapshot_bounded(
            &mut self.run,
            self.capture_limits,
            snapshot,
        );
        Ok(())
    }
    /// Appends a runtime snapshot.
    ///
    /// The snapshot is retained only while runtime snapshot capture-limit
    /// capacity remains; otherwise it is dropped and
    /// `truncation.dropped_runtime_snapshots` is updated.
    ///
    /// # Errors
    ///
    /// Currently returns `Ok(())` for all runtime snapshots. The `Result`
    /// keeps this API consistent with other [`RunBuilder`] push methods and
    /// leaves room for future runtime snapshot validation without changing
    /// the method shape.
    pub fn push_runtime_snapshot(
        &mut self,
        snapshot: RuntimeSnapshot,
    ) -> Result<(), RunBuilderEventError> {
        let _ = crate::retention::push_runtime_snapshot_bounded(
            &mut self.run,
            self.capture_limits,
            snapshot,
        );
        Ok(())
    }
    /// Adds one lifecycle warning string.
    pub fn add_lifecycle_warning(&mut self, warning: impl Into<String>) {
        self.run.metadata.lifecycle_warnings.push(warning.into());
    }
    /// Sets unfinished-request metadata.
    pub fn set_unfinished_requests(&mut self, unfinished: UnfinishedRequests) {
        self.run.metadata.unfinished_requests = unfinished;
    }
    /// Sets effective Tokio sampler configuration metadata.
    pub fn set_effective_tokio_sampler_config(&mut self, config: EffectiveTokioSamplerConfig) {
        self.run.metadata.effective_tokio_sampler_config = Some(config);
    }
    /// Sets run-end reason only when absent.
    pub fn set_run_end_reason_if_absent(&mut self, reason: RunEndReason) {
        if self.run.metadata.run_end_reason.is_none() {
            self.run.metadata.run_end_reason = Some(reason);
        }
    }
    /// Consumes the builder and returns the assembled finalized [`Run`].
    ///
    /// This does not perform lifecycle validation or synthesize missing
    /// completions.
    #[must_use]
    pub fn finish(self) -> Run {
        let normalized = crate::normalize_run_permissive(&self.run);
        let mut run = normalized.run;
        for warning in crate::summarize_run_validation_lifecycle(&normalized.report) {
            if !run.metadata.lifecycle_warnings.contains(&warning) {
                run.metadata.lifecycle_warnings.push(warning);
            }
        }
        run
    }
}

fn invalid_event(
    event: &'static str,
    field: &'static str,
    reason: impl Into<String>,
) -> RunBuilderEventError {
    RunBuilderEventError::InvalidEvent {
        event,
        field,
        reason: reason.into(),
    }
}

fn validate_request_event(event: &RequestEvent) -> Result<(), RunBuilderEventError> {
    crate::validation::validate_request_shape(event)
        .map_err(|(field, reason)| invalid_event("RequestEvent", field, reason))
}
fn validate_stage_event(event: &StageEvent) -> Result<(), RunBuilderEventError> {
    crate::validation::validate_stage_shape(event)
        .map_err(|(field, reason)| invalid_event("StageEvent", field, reason))
}
fn validate_queue_event(event: &QueueEvent) -> Result<(), RunBuilderEventError> {
    crate::validation::validate_queue_shape(event)
        .map_err(|(field, reason)| invalid_event("QueueEvent", field, reason))
}
fn validate_inflight_snapshot(snapshot: &InFlightSnapshot) -> Result<(), RunBuilderEventError> {
    crate::validation::validate_inflight_shape(snapshot)
        .map_err(|(field, reason)| invalid_event("InFlightSnapshot", field, reason))
}
