#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

//! Long-lived capture control layer for repeated bounded tailtriage activations.
//!
//! Layering:
//!
//! - [`tailtriage_core`] remains the per-run collector and artifact model.
//! - `tailtriage-controller` provides control-layer scaffolding for live arm/disarm
//!   workflows that create fresh bounded runs on every activation.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use tailtriage_core::{
    BuildError, CaptureMode, OwnedRequestCompletion, OwnedRequestHandle, RequestOptions, Tailtriage,
};

/// Builder for a long-lived [`TailtriageController`].
#[derive(Debug, Clone)]
pub struct TailtriageControllerBuilder {
    service_name: String,
    config_path: Option<PathBuf>,
    initially_enabled: bool,
    sink_template: ControllerSinkTemplate,
    run_end_policy: RunEndPolicy,
}

impl TailtriageControllerBuilder {
    /// Creates a controller builder for one service.
    #[must_use]
    pub fn new(service_name: impl Into<String>) -> Self {
        Self {
            service_name: service_name.into(),
            config_path: None,
            initially_enabled: false,
            sink_template: ControllerSinkTemplate::LocalJson {
                output_path: PathBuf::from("tailtriage-run.json"),
            },
            run_end_policy: RunEndPolicy::Manual,
        }
    }

    /// Sets the optional config path used for reloadable controller config.
    #[must_use]
    pub fn config_path(mut self, config_path: impl AsRef<Path>) -> Self {
        self.config_path = Some(config_path.as_ref().to_path_buf());
        self
    }

    /// Sets whether the controller starts with an active generation.
    #[must_use]
    pub const fn initially_enabled(mut self, initially_enabled: bool) -> Self {
        self.initially_enabled = initially_enabled;
        self
    }

    /// Sets the output location template for future activation runs.
    #[must_use]
    pub fn output(mut self, output_path: impl AsRef<Path>) -> Self {
        self.sink_template = ControllerSinkTemplate::LocalJson {
            output_path: output_path.as_ref().to_path_buf(),
        };
        self
    }

    /// Sets a run-end policy template applied to future activations.
    #[must_use]
    pub const fn run_end_policy(mut self, run_end_policy: RunEndPolicy) -> Self {
        self.run_end_policy = run_end_policy;
        self
    }

    /// Builds the controller.
    ///
    /// # Errors
    ///
    /// Returns [`ControllerBuildError::EmptyServiceName`] when `service_name` is blank.
    pub fn build(self) -> Result<TailtriageController, ControllerBuildError> {
        if self.service_name.trim().is_empty() {
            return Err(ControllerBuildError::EmptyServiceName);
        }

        let template = TailtriageControllerTemplate {
            service_name: self.service_name,
            config_path: self.config_path,
            sink_template: self.sink_template,
            selected_mode: CaptureMode::Light,
            run_end_policy: self.run_end_policy,
        };

        let inner = Arc::new(ControllerInner {
            template: Mutex::new(template),
            lifecycle: Mutex::new(ControllerLifecycle::Disabled { next_generation: 1 }),
            inert_run: Arc::new(
                Tailtriage::builder("tailtriage-controller-disabled")
                    .output(std::env::temp_dir().join(format!(
                        "tailtriage-controller-disabled-{}-{}.json",
                        std::process::id(),
                        tailtriage_core::unix_time_ms()
                    )))
                    .build()
                    .map_err(ControllerBuildError::InertRunBuild)?,
            ),
        });
        inner.inert_run.set_capture_enabled(false);

        let controller = TailtriageController { inner };
        if self.initially_enabled {
            controller
                .enable()
                .map_err(ControllerBuildError::InitialEnable)?;
        }

        Ok(controller)
    }
}

/// Long-lived live-capture controller for arm/disarm workflows.
#[derive(Debug, Clone)]
pub struct TailtriageController {
    inner: Arc<ControllerInner>,
}

#[derive(Debug)]
struct ControllerInner {
    template: Mutex<TailtriageControllerTemplate>,
    lifecycle: Mutex<ControllerLifecycle>,
    inert_run: Arc<Tailtriage>,
}

#[derive(Debug)]
struct ActiveGenerationRuntime {
    state: ActiveGenerationState,
    artifact_path: PathBuf,
    run: Arc<Tailtriage>,
    accepting_new: AtomicBool,
    closing: AtomicBool,
    inflight_captured: AtomicU64,
    finalize_started: AtomicBool,
}

