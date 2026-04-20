#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

//! Long-lived capture control layer for repeated bounded tailtriage activations.
//!
//! Layering:
//!
//! - [`tailtriage_core`] remains the per-run collector and artifact model.
//! - `tailtriage-controller` provides control-layer scaffolding for live arm/disarm
//!   workflows that will create fresh bounded runs on activation.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use tailtriage_core::{unix_time_ms, CaptureMode};

/// Builder for a long-lived [`TailtriageController`].
///
/// Unlike [`tailtriage_core::TailtriageBuilder`], this builder configures controller-level
/// scaffolding for repeated bounded capture activations over process lifetime.
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

    /// Sets whether the controller starts in an enabled state.
    #[must_use]
    pub const fn initially_enabled(mut self, initially_enabled: bool) -> Self {
        self.initially_enabled = initially_enabled;
        self
    }

    /// Sets the output location for future activation runs.
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

    /// Builds the controller scaffolding.
    ///
    /// This method intentionally does not start any capture run.
    ///
    /// # Errors
    ///
    /// Returns [`ControllerBuildError::EmptyServiceName`] when `service_name` is blank.
    pub fn build(self) -> Result<TailtriageController, ControllerBuildError> {
        if self.service_name.trim().is_empty() {
            return Err(ControllerBuildError::EmptyServiceName);
        }

        let next_generation = 1;
        let lifecycle = if self.initially_enabled {
            ControllerLifecycle::EnabledIdle { next_generation }
        } else {
            ControllerLifecycle::Disabled { next_generation }
        };

        Ok(TailtriageController {
            template: Mutex::new(TailtriageControllerTemplate {
                service_name: self.service_name,
                config_path: self.config_path,
                sink_template: self.sink_template,
                selected_mode: CaptureMode::Light,
                run_end_policy: self.run_end_policy,
            }),
            lifecycle: Mutex::new(lifecycle),
        })
    }
}

/// Long-lived live-capture controller scaffolding.
///
/// Lifecycle intent:
///
/// - The controller object is long-lived.
/// - Each activation is expected to start a fresh bounded capture run.
/// - At most one generation may be active at a time.
/// - Reload updates controller template for the *next* activation only.
///   It does not mutate active-generation config.
#[derive(Debug)]
pub struct TailtriageController {
    template: Mutex<TailtriageControllerTemplate>,
    lifecycle: Mutex<ControllerLifecycle>,
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
    /// Panics if the internal controller mutexes are poisoned.
    #[must_use]
    pub fn status(&self) -> TailtriageControllerStatus {
        let template = self
            .template
            .lock()
            .expect("controller template lock poisoned");
        let lifecycle = self
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
    /// Reload is intentionally non-invasive for an active run: active generation
    /// state remains unchanged and new template values apply only to future
    /// activations.
    ///
    /// # Panics
    ///
    /// Panics if the internal template mutex is poisoned.
    pub fn reload_template(&self, next_template: TailtriageControllerTemplate) {
        let mut template = self
            .template
            .lock()
            .expect("controller template lock poisoned");
        *template = next_template;
    }

    /// Marks the start of one activation generation in controller state.
    ///
    /// This scaffolding method enforces the one-active-generation invariant but
    /// intentionally does not create or manage a [`tailtriage_core::Tailtriage`] run yet.
    ///
    /// # Errors
    ///
    /// Returns [`StartGenerationError::ControllerDisabled`] when disarmed, and
    /// [`StartGenerationError::AlreadyActive`] when a generation is already active.
    ///
    /// # Panics
    ///
    /// Panics if the internal lifecycle mutex is poisoned.
    pub fn start_generation(&self) -> Result<GenerationState, StartGenerationError> {
        let mut lifecycle = self
            .lifecycle
            .lock()
            .expect("controller lifecycle lock poisoned");

        let started = match *lifecycle {
            ControllerLifecycle::Disabled { .. } => {
                return Err(StartGenerationError::ControllerDisabled);
            }
            ControllerLifecycle::EnabledIdle {
                ref mut next_generation,
            } => {
                let generation_id = *next_generation;
                *next_generation = next_generation.saturating_add(1);
                let active = ActiveGenerationState {
                    generation_id,
                    started_at_unix_ms: unix_time_ms(),
                };
                *lifecycle = ControllerLifecycle::Active {
                    active,
                    next_generation: *next_generation,
                };
                GenerationState::Active(active)
            }
            ControllerLifecycle::Active { active, .. } => {
                return Err(StartGenerationError::AlreadyActive {
                    generation_id: active.generation_id,
                });
            }
        };

        Ok(started)
    }

    /// Marks the active generation as finished when IDs match.
    ///
    /// Returns `true` when an active generation was cleared.
    ///
    /// # Panics
    ///
    /// Panics if the internal lifecycle mutex is poisoned.
    #[must_use]
    pub fn finish_generation(&self, generation_id: u64) -> bool {
        let mut lifecycle = self
            .lifecycle
            .lock()
            .expect("controller lifecycle lock poisoned");

        let ControllerLifecycle::Active {
            active,
            next_generation,
        } = *lifecycle
        else {
            return false;
        };

        if active.generation_id != generation_id {
            return false;
        }

        *lifecycle = ControllerLifecycle::EnabledIdle { next_generation };
        true
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
        /// Destination artifact path for each generated run.
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActiveGenerationState {
    /// Monotonic generation identifier.
    pub generation_id: u64,
    /// Activation start timestamp.
    pub started_at_unix_ms: u64,
}

#[derive(Debug, Clone, Copy)]
enum ControllerLifecycle {
    Disabled {
        /// Next generation ID that would be assigned on activation.
        next_generation: u64,
    },
    EnabledIdle {
        /// Next generation ID that will be assigned on activation.
        next_generation: u64,
    },
    Active {
        active: ActiveGenerationState,
        next_generation: u64,
    },
}

impl ControllerLifecycle {
    fn snapshot(self) -> GenerationState {
        match self {
            Self::Disabled { next_generation } => GenerationState::Disabled { next_generation },
            Self::EnabledIdle { next_generation } => {
                GenerationState::EnabledIdle { next_generation }
            }
            Self::Active { active, .. } => GenerationState::Active(active),
        }
    }
}

/// Errors emitted while building a controller scaffold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControllerBuildError {
    /// Service name was empty.
    EmptyServiceName,
}

impl std::fmt::Display for ControllerBuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyServiceName => write!(f, "service_name cannot be empty"),
        }
    }
}

