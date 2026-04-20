#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

//! Long-lived capture control layer for repeated bounded tailtriage activations.
//!
//! Layering:
//!
//! - [`tailtriage_core`] remains the per-run collector and artifact model.
//! - `tailtriage-controller` provides control-layer scaffolding for live arm/disarm
//!   workflows that create fresh bounded runs on every activation.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tailtriage_core::{
    BuildError, CaptureLimitsOverride, CaptureMode, InflightGuard, Outcome, OwnedRequestCompletion,
    OwnedRequestHandle, QueueTimer, RequestOptions, RunEndReason, StageTimer, Tailtriage,
};
use tailtriage_tokio::{RuntimeSampler, SamplerStartError};

/// Builder for a long-lived [`TailtriageController`].
#[derive(Debug, Clone)]
pub struct TailtriageControllerBuilder {
    service_name: String,
    config_path: Option<PathBuf>,
    initially_enabled: bool,
    sink_template: ControllerSinkTemplate,
    capture_limits_override: CaptureLimitsOverride,
    strict_lifecycle: bool,
    runtime_sampler: RuntimeSamplerTemplate,
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
            capture_limits_override: CaptureLimitsOverride::default(),
            strict_lifecycle: false,
            runtime_sampler: RuntimeSamplerTemplate::default(),
            run_end_policy: RunEndPolicy::ContinueAfterLimitsHit,
        }
    }

    /// Sets the optional config path used for reloadable controller config.
    #[must_use]
    pub fn config_path(mut self, config_path: impl AsRef<Path>) -> Self {
        self.config_path = Some(config_path.as_ref().to_path_buf());
        self
    }

    /// Sets whether build should immediately create the first active generation.
    ///
    /// When set to `true`, [`Self::build`] calls [`TailtriageController::enable`]
    /// during construction so generation `1` is active as soon as build succeeds.
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

    /// Sets field-level capture limit overrides applied on top of selected mode defaults.
    #[must_use]
    pub const fn capture_limits_override(
        mut self,
        capture_limits_override: CaptureLimitsOverride,
    ) -> Self {
        self.capture_limits_override = capture_limits_override;
        self
    }

    /// Sets strict lifecycle validation applied to future activation runs.
    #[must_use]
    pub const fn strict_lifecycle(mut self, strict_lifecycle: bool) -> Self {
        self.strict_lifecycle = strict_lifecycle;
        self
    }

    /// Sets runtime sampler template settings for future activations.
    #[must_use]
    pub const fn runtime_sampler(mut self, runtime_sampler: RuntimeSamplerTemplate) -> Self {
        self.runtime_sampler = runtime_sampler;
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
        let mut service_name = self.service_name;
        if service_name.trim().is_empty() {
            return Err(ControllerBuildError::EmptyServiceName);
        }

        let mut initially_enabled = self.initially_enabled;
        let mut sink_template = self.sink_template;
        let mut selected_mode = CaptureMode::Light;
        let mut capture_limits_override = self.capture_limits_override;
        let mut strict_lifecycle = self.strict_lifecycle;
        let mut runtime_sampler = self.runtime_sampler;
        let mut run_end_policy = self.run_end_policy;

        if let Some(config_path) = self.config_path.as_ref() {
            let loaded = TailtriageController::load_config_from_path(config_path)
                .map_err(ControllerBuildError::ConfigLoad)?;
            let activation = loaded.activation_template;
            service_name = loaded.service_name.unwrap_or(service_name);
            initially_enabled = loaded.initially_enabled.unwrap_or(initially_enabled);
            sink_template = activation.sink_template;
            selected_mode = activation.selected_mode;
            capture_limits_override = activation.capture_limits_override;
            strict_lifecycle = activation.strict_lifecycle;
            runtime_sampler = activation.runtime_sampler;
            run_end_policy = activation.run_end_policy;
        }

        if service_name.trim().is_empty() {
            return Err(ControllerBuildError::EmptyServiceName);
        }

        let template = TailtriageControllerTemplate {
            service_name,
            config_path: self.config_path,
            sink_template,
            selected_mode: CaptureMode::Light,
            capture_limits_override,
            strict_lifecycle,
            runtime_sampler,
            run_end_policy,
        };
        let template = TailtriageControllerTemplate {
            selected_mode,
            ..template
        };

        let inner = Arc::new(ControllerInner {
            template: Mutex::new(template),
            lifecycle: Mutex::new(ControllerLifecycle::Disabled { next_generation: 1 }),
        });

        let controller = TailtriageController { inner };
        if initially_enabled {
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
    runtime_sampler: Mutex<Option<RuntimeSampler>>,
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
            activation_config: self.state.activation_config.clone(),
        }
    }
}

impl TailtriageController {
    /// Creates a builder for controller-level scaffolding.
    #[must_use]
    pub fn builder(service_name: impl Into<String>) -> TailtriageControllerBuilder {
        TailtriageControllerBuilder::new(service_name)
    }