impl ActiveGenerationRuntime {
    fn snapshot(&self) -> ActiveGenerationState {
        ActiveGenerationState {
            generation_id: self.state.generation_id,
            started_at_unix_ms: self.state.started_at_unix_ms,
            artifact_path: self.artifact_path.clone(),
            accepting_new_admissions: self.accepting_new.load(Ordering::Relaxed),
            closing: self.closing.load(Ordering::Relaxed),
            inflight_captured_requests: self.inflight_captured.load(Ordering::Relaxed),
        }
    }
}

impl TailtriageController {
    /// Creates a builder for controller-level scaffolding.
    #[must_use]
    pub fn builder(service_name: impl Into<String>) -> TailtriageControllerBuilder {
        TailtriageControllerBuilder::new(service_name)
    }

    /// Returns a status snapshot of controller lifecycle and template state.
    ///
    /// # Panics
    ///
    /// Panics if controller internal mutexes are poisoned.
    #[must_use]
    pub fn status(&self) -> TailtriageControllerStatus {
        let template = self
            .inner
            .template
            .lock()
            .expect("controller template lock poisoned");
        let lifecycle = self
            .inner
            .lifecycle
            .lock()
            .expect("controller lifecycle lock poisoned");

        TailtriageControllerStatus {
            service_name: template.service_name.clone(),
            config_path: template.config_path.clone(),
            sink_template: template.sink_template.clone(),
            selected_mode: template.selected_mode,
            run_end_policy: template.run_end_policy,
            generation: lifecycle.snapshot(),
        }
    }

    /// Replaces the template used to create the next activation generation.
    ///
    /// # Panics
    ///
    /// Panics if the controller template mutex is poisoned.
    pub fn reload_template(&self, next_template: TailtriageControllerTemplate) {
        let mut template = self
            .inner
            .template
            .lock()
            .expect("controller template lock poisoned");
        *template = next_template;
    }

    /// Arms capture by creating a fresh active generation with a bounded run.
    ///
    /// # Errors
    ///
    /// Returns [`EnableError::AlreadyActive`] when another generation is already active,
    /// and [`EnableError::Build`] when the run cannot be constructed.
    ///
    /// # Panics
    ///
    /// Panics if controller internal mutexes are poisoned.
    pub fn enable(&self) -> Result<ActiveGenerationState, EnableError> {
        let template = self
            .inner
            .template
            .lock()
            .expect("controller template lock poisoned")
            .clone();

        let mut lifecycle = self
            .inner
            .lifecycle
            .lock()
            .expect("controller lifecycle lock poisoned");

        let next_generation = match *lifecycle {
            ControllerLifecycle::Disabled { next_generation } => next_generation,
            ControllerLifecycle::Active { ref active, .. } => {
                return Err(EnableError::AlreadyActive {
                    generation_id: active.state.generation_id,
                });
            }
        };

        let artifact_path = generated_artifact_path(&template.sink_template, next_generation);
        let run_id = format!("{}-generation-{next_generation}", template.service_name);

        let mut builder = Tailtriage::builder(template.service_name.clone())
            .run_id(run_id)
            .output(&artifact_path);

        builder = match template.selected_mode {
            CaptureMode::Light => builder.light(),
            CaptureMode::Investigation => builder.investigation(),
        };

        let run = Arc::new(builder.build().map_err(EnableError::Build)?);
        let runtime = Arc::new(ActiveGenerationRuntime {
            state: ActiveGenerationState {
                generation_id: next_generation,
                started_at_unix_ms: tailtriage_core::unix_time_ms(),
                artifact_path: artifact_path.clone(),
                accepting_new_admissions: true,
                closing: false,
                inflight_captured_requests: 0,
            },
            artifact_path,
            run,
            accepting_new: AtomicBool::new(true),
            closing: AtomicBool::new(false),
            inflight_captured: AtomicU64::new(0),
            finalize_started: AtomicBool::new(false),
        });

        *lifecycle = ControllerLifecycle::Active {
            active: Arc::clone(&runtime),
            next_generation: next_generation.saturating_add(1),
        };

        Ok(runtime.snapshot())
    }

