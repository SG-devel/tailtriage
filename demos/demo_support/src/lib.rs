use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use tailtriage_core::Tailtriage;
use tailtriage_core::{Outcome, RequestOptions};
use tailtriage_tracing::TracingRecorder;
use tokio::sync::Barrier;
use tracing::Instrument;
use tracing_subscriber::prelude::*;

/// Demo profile selector used by before/after style demo binaries.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DemoMode {
    /// Run the baseline or "before" profile.
    Baseline,
    /// Run the mitigated or "after" profile.
    Mitigated,
}

impl DemoMode {
    /// Parse a mode argument.
    ///
    /// Accepted values:
    /// - `baseline` or `before`
    /// - `mitigated` or `after`
    ///
    /// If omitted, defaults to `baseline`.
    ///
    /// # Errors
    ///
    /// Returns an error if `value` is present but is not one of:
    /// `baseline`, `before`, `mitigated`, or `after`.
    pub fn from_arg(value: Option<&String>) -> anyhow::Result<Self> {
        match value.map(String::as_str) {
            None | Some("baseline" | "before") => Ok(Self::Baseline),
            Some("mitigated" | "after") => Ok(Self::Mitigated),
            Some(other) => anyhow::bail!(
                "unsupported mode '{other}', expected one of: baseline, before, mitigated, after"
            ),
        }
    }
}

/// Parsed common demo CLI arguments.
pub struct DemoArgs {
    /// Output path for the generated demo artifact.
    pub output_path: PathBuf,
    /// Selected demo mode.
    pub mode: DemoMode,
    /// Instrumentation backend for the demo run.
    pub instrumentation: InstrumentationMode,
}

/// Instrumentation backend selector for demos.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstrumentationMode {
    Native,
    Tracing,
}

impl InstrumentationMode {
    /// Parses the `--instrumentation` value.
    ///
    /// # Errors
    ///
    /// Returns an error for unsupported values.
    pub fn from_arg(value: Option<&str>) -> anyhow::Result<Self> {
        match value {
            None | Some("native") => Ok(Self::Native),
            Some("tracing") => Ok(Self::Tracing),
            Some(other) => anyhow::bail!(
                "unsupported instrumentation '{other}', expected one of: native, tracing"
            ),
        }
    }
}

/// Parse common `<output_path> [mode]` demo arguments.
///
/// The first positional argument, if present, is parsed as the output path.
/// Otherwise, `default_output_path` is used.
///
/// The second positional argument, if present, is parsed as the demo mode.
/// Accepted values are `baseline`/`before` and `mitigated`/`after`.
/// If omitted, the mode defaults to `baseline`.
///
/// # Errors
///
/// Returns an error if the mode argument is unsupported, or if preparing the
/// parent directory for the output path fails.
pub fn parse_demo_args(default_output_path: &str) -> anyhow::Result<DemoArgs> {
    let mut args = std::env::args().skip(1).peekable();
    let output_path = args
        .next_if(|arg| !arg.starts_with("--"))
        .map_or_else(|| PathBuf::from(default_output_path), PathBuf::from);
    let mode = DemoMode::from_arg(args.next_if(|arg| !arg.starts_with("--")).as_ref())?;
    let mut instrumentation = InstrumentationMode::Native;
    while let Some(flag) = args.next() {
        if flag != "--instrumentation" {
            anyhow::bail!(
                "unsupported argument '{flag}', expected '--instrumentation <native|tracing>'"
            );
        }
        instrumentation = InstrumentationMode::from_arg(args.next().as_deref())?;
    }
    ensure_parent_dir(&output_path)?;

    Ok(DemoArgs {
        output_path,
        mode,
        instrumentation,
    })
}

/// Parse a common `<output_path>` demo argument.
///
/// The first positional argument, if present, is used as the output path.
/// Otherwise, `default_output_path` is used.
///
/// # Errors
///
/// Returns an error if preparing the parent directory for the resolved output
/// path fails.
pub fn parse_output_arg(default_output_path: &str) -> anyhow::Result<PathBuf> {
    let output_path = std::env::args()
        .nth(1)
        .map_or_else(|| PathBuf::from(default_output_path), PathBuf::from);
    ensure_parent_dir(&output_path)?;
    Ok(output_path)
}

fn ensure_parent_dir(output_path: &Path) -> anyhow::Result<()> {
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create artifact directory {}", parent.display()))?;
    }
    Ok(())
}

/// Initialize a shared `Tailtriage` collector for the given service and output path.
///
/// The collector is configured with `service_name` and writes its output to
/// `output_path`.
///
/// # Errors
///
/// Returns an error if building the `Tailtriage` collector fails.
pub fn init_collector(service_name: &str, output_path: &Path) -> anyhow::Result<Arc<Tailtriage>> {
    let collector = Tailtriage::builder(service_name)
        .output(output_path)
        .build()?;
    Ok(Arc::new(collector))
}

pub enum DemoRecorder {
    Native(Arc<Tailtriage>),
    Tracing(TracingRecorder),
}