    /// Loads controller TOML config from `path` without mutating controller state.
    ///
    /// This helper parses and returns the activation template that would be applied
    /// on reload/build.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigLoadError`] when reading or parsing the TOML file fails.
    pub fn load_config_from_path(
        path: impl AsRef<Path>,
    ) -> Result<LoadedControllerConfig, ConfigLoadError> {
        let path = path.as_ref();
        let file = ControllerConfigFile::from_path(path)?;
        Ok(file.into_loaded())
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
            template: template.clone(),
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

    /// Reloads controller config from the configured template file path.
    ///
    /// Reload only updates the template for future activations. Any active generation
    /// keeps the activation config it started with.
    ///
    /// # Errors
    ///
    /// Returns [`ReloadConfigError`] when the controller has no `config_path` or when
    /// loading/parsing/validating the TOML file fails.
    ///
    /// # Panics
    ///
    /// Panics if the controller template mutex is poisoned.
    pub fn reload_config(&self) -> Result<(), ReloadConfigError> {
        let (config_path, service_name) = {
            let template = self
                .inner
                .template
                .lock()
                .expect("controller template lock poisoned");
            let Some(config_path) = template.config_path.clone() else {
                return Err(ReloadConfigError::MissingConfigPath);
            };
            (config_path, template.service_name.clone())
        };

        let loaded = TailtriageController::load_config_from_path(&config_path)
            .map_err(ReloadConfigError::Load)?;
        let activation = loaded.activation_template;

        let mut template = self
            .inner
            .template
            .lock()
            .expect("controller template lock poisoned");
        template.service_name = loaded.service_name.unwrap_or(service_name);
        template.sink_template = activation.sink_template;
        template.selected_mode = activation.selected_mode;
        template.capture_limits_override = activation.capture_limits_override;
        template.strict_lifecycle = activation.strict_lifecycle;
        template.runtime_sampler = activation.runtime_sampler;
        template.run_end_policy = activation.run_end_policy;

        Ok(())
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
        builder = builder.capture_limits_override(template.capture_limits_override);
        builder = builder.strict_lifecycle(template.strict_lifecycle);

        let run = Arc::new(builder.build().map_err(EnableError::Build)?);
        let runtime_sampler = if template.runtime_sampler.enabled_for_armed_runs {
            let _ = tokio::runtime::Handle::try_current()
                .map_err(|_| EnableError::MissingTokioRuntimeForSampler)?;
            let mut sampler_builder = RuntimeSampler::builder(Arc::clone(&run));
            if let Some(mode_override) = template.runtime_sampler.mode_override {
                sampler_builder = sampler_builder.mode(mode_override);
            }
            if let Some(interval_ms) = template.runtime_sampler.interval_ms {
                sampler_builder = sampler_builder.interval(Duration::from_millis(interval_ms));
            }
            if let Some(max_runtime_snapshots) = template.runtime_sampler.max_runtime_snapshots {
                sampler_builder = sampler_builder.max_runtime_snapshots(max_runtime_snapshots);
            }
            Some(
                sampler_builder
                    .start()
                    .map_err(EnableError::StartRuntimeSampler)?,
            )
        } else {
            None
        };
        let runtime = Arc::new(ActiveGenerationRuntime {
            state: ActiveGenerationState {
                generation_id: next_generation,
                started_at_unix_ms: tailtriage_core::unix_time_ms(),
                artifact_path: artifact_path.clone(),
                accepting_new_admissions: true,
                closing: false,
                inflight_captured_requests: 0,
                activation_config: ControllerActivationTemplate {
                    sink_template: template.sink_template.clone(),
                    selected_mode: template.selected_mode,
                    capture_limits_override: template.capture_limits_override,
                    strict_lifecycle: template.strict_lifecycle,
                    runtime_sampler: template.runtime_sampler,
                    run_end_policy: template.run_end_policy,
                },
            },
            artifact_path,
            run,
            accepting_new: AtomicBool::new(true),
            closing: AtomicBool::new(false),
            inflight_captured: AtomicU64::new(0),
            finalize_started: AtomicBool::new(false),
            runtime_sampler: Mutex::new(runtime_sampler),
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

            active
                .run
                .set_run_end_reason_if_absent(RunEndReason::ManualDisarm);
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

        ControllerStartedRequest {
            handle: ControllerRequestHandle::Inert(InertControllerRequestHandle::new(
                route, options,
            )),
            completion: ControllerRequestCompletion {
                kind: ControllerCompletionKind::Inert,
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

        if active.state.activation_config.run_end_policy == RunEndPolicy::AutoSealOnLimitsHit
            && active.run.snapshot().truncation.limits_hit
        {
            active
                .run
                .set_run_end_reason_if_absent(RunEndReason::AutoSealOnLimitsHit);
            active.accepting_new.store(false, Ordering::Release);
            active.closing.store(true, Ordering::Release);
            if active.inflight_captured.load(Ordering::Acquire) == 0 {
                let _ = self.force_finalize_generation(&active);
            }
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
        Self::apply_run_end_policy_if_limits_hit(&active);

        Some(ControllerStartedRequest {
            handle: ControllerRequestHandle::Active(started.handle),
            completion: ControllerRequestCompletion {
                kind: ControllerCompletionKind::Active(ActiveControllerCompletion {
                    completion: Some(started.completion),
                    admission_generation_id: active.state.generation_id,
                    admitted_generation: Arc::downgrade(&active),
                    inner: Arc::downgrade(&self.inner),
                    run_end_policy: active.state.activation_config.run_end_policy,
                    inflight_recorded: true,
                }),
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
            active
                .run
                .set_run_end_reason_if_absent(RunEndReason::Shutdown);
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

        Self::stop_runtime_sampler(active);
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

    fn stop_runtime_sampler(active: &Arc<ActiveGenerationRuntime>) {
        let sampler = active
            .runtime_sampler
            .lock()
            .expect("generation runtime sampler lock poisoned")
            .take();
        if let Some(sampler) = sampler {
            let shutdown_thread = std::thread::spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("sampler shutdown runtime should build");
                runtime.block_on(sampler.shutdown());
            });
            let _ = shutdown_thread.join();
        }
    }

    fn apply_run_end_policy_if_limits_hit(active: &Arc<ActiveGenerationRuntime>) {
        if active.state.activation_config.run_end_policy != RunEndPolicy::AutoSealOnLimitsHit {
            return;
        }

        if !active.run.snapshot().truncation.limits_hit {
            return;
        }

        active
            .run
            .set_run_end_reason_if_absent(RunEndReason::AutoSealOnLimitsHit);
        active.accepting_new.store(false, Ordering::Release);
        active.closing.store(true, Ordering::Release);
    }
}

/// Result of trying to begin one captured request in a generation.
#[must_use = "request completion must be finished explicitly"]
#[derive(Debug)]
pub struct ControllerStartedRequest {
    /// Instrumentation handle for queue/stage/inflight timing.
    pub handle: ControllerRequestHandle,
    /// Completion token bound to one generation.
    pub completion: ControllerRequestCompletion,
}

/// Completion token for a request admitted through [`TailtriageController`].
#[must_use = "request completion must be finished explicitly"]
#[derive(Debug)]
pub struct ControllerRequestCompletion {
    kind: ControllerCompletionKind,
}

impl ControllerRequestCompletion {
    /// Finishes this request with an explicit outcome.
    pub fn finish(mut self, outcome: Outcome) {
        if let ControllerCompletionKind::Active(active) = &mut self.kind {
            if let Some(completion) = active.completion.take() {
                completion.finish(outcome);
                active.mark_finished();
            }
        }
    }

    /// Convenience helper for successful completion.
    pub fn finish_ok(self) {
        self.finish(Outcome::Ok);
    }

    /// Finishes from `result` and returns `result` unchanged.
    ///
    /// # Errors
    ///
    /// This method does not create new errors. It returns `result` unchanged,
    /// including the original `Err(E)` value.
    pub fn finish_result<T, E>(mut self, result: Result<T, E>) -> Result<T, E> {
        if let ControllerCompletionKind::Active(active) = &mut self.kind {
            if let Some(completion) = active.completion.take() {
                completion.finish(if result.is_ok() {
                    Outcome::Ok
                } else {
                    Outcome::Error
                });
                active.mark_finished();
            }
        }
        result
    }
}

#[derive(Debug)]
enum ControllerCompletionKind {
    Active(ActiveControllerCompletion),
    Inert,
}

#[derive(Debug)]
struct ActiveControllerCompletion {
    completion: Option<OwnedRequestCompletion>,
    /// Generation captured at admission time.
    ///
    /// This binding is immutable for the life of the completion token so that
    /// request finalization cannot migrate to a later generation during rapid
    /// enable/disable/re-enable transitions.
    admission_generation_id: u64,
    /// Weak reference to the exact runtime generation that admitted the request.
    ///
    /// Keeping this pointer ensures inflight accounting and close/finalize checks
    /// operate on the admitted generation even if controller lifecycle has already
    /// advanced to a newer generation.
    admitted_generation: Weak<ActiveGenerationRuntime>,
    inner: Weak<ControllerInner>,
    run_end_policy: RunEndPolicy,
    inflight_recorded: bool,
}

impl ActiveControllerCompletion {
    fn mark_finished(&mut self) {
        if !self.inflight_recorded {
            return;
        }

        self.inflight_recorded = false;

        let Some(active) = self.admitted_generation.upgrade() else {
            return;
        };

        debug_assert_eq!(
            active.state.generation_id, self.admission_generation_id,
            "controller completion generation binding should remain stable"
        );

        if self.run_end_policy == RunEndPolicy::AutoSealOnLimitsHit
            && active.run.snapshot().truncation.limits_hit
        {
            active
                .run
                .set_run_end_reason_if_absent(RunEndReason::AutoSealOnLimitsHit);
            active.accepting_new.store(false, Ordering::Release);
            active.closing.store(true, Ordering::Release);
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

        TailtriageController::stop_runtime_sampler(active);
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

/// Instrumentation handle for requests admitted through [`TailtriageController`].
#[derive(Debug, Clone)]
pub enum ControllerRequestHandle {
    /// Active request handle delegated to one admitted generation.
    Active(OwnedRequestHandle),
    /// Inert request handle returned while disabled/closing.
    Inert(InertControllerRequestHandle),
}

impl ControllerRequestHandle {
    /// Correlation ID attached to this request.
    #[must_use]
    pub fn request_id(&self) -> &str {
        match self {
            Self::Active(handle) => handle.request_id(),
            Self::Inert(handle) => handle.request_id(),
        }
    }

    /// Route/operation name attached to this request.
    #[must_use]
    pub fn route(&self) -> &str {
        match self {
            Self::Active(handle) => handle.route(),
            Self::Inert(handle) => handle.route(),
        }
    }

    /// Optional kind metadata attached to this request.
    #[must_use]
    pub fn kind(&self) -> Option<&str> {
        match self {
            Self::Active(handle) => handle.kind(),
            Self::Inert(handle) => handle.kind(),
        }
    }

    /// Starts queue-wait timing instrumentation for `queue`.
    #[must_use]
    pub fn queue(&self, queue: impl Into<String>) -> ControllerQueueTimer<'_> {
        match self {
            Self::Active(handle) => ControllerQueueTimer::Active(handle.queue(queue)),
            Self::Inert(_) => ControllerQueueTimer::Inert,
        }
    }

    /// Starts stage timing instrumentation for `stage`.
    #[must_use]
    pub fn stage(&self, stage: impl Into<String>) -> ControllerStageTimer<'_> {
        match self {
            Self::Active(handle) => ControllerStageTimer::Active(handle.stage(stage)),
            Self::Inert(_) => ControllerStageTimer::Inert,
        }
    }

    /// Creates an in-flight guard for `gauge`.
    #[must_use]
    pub fn inflight(&self, gauge: impl Into<String>) -> ControllerInflightGuard<'_> {
        match self {
            Self::Active(handle) => ControllerInflightGuard::Active(handle.inflight(gauge)),
            Self::Inert(_) => ControllerInflightGuard::Inert,
        }
    }
}

/// Inert controller request handle metadata stored while disabled/closing.
#[derive(Debug, Clone)]
pub struct InertControllerRequestHandle {
    request_id: String,
    route: String,
    kind: Option<String>,
}

impl InertControllerRequestHandle {
    fn new(route: String, options: RequestOptions) -> Self {
        Self {
            request_id: options.request_id.unwrap_or_default(),
            route,
            kind: options.kind,
        }
    }

    fn request_id(&self) -> &str {
        &self.request_id
    }

    fn route(&self) -> &str {
        &self.route
    }

    fn kind(&self) -> Option<&str> {
        self.kind.as_deref()
    }
}

/// Controller-local queue timer wrapper.
#[derive(Debug)]
pub enum ControllerQueueTimer<'a> {
    /// Queue timer delegated to an active generation.
    Active(QueueTimer<'a>),
    /// Inert timer used while disabled/closing.
    Inert,
}

impl ControllerQueueTimer<'_> {
    /// Sets queue depth sample captured at wait start.
    #[must_use]
    pub fn with_depth_at_start(self, depth_at_start: u64) -> Self {
        match self {
            Self::Active(timer) => Self::Active(timer.with_depth_at_start(depth_at_start)),
            Self::Inert => Self::Inert,
        }
    }

    /// Awaits `fut`, recording queue wait for active requests only.
    pub async fn await_on<Fut, T>(self, fut: Fut) -> T
    where
        Fut: std::future::Future<Output = T>,
    {
        match self {
            Self::Active(timer) => timer.await_on(fut).await,
            Self::Inert => fut.await,
        }
    }
}

/// Controller-local stage timer wrapper.
#[derive(Debug)]
pub enum ControllerStageTimer<'a> {
    /// Stage timer delegated to an active generation.
    Active(StageTimer<'a>),
    /// Inert timer used while disabled/closing.
    Inert,
}

impl ControllerStageTimer<'_> {
    /// Awaits `fut`, recording stage duration for active requests only.
    ///
    /// # Errors
    ///
    /// Returns the same `Err(E)` produced by `fut` unchanged.
    pub async fn await_on<Fut, T, E>(self, fut: Fut) -> Result<T, E>
    where
        Fut: std::future::Future<Output = Result<T, E>>,
    {
        match self {
            Self::Active(timer) => timer.await_on(fut).await,
            Self::Inert => fut.await,
        }
    }

    /// Awaits infallible stage work, recording active requests only.
    pub async fn await_value<Fut, T>(self, fut: Fut) -> T
    where
        Fut: std::future::Future<Output = T>,
    {
        match self {
            Self::Active(timer) => timer.await_value(fut).await,
            Self::Inert => fut.await,
        }
    }
}

/// Controller-local in-flight guard wrapper.
#[derive(Debug)]
pub enum ControllerInflightGuard<'a> {
    /// In-flight guard delegated to an active generation.
    Active(InflightGuard<'a>),
    /// Inert guard used while disabled/closing.
    Inert,
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
    /// Field-level capture limits override applied on top of mode defaults.
    pub capture_limits_override: CaptureLimitsOverride,
    /// Strict lifecycle behavior for next activations.
    pub strict_lifecycle: bool,
    /// Runtime sampler template for next activations.
    pub runtime_sampler: RuntimeSamplerTemplate,
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

/// Runtime sampler template attached to controller activation settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RuntimeSamplerTemplate {
    /// Enables runtime sampler startup for armed runs.
    pub enabled_for_armed_runs: bool,
    /// Optional mode override used by runtime sampler.
    pub mode_override: Option<CaptureMode>,
    /// Optional interval override in milliseconds.
    pub interval_ms: Option<u64>,
    /// Optional max runtime snapshots override.
    pub max_runtime_snapshots: Option<usize>,
}

/// Policy for bounded activation run completion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunEndPolicy {
    /// Keep cheap-dropping after limits are hit until manual disarm or shutdown.
    ContinueAfterLimitsHit,
    /// Stop admissions and seal the run once limits are hit.
    AutoSealOnLimitsHit,
}

/// Public status snapshot for reporting controller state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TailtriageControllerStatus {
    /// Template used for the next activation generation.
    pub template: TailtriageControllerTemplate,
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
    /// Controller currently owns one active generation.
    Active(Box<ActiveGenerationState>),
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
    /// Effective activation settings fixed for this generation.
    pub activation_config: ControllerActivationTemplate,
}

/// One bounded activation template snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControllerActivationTemplate {
    /// Sink/output settings for this generation.
    pub sink_template: ControllerSinkTemplate,
    /// Core mode for this generation.
    pub selected_mode: CaptureMode,
    /// Field-level capture limit overrides for this generation.
    pub capture_limits_override: CaptureLimitsOverride,
    /// Strict lifecycle behavior for this generation.
    pub strict_lifecycle: bool,
    /// Runtime sampler settings for this generation.
    pub runtime_sampler: RuntimeSamplerTemplate,
    /// Run-end policy for this generation.
    pub run_end_policy: RunEndPolicy,
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
            Self::Active { active, .. } => GenerationState::Active(Box::new(active.snapshot())),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct ControllerConfigFile {
    controller: ControllerConfigToml,
}

impl ControllerConfigFile {
    fn from_path(path: &Path) -> Result<Self, ConfigLoadError> {
        let raw = fs::read_to_string(path).map_err(|source| ConfigLoadError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        toml::from_str(&raw).map_err(|source| ConfigLoadError::Parse {
            path: path.to_path_buf(),
            source,
        })
    }

    fn into_loaded(self) -> LoadedControllerConfig {
        let activation = self.controller.activation;
        let run_end_policy = activation.run_end_policy();
        LoadedControllerConfig {
            service_name: self.controller.service_name,
            initially_enabled: self.controller.initially_enabled,
            activation_template: ControllerActivationTemplate {
                sink_template: activation.sink.into_template(),
                selected_mode: activation.mode,
                capture_limits_override: activation.capture_limits_override,
                strict_lifecycle: activation.strict_lifecycle,
                runtime_sampler: activation.runtime_sampler,
                run_end_policy,
            },
        }
    }
}

/// Parsed controller config loaded from a TOML file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedControllerConfig {
    /// Optional service name override.
    pub service_name: Option<String>,
    /// Optional initially-enabled flag.
    pub initially_enabled: Option<bool>,
    /// Activation template loaded from config.
    pub activation_template: ControllerActivationTemplate,
}

#[derive(Debug, Clone, Deserialize)]
struct ControllerConfigToml {
    service_name: Option<String>,
    initially_enabled: Option<bool>,
    activation: ControllerActivationConfigToml,
}

#[derive(Debug, Clone, Deserialize)]
struct ControllerActivationConfigToml {
    mode: CaptureMode,
    #[serde(default)]
    capture_limits_override: CaptureLimitsOverride,
    #[serde(default)]
    strict_lifecycle: bool,
    sink: ControllerSinkTemplateToml,
    #[serde(default)]
    runtime_sampler: RuntimeSamplerTemplate,
    #[serde(default)]
    run_end_policy: RunEndPolicyConfigToml,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ControllerSinkTemplateToml {
    LocalJson { output_path: PathBuf },
}

impl ControllerSinkTemplateToml {
    fn into_template(self) -> ControllerSinkTemplate {
        match self {
            Self::LocalJson { output_path } => ControllerSinkTemplate::LocalJson { output_path },
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum RunEndPolicyConfigToml {
    #[default]
    ContinueAfterLimitsHit,
    AutoSealOnLimitsHit,
}

impl From<RunEndPolicyConfigToml> for RunEndPolicy {
    fn from(value: RunEndPolicyConfigToml) -> Self {
        match value {
            RunEndPolicyConfigToml::ContinueAfterLimitsHit => Self::ContinueAfterLimitsHit,
            RunEndPolicyConfigToml::AutoSealOnLimitsHit => Self::AutoSealOnLimitsHit,
        }
    }
}

impl ControllerActivationConfigToml {
    fn run_end_policy(&self) -> RunEndPolicy {
        self.run_end_policy.clone().into()
    }
}

/// Errors emitted while loading controller TOML config from disk.
#[derive(Debug)]
pub enum ConfigLoadError {
    /// Reading the config file failed.
    Io {
        /// Path that failed to read.
        path: PathBuf,
        /// Underlying I/O error.
        source: std::io::Error,
    },
    /// TOML parsing failed.
    Parse {
        /// Path that failed to parse.
        path: PathBuf,
        /// Underlying TOML parse error.
        source: toml::de::Error,
    },
}

impl std::fmt::Display for ConfigLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(
                    f,
                    "failed to read controller config {}: {source}",
                    path.display()
                )
            }
            Self::Parse { path, source } => {
                write!(
                    f,
                    "failed to parse controller config TOML {}: {source}",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for ConfigLoadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Parse { source, .. } => Some(source),
        }
    }
}

/// Errors emitted while building a controller.
#[derive(Debug)]
pub enum ControllerBuildError {
    /// Service name was empty.
    EmptyServiceName,
    /// Config file load failed while building.
    ConfigLoad(ConfigLoadError),
    /// Initially-enabled controller failed to create first generation.
    InitialEnable(EnableError),
}

impl std::fmt::Display for ControllerBuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyServiceName => write!(f, "service_name cannot be empty"),
            Self::ConfigLoad(err) => write!(f, "failed to load config for build: {err}"),
            Self::InitialEnable(err) => write!(f, "failed to start initial generation: {err}"),
        }
    }
}

impl std::error::Error for ControllerBuildError {}

/// Errors emitted while reloading controller TOML config.
#[derive(Debug)]
pub enum ReloadConfigError {
    /// Reload requested but no config path is configured.
    MissingConfigPath,
    /// Loading/parsing TOML config failed.
    Load(ConfigLoadError),
}

impl std::fmt::Display for ReloadConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingConfigPath => write!(f, "controller has no config_path; cannot reload"),
            Self::Load(err) => write!(f, "failed to reload controller config: {err}"),
        }
    }
}