    /// Disarms capture for the active generation.
    ///
    /// This stops new request admissions immediately. If no admitted captured requests
    /// remain in flight, disarm finalizes immediately. Otherwise the generation is marked
    /// closing and finalization happens after the admitted captured requests drain.
    ///
    /// # Errors
    ///
    /// Returns [`DisableError::Finalize`] when final artifact writing fails.
    ///
    /// # Panics
    ///
    /// Panics if controller internal mutexes are poisoned.
    pub fn disable(&self) -> Result<DisableOutcome, DisableError> {
        let (active, next_generation, generation_id) = {
            let lifecycle = self
                .inner
                .lifecycle
                .lock()
                .expect("controller lifecycle lock poisoned");

            let ControllerLifecycle::Active {
                ref active,
                next_generation,
            } = *lifecycle
            else {
                return Ok(DisableOutcome::AlreadyDisabled);
            };

            active.accepting_new.store(false, Ordering::Relaxed);
            active.closing.store(true, Ordering::Relaxed);

            if active.inflight_captured.load(Ordering::Relaxed) == 0 {
                (
                    Some(Arc::clone(active)),
                    Some(next_generation),
                    active.state.generation_id,
                )
            } else {
                return Ok(DisableOutcome::Closing {
                    generation_id: active.state.generation_id,
                    inflight_captured_requests: active.inflight_captured.load(Ordering::Relaxed),
                });
            }
        };

        self.finalize_active(
            &active.expect("checked above"),
            next_generation.expect("checked above"),
        )?;

        Ok(DisableOutcome::Finalized { generation_id })
    }

    /// Begins one request through the controller.
    ///
    /// When an active generation is still admitting requests, the returned tokens are
    /// bound to that generation.
    ///
    /// When controller capture is disabled (or an active generation is closing), this
    /// returns inert/no-op request tokens.
    ///
    /// # Panics
    ///
    /// Panics if controller lifecycle mutex is poisoned.
    pub fn begin_request_with(
        &self,
        route: impl Into<String>,
        options: RequestOptions,
    ) -> ControllerStartedRequest {
        let route = route.into();
        if let Some(started) = self.try_begin_request_with(route.clone(), options.clone()) {
            return started;
        }

        let started = self
            .inner
            .inert_run
            .begin_request_with_owned(route, options);
        ControllerStartedRequest {
            handle: started.handle,
            completion: ControllerRequestCompletion {
                completion: Some(started.completion),
                admission_generation_id: None,
                admitted_generation: Weak::new(),
                inner: Weak::new(),
                inflight_recorded: false,
            },
        }
    }

    /// Convenience helper using default request options.
    pub fn begin_request(&self, route: impl Into<String>) -> ControllerStartedRequest {
        self.begin_request_with(route, RequestOptions::new())
    }

    /// Tries to begin a captured request when an active generation is still admitting requests.
    ///
    /// The returned handle and completion are generation-bound at admission time.
    /// They remain attached to that admitted generation even if the controller is
    /// disabled and re-enabled before completion finishes.
    ///
    /// Returns `None` when controller is disabled or when active generation is closing.
    ///
    /// Prefer [`TailtriageController::begin_request_with`] for the primary non-branching API.
    ///
    /// # Panics
    ///
    /// Panics if controller lifecycle mutex is poisoned.
    #[must_use]
    pub fn try_begin_request_with(
        &self,
        route: impl Into<String>,
        options: RequestOptions,
    ) -> Option<ControllerStartedRequest> {
        let active = {
            let lifecycle = self
                .inner
                .lifecycle
                .lock()
                .expect("controller lifecycle lock poisoned");

            match *lifecycle {
                ControllerLifecycle::Active { ref active, .. } => Arc::clone(active),
                ControllerLifecycle::Disabled { .. } => return None,
            }
        };

        if !active.accepting_new.load(Ordering::Acquire) {
            return None;
        }

        active.inflight_captured.fetch_add(1, Ordering::AcqRel);
        if !active.accepting_new.load(Ordering::Acquire) {
            active.inflight_captured.fetch_sub(1, Ordering::AcqRel);
            return None;
        }

        // Admission is now committed to this concrete generation runtime.
        // The completion token keeps a weak reference to this runtime so finish
        // bookkeeping cannot drift into a later generation.
        let started = active.run.begin_request_with_owned(route, options);

        Some(ControllerStartedRequest {
            handle: started.handle,
            completion: ControllerRequestCompletion {
                completion: Some(started.completion),
                admission_generation_id: Some(active.state.generation_id),
                admitted_generation: Arc::downgrade(&active),
                inner: Arc::downgrade(&self.inner),
                inflight_recorded: true,
            },
        })
    }

    /// Compatibility helper using default request options.
    ///
    /// Prefer [`TailtriageController::begin_request`] for the primary non-branching API.
    #[must_use]
    pub fn try_begin_request(&self, route: impl Into<String>) -> Option<ControllerStartedRequest> {
        self.try_begin_request_with(route, RequestOptions::new())
    }