impl std::error::Error for ControllerBuildError {}

/// Errors emitted when transitioning into active-generation state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartGenerationError {
    /// Controller is currently disabled/disarmed.
    ControllerDisabled,
    /// Another generation is already active.
    AlreadyActive {
        /// ID of the active generation blocking a new start.
        generation_id: u64,
    },
}

impl std::fmt::Display for StartGenerationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ControllerDisabled => write!(f, "controller is disabled"),
            Self::AlreadyActive { generation_id } => {
                write!(f, "generation {generation_id} is already active")
            }
        }
    }
}

impl std::error::Error for StartGenerationError {}

#[cfg(test)]
mod tests {
    use super::{
        ControllerSinkTemplate, GenerationState, RunEndPolicy, StartGenerationError,
        TailtriageController,
    };

    #[test]
    fn builder_defaults_are_stable() {
        let controller = TailtriageController::builder("checkout-service")
            .build()
            .expect("build should succeed");

        let status = controller.status();
        assert_eq!(status.service_name, "checkout-service");
        assert_eq!(status.config_path, None);
        assert_eq!(status.selected_mode, tailtriage_core::CaptureMode::Light);
        assert_eq!(status.run_end_policy, RunEndPolicy::Manual);
        assert_eq!(
            status.sink_template,
            ControllerSinkTemplate::LocalJson {
                output_path: "tailtriage-run.json".into()
            }
        );
        assert_eq!(
            status.generation,
            GenerationState::Disabled { next_generation: 1 }
        );
    }

    #[test]
    fn status_defaults_for_initially_enabled_controller() {
        let controller = TailtriageController::builder("checkout-service")
            .initially_enabled(true)
            .build()
            .expect("build should succeed");

        let status = controller.status();
        assert_eq!(
            status.generation,
            GenerationState::EnabledIdle { next_generation: 1 }
        );
    }

    #[test]
    fn only_one_generation_can_be_active() {
        let controller = TailtriageController::builder("checkout-service")
            .initially_enabled(true)
            .build()
            .expect("build should succeed");

        let first = controller
            .start_generation()
            .expect("first generation should start");
        let GenerationState::Active(active) = first else {
            panic!("first generation should be active");
        };

        let err = controller
            .start_generation()
            .expect_err("second start should fail while first active");
        assert_eq!(
            err,
            StartGenerationError::AlreadyActive {
                generation_id: active.generation_id
            }
        );

        assert!(controller.finish_generation(active.generation_id));
        assert!(matches!(
            controller.status().generation,
            GenerationState::EnabledIdle { next_generation: 2 }
        ));
    }
}