impl DemoRecorder {
    /// Builds a recorder backend for the requested instrumentation mode.
    ///
    /// # Errors
    ///
    /// Returns an error if backend initialization fails.
    pub fn new(
        service_name: &str,
        output_path: &Path,
        instrumentation: InstrumentationMode,
    ) -> anyhow::Result<Self> {
        match instrumentation {
            InstrumentationMode::Native => {
                Ok(Self::Native(init_collector(service_name, output_path)?))
            }
            InstrumentationMode::Tracing => {
                let recorder = TracingRecorder::builder(service_name).strict(false).build();
                let subscriber = tracing_subscriber::registry().with(recorder.layer());
                tracing::subscriber::set_global_default(subscriber).map_err(|err| {
                    anyhow::anyhow!("failed to install tracing subscriber: {err}")
                })?;
                Ok(Self::Tracing(recorder))
            }
        }
    }

    pub fn start_request(&self, route: &str, request_id: &str) -> DemoRequest {
        match self {
            Self::Native(collector) => {
                let started = collector.begin_request_with_owned(
                    route,
                    RequestOptions::new().request_id(request_id.to_owned()),
                );
                DemoRequest::Native(started)
            }
            Self::Tracing(_) => DemoRequest::Tracing {
                request_id: request_id.to_owned(),
                request_span: tracing::info_span!(
                    "request",
                    tt.kind = "request",
                    tt.request_id = request_id,
                    tt.route = route
                ),
            },
        }
    }

    /// Shuts down recording and writes run JSON to `output_path`.
    ///
    /// # Errors
    ///
    /// Returns an error if shutdown or JSON writing fails.
    pub fn shutdown(self, output_path: &Path) -> anyhow::Result<()> {
        match self {
            Self::Native(collector) => {
                collector.shutdown()?;
                Ok(())
            }
            Self::Tracing(recorder) => {
                let imported = recorder.shutdown()?;
                std::fs::write(output_path, serde_json::to_vec_pretty(imported.run())?)?;
                Ok(())
            }
        }
    }
}

pub enum DemoRequest {
    Native(tailtriage_core::OwnedStartedRequest),
    Tracing {
        request_id: String,
        request_span: tracing::Span,
    },
}

impl DemoRequest {
    pub async fn queue_wait<F, T>(&self, queue: &str, depth_at_start: Option<u64>, fut: F) -> T
    where
        F: Future<Output = T>,
    {
        match self {
            Self::Native(started) => {
                let timer = started.handle.queue(queue);
                match depth_at_start {
                    Some(depth) => timer.with_depth_at_start(depth).await_on(fut).await,
                    None => timer.await_on(fut).await,
                }
            }
            Self::Tracing { request_id, .. } => {
                let span = if let Some(depth) = depth_at_start {
                    tracing::info_span!(
                        "queue",
                        tt.kind = "queue",
                        tt.request_id = request_id.as_str(),
                        tt.queue = queue,
                        tt.depth_at_start = depth
                    )
                } else {
                    tracing::info_span!(
                        "queue",
                        tt.kind = "queue",
                        tt.request_id = request_id.as_str(),
                        tt.queue = queue
                    )
                };
                fut.instrument(span).await
            }
        }
    }

    pub async fn stage_value<F, T>(&self, stage: &str, fut: F) -> T
    where
        F: Future<Output = T>,
    {
        match self {
            Self::Native(started) => started.handle.stage(stage).await_value(fut).await,
            Self::Tracing { request_id, .. } => {
                let span = tracing::info_span!(
                    "stage",
                    tt.kind = "stage",
                    tt.request_id = request_id.as_str(),
                    tt.stage = stage,
                    tt.success = true
                );
                fut.instrument(span).await
            }
        }
    }

    pub fn finish(self, outcome: Outcome) {
        match self {
            Self::Native(started) => started.completion.finish(outcome),
            Self::Tracing { request_span, .. } => {
                request_span.record("tt.outcome", outcome.as_str());
            }
        }
    }
}

/// Shared synchronized start gate for a request cohort.
///
/// This helps demos avoid ad-hoc burst pacing and start measured work at
/// roughly the same time across request tasks.
#[derive(Clone)]
pub struct CohortStart {
    barrier: Arc<Barrier>,
}

impl CohortStart {
    /// Create a cohort barrier for `participant_count` async tasks.
    #[must_use]
    pub fn new(participant_count: usize) -> Self {
        Self {
            barrier: Arc::new(Barrier::new(participant_count)),
        }
    }

    /// Wait for all participants before entering measured work.
    pub async fn wait(&self) {
        self.barrier.wait().await;
    }
}

/// Run a warmup phase followed by a measured phase.
///
/// This utility keeps demo shaping consistent when services need runtime
/// warmup before collecting artifact-relevant measured requests.
pub async fn run_warmup_then_measured<Warmup, WarmupFut, Measured, MeasuredFut>(
    warmup_requests: usize,
    warmup_phase: Warmup,
    measured_phase: Measured,
) where
    Warmup: FnOnce() -> WarmupFut,
    WarmupFut: std::future::Future<Output = ()>,
    Measured: FnOnce() -> MeasuredFut,
    MeasuredFut: std::future::Future<Output = ()>,
{
    if warmup_requests > 0 {
        warmup_phase().await;
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    measured_phase().await;
}

#[cfg(test)]
mod tests {
    use super::InstrumentationMode;

    #[test]
    fn instrumentation_mode_defaults_to_native() {
        assert_eq!(
            InstrumentationMode::from_arg(None).expect("default should parse"),
            InstrumentationMode::Native
        );
    }

    #[test]
    fn instrumentation_mode_accepts_tracing() {
        assert_eq!(
            InstrumentationMode::from_arg(Some("tracing")).expect("tracing should parse"),
            InstrumentationMode::Tracing
        );
    }
}