    /// Finalizes controller state for process shutdown.
    ///
    /// Shutdown makes lifecycle behavior explicit: it immediately stops new admissions and
    /// writes any active generation artifact, even if unfinished requests remain.
    /// That behavior matches [`tailtriage_core::Tailtriage::shutdown`].
    ///
    /// # Errors
    ///
    /// Returns [`ShutdownError::Finalize`] if artifact writing fails.
    ///
    /// # Panics
    ///
    /// Panics if controller lifecycle mutex is poisoned.
    pub fn shutdown(&self) -> Result<(), ShutdownError> {
        let maybe_active = {
            let lifecycle = self
                .inner
                .lifecycle
                .lock()
                .expect("controller lifecycle lock poisoned");
            match *lifecycle {
                ControllerLifecycle::Active { ref active, .. } => Some(Arc::clone(active)),
                ControllerLifecycle::Disabled { .. } => None,
            }
        };

        if let Some(active) = maybe_active {
            active.accepting_new.store(false, Ordering::Relaxed);
            active.closing.store(true, Ordering::Relaxed);
            self.force_finalize_generation(&active)
                .map_err(ShutdownError::Finalize)?;
        }

        Ok(())
    }

    fn force_finalize_generation(
        &self,
        active: &Arc<ActiveGenerationRuntime>,
    ) -> Result<(), DisableError> {
        let next_generation = {
            let lifecycle = self
                .inner
                .lifecycle
                .lock()
                .expect("controller lifecycle lock poisoned");
            match *lifecycle {
                ControllerLifecycle::Active {
                    active: ref current_active,
                    next_generation,
                } if current_active.state.generation_id == active.state.generation_id => {
                    next_generation
                }
                _ => return Ok(()),
            }
        };

        self.finalize_active(active, next_generation)
    }

    fn finalize_active(
        &self,
        active: &Arc<ActiveGenerationRuntime>,
        next_generation: u64,
    ) -> Result<(), DisableError> {
        if active.finalize_started.swap(true, Ordering::AcqRel) {
            return Ok(());
        }

        active.run.shutdown().map_err(DisableError::Finalize)?;

        let mut lifecycle = self
            .inner
            .lifecycle
            .lock()
            .expect("controller lifecycle lock poisoned");

        if matches!(
            *lifecycle,
            ControllerLifecycle::Active {
                active: ref current_active,
                next_generation: ng,
            } if current_active.state.generation_id == active.state.generation_id && ng == next_generation
        ) {
            *lifecycle = ControllerLifecycle::Disabled { next_generation };
        }

        Ok(())
    }
}

/// Result of trying to begin one captured request in a generation.
#[must_use = "request completion must be finished explicitly"]
#[derive(Debug)]
pub struct ControllerStartedRequest {
    /// Instrumentation handle for queue/stage/inflight timing.
    pub handle: OwnedRequestHandle,
    /// Completion token bound to one generation.
    pub completion: ControllerRequestCompletion,
}

/// Completion token for a request admitted through [`TailtriageController`].
#[must_use = "request completion must be finished explicitly"]
#[derive(Debug)]
pub struct ControllerRequestCompletion {
    completion: Option<OwnedRequestCompletion>,
    /// Generation captured at admission time.
    ///
    /// This binding is immutable for the life of the completion token so that
    /// request finalization cannot migrate to a later generation during rapid
    /// enable/disable/re-enable transitions.
    admission_generation_id: Option<u64>,
    /// Weak reference to the exact runtime generation that admitted the request.
    ///
    /// Keeping this pointer ensures inflight accounting and close/finalize checks
    /// operate on the admitted generation even if controller lifecycle has already
    /// advanced to a newer generation.
    admitted_generation: Weak<ActiveGenerationRuntime>,
    inner: Weak<ControllerInner>,
    inflight_recorded: bool,
}

impl ControllerRequestCompletion {
    /// Finishes this request with an explicit outcome.
    pub fn finish(mut self, outcome: tailtriage_core::Outcome) {
        if let Some(completion) = self.completion.take() {
            completion.finish(outcome);
            self.mark_finished();
        }
    }

    /// Convenience helper for successful completion.
    pub fn finish_ok(self) {
        self.finish(tailtriage_core::Outcome::Ok);
    }

    /// Finishes from `result` and returns `result` unchanged.
    ///
    /// # Errors
    ///
    /// This method does not create new errors. It returns `result` unchanged,
    /// including the original `Err(E)` value.
    pub fn finish_result<T, E>(mut self, result: Result<T, E>) -> Result<T, E> {
        if let Some(completion) = self.completion.take() {
            completion.finish(if result.is_ok() {
                tailtriage_core::Outcome::Ok
            } else {
                tailtriage_core::Outcome::Error
            });
            self.mark_finished();
        }
        result
    }