impl std::error::Error for ReloadConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::MissingConfigPath => None,
            Self::Load(err) => Some(err),
        }
    }
}

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
    /// Runtime sampler was enabled but no Tokio runtime was active.
    MissingTokioRuntimeForSampler,
    /// Runtime sampler failed to start for this generation.
    StartRuntimeSampler(SamplerStartError),
}

impl std::fmt::Display for EnableError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyActive { generation_id } => {
                write!(f, "generation {generation_id} is already active")
            }
            Self::Build(err) => write!(f, "failed to build generation run: {err}"),
            Self::MissingTokioRuntimeForSampler => {
                write!(f, "runtime sampler requires an active Tokio runtime")
            }
            Self::StartRuntimeSampler(err) => {
                write!(f, "failed to start runtime sampler for generation: {err}")
            }
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
    use std::time::Duration;

    use super::{
        DisableOutcome, EnableError, GenerationState, RunEndPolicy, RuntimeSamplerTemplate,
        TailtriageController,
    };
    use tailtriage_core::{CaptureLimitsOverride, CaptureMode, RequestOptions, Run};

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

    fn read_run(path: &std::path::Path) -> Run {
        let artifact = read_artifact(path);
        serde_json::from_str(&artifact).expect("artifact should parse as Run")
    }

    fn test_config_path(base: &str) -> std::path::PathBuf {
        let unique = format!(
            "tailtriage-controller-config-{base}-{}-{}.toml",
            std::process::id(),
            tailtriage_core::unix_time_ms()
        );
        std::env::temp_dir().join(unique)
    }

    fn write_config(
        path: &std::path::Path,
        output: &std::path::Path,
        mode: &str,
        strict: bool,
        sampler_enabled: bool,
    ) {
        let content = format!(
            r#"[controller]
initially_enabled = false

[controller.activation]
mode = "{mode}"
strict_lifecycle = {strict}

[controller.activation.capture_limits_override]
max_requests = 17
max_stages = 18

[controller.activation.sink]
type = "local_json"
output_path = "{}"

[controller.activation.runtime_sampler]
enabled_for_armed_runs = {sampler_enabled}
mode_override = "investigation"
interval_ms = 250
max_runtime_snapshots = 123

[controller.activation.run_end_policy]
kind = "auto_seal_on_limits_hit"
"#,
            output.display()
        );
        fs::write(path, content).expect("config write should succeed");
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
    fn initially_enabled_build_starts_first_active_generation() {
        let output = test_output("initially-enabled");
        let controller = TailtriageController::builder("checkout-service")
            .initially_enabled(true)
            .output(&output)
            .build()
            .expect("build should succeed");

        let status = controller.status();
        let active = match status.generation {
            GenerationState::Active(active) => active,
            disabled @ GenerationState::Disabled { .. } => {
                panic!("expected active generation after build, got {disabled:?}")
            }
        };
        assert_eq!(active.generation_id, 1);

        assert!(matches!(
            controller.disable(),
            Ok(DisableOutcome::Finalized { generation_id: 1 })
        ));
        fs::remove_file(active.artifact_path).expect("cleanup should succeed");
    }

    #[test]
    fn disabled_status_reports_next_generation() {
        let controller = TailtriageController::builder("checkout-service")
            .build()
            .expect("build should succeed");

        assert!(matches!(
            controller.status().generation,
            GenerationState::Disabled { next_generation: 1 }
        ));
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
    fn default_policy_preserves_cheap_drop_after_saturation() {
        let output = test_output("default-policy-cheap-drop");
        let controller = TailtriageController::builder("checkout-service")
            .output(&output)
            .capture_limits_override(CaptureLimitsOverride {
                max_requests: Some(1),
                ..CaptureLimitsOverride::default()
            })
            .build()
            .expect("build should succeed");

        let active = controller.enable().expect("enable should succeed");
        controller.begin_request("/checkout").completion.finish_ok();
        controller.begin_request("/checkout").completion.finish_ok();
        controller.begin_request("/checkout").completion.finish_ok();

        let status = controller.status();
        let GenerationState::Active(active_status) = status.generation else {
            panic!("default policy should keep generation active after saturation");
        };
        assert!(active_status.accepting_new_admissions);
        assert!(!active_status.closing);

        assert!(matches!(
            controller.disable(),
            Ok(DisableOutcome::Finalized { generation_id }) if generation_id == active.generation_id
        ));

        let run = read_run(&active.artifact_path);
        assert!(run.truncation.limits_hit);
        assert_eq!(run.truncation.dropped_requests, 2);
        assert_eq!(
            run.metadata.run_end_reason,
            Some(tailtriage_core::RunEndReason::ManualDisarm)
        );

        fs::remove_file(active.artifact_path).expect("cleanup should succeed");
    }

    #[test]
    fn auto_seal_policy_ends_generation_after_limits_hit() {
        let output = test_output("auto-seal-policy");
        let controller = TailtriageController::builder("checkout-service")
            .output(&output)
            .run_end_policy(RunEndPolicy::AutoSealOnLimitsHit)
            .capture_limits_override(CaptureLimitsOverride {
                max_requests: Some(1),
                ..CaptureLimitsOverride::default()
            })
            .build()
            .expect("build should succeed");

        let active = controller.enable().expect("enable should succeed");
        controller.begin_request("/checkout").completion.finish_ok();
        controller.begin_request("/checkout").completion.finish_ok();

        let status = controller.status();
        assert!(matches!(
            status.generation,
            GenerationState::Disabled { next_generation: 2 }
        ));

        let run = read_run(&active.artifact_path);
        assert!(run.truncation.limits_hit);
        assert!(run.truncation.dropped_requests > 0);
        assert_eq!(
            run.metadata.run_end_reason,
            Some(tailtriage_core::RunEndReason::AutoSealOnLimitsHit)
        );

        fs::remove_file(active.artifact_path).expect("cleanup should succeed");
    }

    #[test]
    fn auto_seal_then_next_enable_creates_fresh_generation() {
        let output = test_output("auto-seal-next-generation");
        let controller = TailtriageController::builder("checkout-service")
            .output(&output)
            .run_end_policy(RunEndPolicy::AutoSealOnLimitsHit)
            .capture_limits_override(CaptureLimitsOverride {
                max_requests: Some(1),
                ..CaptureLimitsOverride::default()
            })
            .build()
            .expect("build should succeed");

        let first = controller.enable().expect("first enable should succeed");
        controller.begin_request("/checkout").completion.finish_ok();
        controller.begin_request("/checkout").completion.finish_ok();
        assert!(matches!(
            controller.status().generation,
            GenerationState::Disabled { next_generation: 2 }
        ));

        let second = controller.enable().expect("second enable should succeed");
        assert_eq!(second.generation_id, first.generation_id + 1);
        controller.begin_request("/checkout").completion.finish_ok();
        assert!(matches!(
            controller.disable(),
            Ok(DisableOutcome::Finalized { generation_id }) if generation_id == second.generation_id
        ));

        fs::remove_file(first.artifact_path).expect("cleanup first should succeed");
        fs::remove_file(second.artifact_path).expect("cleanup second should succeed");
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
        assert_eq!(disabled_started.handle.request_id(), "req-disabled");
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

        assert_eq!(started.handle.request_id(), "req-disabled-noop");
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

    #[test]
    fn toml_parsing_success_and_failure() {
        let output = test_output("toml-parse");
        let config = test_config_path("toml-parse");
        write_config(&config, &output, "light", false, true);

        let loaded =
            TailtriageController::load_config_from_path(&config).expect("valid TOML should parse");
        assert_eq!(loaded.activation_template.selected_mode, CaptureMode::Light);
        assert_eq!(
            loaded.activation_template.capture_limits_override,
            CaptureLimitsOverride {
                max_requests: Some(17),
                max_stages: Some(18),
                max_queues: None,
                max_inflight_snapshots: None,
                max_runtime_snapshots: None,
            }
        );
        assert!(
            loaded
                .activation_template
                .runtime_sampler
                .enabled_for_armed_runs
        );
        assert_eq!(
            loaded.activation_template.run_end_policy,
            RunEndPolicy::AutoSealOnLimitsHit
        );

        fs::write(&config, "[controller\n").expect("invalid TOML write should succeed");
        assert!(TailtriageController::load_config_from_path(&config).is_err());

        fs::remove_file(config).expect("config cleanup should succeed");
    }

    #[test]
    fn reload_updates_next_activation_template_only() {
        let output_before = test_output("reload-template-before");
        let output_after = test_output("reload-template-after");
        let config = test_config_path("reload-template");
        write_config(&config, &output_before, "light", false, false);

        let controller = TailtriageController::builder("checkout-service")
            .config_path(&config)
            .build()
            .expect("build should succeed");
        assert_eq!(
            controller.status().template.selected_mode,
            CaptureMode::Light
        );

        write_config(&config, &output_after, "investigation", true, false);
        controller.reload_config().expect("reload should succeed");

        let status = controller.status();
        assert_eq!(status.template.selected_mode, CaptureMode::Investigation);
        assert!(status.template.strict_lifecycle);
        assert_eq!(
            status.template.run_end_policy,
            RunEndPolicy::AutoSealOnLimitsHit
        );

        fs::remove_file(config).expect("config cleanup should succeed");
    }

    #[test]
    fn active_generation_keeps_original_config_after_reload() {
        let output_before = test_output("active-keeps-before");
        let output_after = test_output("active-keeps-after");
        let config = test_config_path("active-keeps");
        write_config(&config, &output_before, "light", false, false);

        let controller = TailtriageController::builder("checkout-service")
            .config_path(&config)
            .build()
            .expect("build should succeed");

        let gen1 = controller.enable().expect("first enable should succeed");
        assert_eq!(gen1.activation_config.selected_mode, CaptureMode::Light);
        assert_eq!(
            gen1.activation_config.sink_template,
            super::ControllerSinkTemplate::LocalJson {
                output_path: output_before.clone()
            }
        );

        write_config(&config, &output_after, "investigation", true, false);
        controller.reload_config().expect("reload should succeed");

        let GenerationState::Active(active_after_reload) = controller.status().generation else {
            panic!("expected active generation");
        };
        assert_eq!(
            active_after_reload.activation_config.selected_mode,
            CaptureMode::Light
        );
        assert!(!active_after_reload.activation_config.strict_lifecycle);

        let started = controller.begin_request("/checkout");
        started.completion.finish_ok();
        assert!(matches!(
            controller.disable(),
            Ok(DisableOutcome::Finalized { generation_id }) if generation_id == gen1.generation_id
        ));

        let gen2 = controller.enable().expect("second enable should succeed");
        assert_eq!(
            gen2.activation_config.selected_mode,
            CaptureMode::Investigation
        );
        assert!(gen2.activation_config.strict_lifecycle);
        assert_eq!(
            gen2.activation_config.sink_template,
            super::ControllerSinkTemplate::LocalJson {
                output_path: output_after.clone()
            }
        );

        assert!(matches!(
            controller.disable(),
            Ok(DisableOutcome::Finalized { generation_id }) if generation_id == gen2.generation_id
        ));

        fs::remove_file(gen1.artifact_path).expect("cleanup gen1 should succeed");
        fs::remove_file(gen2.artifact_path).expect("cleanup gen2 should succeed");
        fs::remove_file(config).expect("config cleanup should succeed");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn armed_generation_with_sampler_enabled_records_effective_metadata() {
        let output = test_output("sampler-enabled");
        let controller = TailtriageController::builder("checkout-service")
            .output(&output)
            .runtime_sampler(RuntimeSamplerTemplate {
                enabled_for_armed_runs: true,
                mode_override: Some(CaptureMode::Investigation),
                interval_ms: Some(15),
                max_runtime_snapshots: Some(8),
            })
            .capture_limits_override(CaptureLimitsOverride {
                max_runtime_snapshots: Some(3),
                ..CaptureLimitsOverride::default()
            })
            .build()
            .expect("build should succeed");

        let active = controller.enable().expect("enable should succeed");
        tokio::time::sleep(Duration::from_millis(40)).await;
        assert!(matches!(
            controller.disable(),
            Ok(DisableOutcome::Finalized { generation_id }) if generation_id == active.generation_id
        ));

        let run = read_run(&active.artifact_path);
        let config = run
            .metadata
            .effective_tokio_sampler_config
            .expect("sampler metadata should be set");
        assert_eq!(config.inherited_mode, CaptureMode::Light);
        assert_eq!(
            config.explicit_mode_override,
            Some(CaptureMode::Investigation)
        );
        assert_eq!(config.resolved_mode, CaptureMode::Investigation);
        assert_eq!(config.resolved_sampler_cadence_ms, 15);
        assert_eq!(config.resolved_runtime_snapshot_retention, 3);

        fs::remove_file(active.artifact_path).expect("cleanup should succeed");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn armed_generation_with_sampler_disabled_keeps_sampler_metadata_empty() {
        let output = test_output("sampler-disabled");
        let controller = TailtriageController::builder("checkout-service")
            .output(&output)
            .runtime_sampler(RuntimeSamplerTemplate {
                enabled_for_armed_runs: false,
                mode_override: Some(CaptureMode::Investigation),
                interval_ms: Some(5),
                max_runtime_snapshots: Some(100),
            })
            .build()
            .expect("build should succeed");

        let active = controller.enable().expect("enable should succeed");
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(matches!(
            controller.disable(),
            Ok(DisableOutcome::Finalized { generation_id }) if generation_id == active.generation_id
        ));

        let run = read_run(&active.artifact_path);
        assert!(run.metadata.effective_tokio_sampler_config.is_none());
        assert!(run.runtime_snapshots.is_empty());

        fs::remove_file(active.artifact_path).expect("cleanup should succeed");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn sampler_stops_on_disarm_and_reenable_uses_fresh_generation_sampler_lifecycle() {
        let output = test_output("sampler-reenable");
        let controller = TailtriageController::builder("checkout-service")
            .output(&output)
            .runtime_sampler(RuntimeSamplerTemplate {
                enabled_for_armed_runs: true,
                mode_override: None,
                interval_ms: Some(10),
                max_runtime_snapshots: Some(32),
            })
            .build()
            .expect("build should succeed");

        let first = controller.enable().expect("first enable should succeed");
        tokio::time::sleep(Duration::from_millis(35)).await;
        assert!(matches!(
            controller.disable(),
            Ok(DisableOutcome::Finalized { generation_id }) if generation_id == first.generation_id
        ));
        tokio::time::sleep(Duration::from_millis(30)).await;

        let first_run = read_run(&first.artifact_path);
        assert!(!first_run.runtime_snapshots.is_empty());
        let first_metadata = first_run
            .metadata
            .effective_tokio_sampler_config
            .expect("first generation sampler metadata should exist");

        let second = controller.enable().expect("second enable should succeed");
        assert_eq!(second.generation_id, first.generation_id + 1);
        tokio::time::sleep(Duration::from_millis(35)).await;
        assert!(matches!(
            controller.disable(),
            Ok(DisableOutcome::Finalized { generation_id }) if generation_id == second.generation_id
        ));

        let second_run = read_run(&second.artifact_path);
        assert!(!second_run.runtime_snapshots.is_empty());
        let second_metadata = second_run
            .metadata
            .effective_tokio_sampler_config
            .expect("second generation sampler metadata should exist");

        assert_eq!(first_metadata.resolved_sampler_cadence_ms, 10);
        assert_eq!(second_metadata.resolved_sampler_cadence_ms, 10);
        assert_ne!(first.artifact_path, second.artifact_path);

        fs::remove_file(first.artifact_path).expect("cleanup first should succeed");
        fs::remove_file(second.artifact_path).expect("cleanup second should succeed");
    }
}
