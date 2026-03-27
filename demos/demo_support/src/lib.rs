use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use tailtriage_core::Tailtriage;
use tokio::sync::Barrier;

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
    let mut args = std::env::args().skip(1);
    let output_path = args
        .next()
        .map_or_else(|| PathBuf::from(default_output_path), PathBuf::from);
    let mode = DemoMode::from_arg(args.next().as_ref())?;
    ensure_parent_dir(&output_path)?;

    Ok(DemoArgs { output_path, mode })
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