    fn mark_finished(&mut self) {
        if !self.inflight_recorded {
            return;
        }

        self.inflight_recorded = false;

        let Some(active) = self.admitted_generation.upgrade() else {
            return;
        };

        if let Some(admission_generation_id) = self.admission_generation_id {
            debug_assert_eq!(
                active.state.generation_id, admission_generation_id,
                "controller completion generation binding should remain stable"
            );
        }

        let remaining = active
            .inflight_captured
            .fetch_sub(1, Ordering::AcqRel)
            .saturating_sub(1);

        if remaining == 0 && active.closing.load(Ordering::Acquire) {
            self.try_finalize_bound_generation(&active);
        }
    }

    fn try_finalize_bound_generation(&self, active: &Arc<ActiveGenerationRuntime>) {
        let Some(inner) = self.inner.upgrade() else {
            return;
        };

        if active.finalize_started.swap(true, Ordering::AcqRel) {
            return;
        }

        if active.run.shutdown().is_err() {
            return;
        }

        let mut lifecycle = inner
            .lifecycle
            .lock()
            .expect("controller lifecycle lock poisoned");

        if let ControllerLifecycle::Active {
            active: ref current_active,
            next_generation,
        } = *lifecycle
        {
            if current_active.state.generation_id == active.state.generation_id {
                *lifecycle = ControllerLifecycle::Disabled { next_generation };
            }
        }
    }
}

/// Template configuration that the controller applies to future activations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TailtriageControllerTemplate {
    /// Service name attached to controller activations.
    pub service_name: String,
    /// Optional source path for reloadable control config.
    pub config_path: Option<PathBuf>,
    /// Sink/output template for bounded run artifacts.
    pub sink_template: ControllerSinkTemplate,
    /// Mode selected for next activations.
    pub selected_mode: CaptureMode,
    /// Policy that determines how an activation run should end.
    pub run_end_policy: RunEndPolicy,
}

/// Sink/output template used by controller-generated runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControllerSinkTemplate {
    /// Write each generated run to a local JSON file.
    LocalJson {
        /// Base destination artifact path for generated runs.
        output_path: PathBuf,
    },
}

/// Policy for bounded activation run completion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunEndPolicy {
    /// End only when the caller disarms/stops capture.
    Manual,
    /// End after this many completed requests.
    MaxRequests {
        /// Maximum completed requests before ending a generation.
        max_requests: u64,
    },
    /// End after this wall-clock duration.
    MaxDuration {
        /// Maximum wall-clock duration before ending a generation.
        max_duration: Duration,
    },
    /// End when any capture section reaches limits.
    FirstLimitHit,
}

/// Public status snapshot for reporting controller state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TailtriageControllerStatus {
    /// Service name used for controller activations.
    pub service_name: String,
    /// Optional source path for reloadable control config.
    pub config_path: Option<PathBuf>,
    /// Sink/output template for generated runs.
    pub sink_template: ControllerSinkTemplate,
    /// Mode selected for next activations.
    pub selected_mode: CaptureMode,
    /// Run-end policy selected for next activations.
    pub run_end_policy: RunEndPolicy,
    /// Current generation state snapshot.
    pub generation: GenerationState,
}

/// Current generation state for a controller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GenerationState {
    /// Controller is disarmed and has no active generation.
    Disabled {
        /// Next generation ID that would be assigned on activation.
        next_generation: u64,
    },
    /// Controller is armed and waiting for next activation.
    EnabledIdle {
        /// Next generation ID that will be assigned on activation.
        next_generation: u64,
    },
    /// Controller currently owns one active generation.
    Active(ActiveGenerationState),
}

/// Metadata for one active generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveGenerationState {
    /// Monotonic generation identifier.
    pub generation_id: u64,
    /// Activation start timestamp.
    pub started_at_unix_ms: u64,
    /// Artifact path assigned to this generation.
    pub artifact_path: PathBuf,
    /// Whether this generation currently accepts new admissions.
    pub accepting_new_admissions: bool,
    /// Whether this generation is marked closing.
    pub closing: bool,
    /// Number of admitted captured requests still in-flight.
    pub inflight_captured_requests: u64,
}

#[derive(Debug)]
enum ControllerLifecycle {
    Disabled {
        next_generation: u64,
    },
    Active {
        active: Arc<ActiveGenerationRuntime>,
        next_generation: u64,
    },
}

impl ControllerLifecycle {
    fn snapshot(&self) -> GenerationState {
        match self {
            Self::Disabled { next_generation } => GenerationState::Disabled {
                next_generation: *next_generation,
            },
            Self::Active { active, .. } => GenerationState::Active(active.snapshot()),
        }
    }
}

/// Errors emitted while building a controller.
#[derive(Debug)]
pub enum ControllerBuildError {
    /// Service name was empty.
    EmptyServiceName,
    /// Building the disabled-path inert run failed.
    InertRunBuild(BuildError),
    /// Initially-enabled controller failed to create first generation.
    InitialEnable(EnableError),
}

impl std::fmt::Display for ControllerBuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyServiceName => write!(f, "service_name cannot be empty"),
            Self::InertRunBuild(err) => write!(f, "failed to build disabled-path inert run: {err}"),
            Self::InitialEnable(err) => write!(f, "failed to start initial generation: {err}"),
        }
    }
}

impl std::error::Error for ControllerBuildError {}

/// Errors emitted when enabling/arming controller capture.
#[derive(Debug)]
pub enum EnableError {
    /// Another generation is already active.
    AlreadyActive {
        /// ID of the active generation blocking a new start.
        generation_id: u64,
    },
    /// Building the fresh bounded run failed.
    Build(BuildError),
}

impl std::fmt::Display for EnableError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyActive { generation_id } => {
                write!(f, "generation {generation_id} is already active")
            }
            Self::Build(err) => write!(f, "failed to build generation run: {err}"),
        }
    }
}

impl std::error::Error for EnableError {}

/// Errors emitted while disarming and finalizing generation artifacts.
#[derive(Debug)]
pub enum DisableError {
    /// Artifact writing failed during generation finalization.
    Finalize(tailtriage_core::SinkError),
}

impl std::fmt::Display for DisableError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Finalize(err) => write!(f, "failed to finalize generation: {err}"),
        }
    }
}

impl std::error::Error for DisableError {}

/// Outcome of calling [`TailtriageController::disable`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisableOutcome {
    /// Controller was already disarmed.
    AlreadyDisabled,
    /// Active generation is closing and will finalize once in-flight requests drain.
    Closing {
        /// Active generation ID.
        generation_id: u64,
        /// Number of admitted captured requests still in flight.
        inflight_captured_requests: u64,
    },
    /// Active generation finalized immediately.
    Finalized {
        /// Generation ID that was finalized.
        generation_id: u64,
    },
}

/// Errors emitted during process shutdown finalization.
#[derive(Debug)]
pub enum ShutdownError {
    /// Active generation could not be finalized.
    Finalize(DisableError),
}

impl std::fmt::Display for ShutdownError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Finalize(err) => write!(f, "shutdown finalization failed: {err}"),
        }
    }
}

impl std::error::Error for ShutdownError {}

fn generated_artifact_path(template: &ControllerSinkTemplate, generation_id: u64) -> PathBuf {
    match template {
        ControllerSinkTemplate::LocalJson { output_path } => {
            let parent = output_path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_default();
            let stem = output_path
                .file_stem()
                .and_then(std::ffi::OsStr::to_str)
                .unwrap_or("tailtriage-run");
            let extension = output_path.extension().and_then(std::ffi::OsStr::to_str);
            let filename = match extension {
                Some(ext) if !ext.is_empty() => format!("{stem}-generation-{generation_id}.{ext}"),
                _ => format!("{stem}-generation-{generation_id}.json"),
            };
            parent.join(filename)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{DisableOutcome, EnableError, GenerationState, TailtriageController};
    use tailtriage_core::RequestOptions;

    fn test_output(base: &str) -> std::path::PathBuf {
        let unique = format!(
            "tailtriage-controller-{base}-{}-{}.json",
            std::process::id(),
            tailtriage_core::unix_time_ms()
        );
        std::env::temp_dir().join(unique)
    }

    fn read_artifact(path: &std::path::Path) -> String {
        fs::read_to_string(path).expect("artifact should be readable")
    }

    #[test]
    fn enable_capture_disable_finalizes_generation() {
        let output = test_output("enable-capture-disable");
        let controller = TailtriageController::builder("checkout-service")
            .output(&output)
            .build()
            .expect("build should succeed");

        let active = controller.enable().expect("enable should succeed");
        let started = controller.begin_request("/checkout");
        started.completion.finish_ok();

        let disable = controller.disable().expect("disable should succeed");
        assert!(matches!(
            disable,
            DisableOutcome::Finalized {
                generation_id: id
            } if id == active.generation_id
        ));

        let expected = output.with_file_name(format!(
            "{}-generation-1.json",
            output
                .file_stem()
                .and_then(std::ffi::OsStr::to_str)
                .expect("stem")
        ));
        assert!(expected.exists());

        fs::remove_file(expected).expect("cleanup should succeed");
    }

    #[test]
    fn enable_disable_reenable_creates_distinct_generation_and_artifact() {
        let output = test_output("reenable");
        let controller = TailtriageController::builder("checkout-service")
            .output(&output)
            .build()
            .expect("build should succeed");

        let first = controller.enable().expect("first enable should succeed");
        assert!(matches!(
            controller.disable(),
            Ok(DisableOutcome::Finalized { generation_id: 1 })
        ));

        let second = controller.enable().expect("second enable should succeed");
        assert_eq!(first.generation_id + 1, second.generation_id);
        assert_ne!(first.artifact_path, second.artifact_path);

        assert!(matches!(
            controller.disable(),
            Ok(DisableOutcome::Finalized { generation_id: 2 })
        ));

        fs::remove_file(first.artifact_path).expect("cleanup first artifact should succeed");
        fs::remove_file(second.artifact_path).expect("cleanup second artifact should succeed");
    }

    #[test]
    fn request_started_before_disable_can_finish_after_disable() {
        let output = test_output("finish-after-disable");
        let controller = TailtriageController::builder("checkout-service")
            .output(&output)
            .build()
            .expect("build should succeed");

        let active = controller.enable().expect("enable should succeed");
        let started = controller.begin_request("/checkout");

        let disable = controller.disable().expect("disable should succeed");
        assert!(matches!(
            disable,
            DisableOutcome::Closing {
                generation_id,
                inflight_captured_requests: 1
            } if generation_id == active.generation_id
        ));

        started.completion.finish_ok();

        let status = controller.status();
        assert!(matches!(
            status.generation,
            GenerationState::Disabled { next_generation: 2 }
        ));
        assert!(active.artifact_path.exists());

        fs::remove_file(active.artifact_path).expect("cleanup should succeed");
    }

    #[test]
    fn no_new_admissions_after_disable() {
        let output = test_output("no-admissions");
        let controller = TailtriageController::builder("checkout-service")
            .output(&output)
            .build()
            .expect("build should succeed");

        let active = controller.enable().expect("enable should succeed");
        let started = controller.begin_request("/checkout");

        let _ = controller.disable().expect("disable should succeed");

        controller.begin_request("/checkout").completion.finish_ok();

        started.completion.finish_ok();
        fs::remove_file(active.artifact_path).expect("cleanup should succeed");
    }

    #[test]
    fn one_active_generation_at_a_time() {
        let controller = TailtriageController::builder("checkout-service")
            .build()
            .expect("build should succeed");

        let first = controller.enable().expect("first enable should succeed");
        let err = controller
            .enable()
            .expect_err("second enable should fail while first generation active");

        assert!(matches!(
            err,
            EnableError::AlreadyActive {
                generation_id
            } if generation_id == first.generation_id
        ));

        assert!(matches!(
            controller.disable(),
            Ok(DisableOutcome::Finalized { .. })
        ));
        fs::remove_file(first.artifact_path).expect("cleanup should succeed");
    }

    #[test]
    fn request_completion_remains_bound_to_original_generation_after_reenable() {
        let output = test_output("generation-binding");
        let controller = TailtriageController::builder("checkout-service")
            .output(&output)
            .build()
            .expect("build should succeed");

        let gen_a = controller.enable().expect("generation A should enable");
        let started_a = controller.begin_request_with(
            "/checkout",
            RequestOptions::new().request_id("req-generation-a"),
        );

        assert!(matches!(
            controller.disable(),
            Ok(DisableOutcome::Closing {
                generation_id,
                inflight_captured_requests: 1
            }) if generation_id == gen_a.generation_id
        ));

        started_a.completion.finish_ok();

        let gen_b = controller.enable().expect("generation B should enable");
        let started_b = controller.begin_request_with(
            "/checkout",
            RequestOptions::new().request_id("req-generation-b"),
        );
        started_b.completion.finish_ok();
        assert!(matches!(
            controller.disable(),
            Ok(DisableOutcome::Finalized { generation_id })
            if generation_id == gen_b.generation_id
        ));

        let run_a = read_artifact(&gen_a.artifact_path);
        let run_b = read_artifact(&gen_b.artifact_path);
        assert!(run_a.contains("req-generation-a"));
        assert!(!run_a.contains("req-generation-b"));
        assert!(run_b.contains("req-generation-b"));
        assert!(!run_b.contains("req-generation-a"));

        fs::remove_file(gen_a.artifact_path).expect("cleanup generation A should succeed");
        fs::remove_file(gen_b.artifact_path).expect("cleanup generation B should succeed");
    }

    #[test]
    fn disabled_begin_request_is_inert_and_never_joins_later_generation() {
        let output = test_output("disabled-admission");
        let controller = TailtriageController::builder("checkout-service")
            .output(&output)
            .build()
            .expect("build should succeed");

        let disabled_started = controller.begin_request_with(
            "/checkout",
            RequestOptions::new().request_id("req-disabled"),
        );
        assert_eq!(disabled_started.handle.request_id(), "");
        disabled_started.completion.finish_ok();

        let active = controller.enable().expect("enable should succeed");
        let started = controller
            .begin_request_with("/checkout", RequestOptions::new().request_id("req-enabled"));
        started.completion.finish_ok();
        assert!(matches!(
            controller.disable(),
            Ok(DisableOutcome::Finalized { generation_id }) if generation_id == active.generation_id
        ));

        let run = read_artifact(&active.artifact_path);
        assert!(run.contains("req-enabled"));
        assert!(!run.contains("req-disabled"));

        fs::remove_file(active.artifact_path).expect("cleanup should succeed");
    }

    #[test]
    fn disabled_handle_and_completion_operations_are_noop() {
        let output = test_output("disabled-noop");
        let controller = TailtriageController::builder("checkout-service")
            .output(&output)
            .build()
            .expect("build should succeed");

        let started = controller.begin_request_with(
            "/checkout",
            RequestOptions::new()
                .request_id("req-disabled-noop")
                .kind("http"),
        );

        assert_eq!(started.handle.request_id(), "");
        assert_eq!(started.handle.route(), "/checkout");
        assert_eq!(started.handle.kind(), Some("http"));
        let request = started.handle.clone();
        let _inflight = request.inflight("inflight-disabled");
        let _queue = request.queue("queue-disabled");
        let _stage = request.stage("stage-disabled");
        started
            .completion
            .finish_result::<(), &str>(Err("disabled-result"))
            .expect_err("disabled result should pass through unchanged");

        let active = controller.enable().expect("enable should succeed");
        let enabled_started = controller
            .begin_request_with("/checkout", RequestOptions::new().request_id("req-enabled"));
        enabled_started.completion.finish_ok();
        assert!(matches!(
            controller.disable(),
            Ok(DisableOutcome::Finalized { generation_id }) if generation_id == active.generation_id
        ));

        let run = read_artifact(&active.artifact_path);
        assert!(run.contains("req-enabled"));
        assert!(!run.contains("req-disabled-noop"));

        fs::remove_file(active.artifact_path).expect("cleanup should succeed");
    }

    #[test]
    fn rapid_enable_disable_boundaries_keep_generation_isolation() {
        let output = test_output("rapid-boundaries");
        let controller = TailtriageController::builder("checkout-service")
            .output(&output)
            .build()
            .expect("build should succeed");

        let mut artifacts = Vec::new();
        for generation in 1..=3 {
            let active = controller.enable().expect("enable should succeed");
            assert_eq!(active.generation_id, generation);

            let started = controller.begin_request_with(
                "/checkout",
                RequestOptions::new().request_id(format!("req-gen-{generation}")),
            );

            assert!(matches!(
                controller.disable(),
                Ok(DisableOutcome::Closing {
                    generation_id,
                    inflight_captured_requests: 1
                }) if generation_id == generation
            ));

            assert!(
                matches!(
                    controller.enable(),
                    Err(EnableError::AlreadyActive { generation_id }) if generation_id == generation
                ),
                "controller must not start next generation before admitted requests drain"
            );

            started.completion.finish_ok();
            artifacts.push(active.artifact_path);
        }

        for (idx, artifact) in artifacts.iter().enumerate() {
            let run = read_artifact(artifact);
            assert!(run.contains(&format!("req-gen-{}", idx + 1)));
            fs::remove_file(artifact).expect("cleanup should succeed");
        }
    }

    #[test]
    fn completion_drain_finalizes_once_without_duplicate_side_effects() {
        let output = test_output("single-finalize");
        let controller = TailtriageController::builder("checkout-service")
            .output(&output)
            .build()
            .expect("build should succeed");

        let active = controller.enable().expect("enable should succeed");
        let started = controller
            .begin_request_with("/checkout", RequestOptions::new().request_id("req-once"));

        assert!(matches!(
            controller.disable(),
            Ok(DisableOutcome::Closing {
                generation_id,
                inflight_captured_requests: 1
            }) if generation_id == active.generation_id
        ));

        started.completion.finish_ok();
        assert!(matches!(
            controller.disable(),
            Ok(DisableOutcome::AlreadyDisabled)
        ));
        assert!(matches!(controller.shutdown(), Ok(())));

        let run = read_artifact(&active.artifact_path);
        assert_eq!(run.matches("req-once").count(), 1);

        fs::remove_file(active.artifact_path).expect("cleanup should succeed");
    }
}
