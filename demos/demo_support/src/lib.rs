use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use tailtriage_core::Tailtriage;
use tokio::sync::Barrier;

/// Demo profile selector used by before/after style demo binaries.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DemoMode {
    Baseline,
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
    pub fn from_arg(value: Option<String>) -> anyhow::Result<Self> {
        match value.as_deref() {
            None | Some("baseline") | Some("before") => Ok(Self::Baseline),
            Some("mitigated") | Some("after") => Ok(Self::Mitigated),
            Some(other) => anyhow::bail!(
                "unsupported mode '{other}', expected one of: baseline, before, mitigated, after"
            ),
        }
    }
}

pub struct DemoArgs {
    pub output_path: PathBuf,
    pub mode: DemoMode,
}

/// Parse common `<output_path> [mode]` demo arguments.
pub fn parse_demo_args(default_output_path: &str) -> anyhow::Result<DemoArgs> {
    let mut args = std::env::args().skip(1);
    let output_path = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(default_output_path));
    let mode = DemoMode::from_arg(args.next())?;
    ensure_parent_dir(&output_path)?;

    Ok(DemoArgs { output_path, mode })
}

/// Parse common `<output_path>` demo arguments.
pub fn parse_output_arg(default_output_path: &str) -> anyhow::Result<PathBuf> {
    let output_path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(default_output_path));
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

/// Build a Tailtriage instance for a demo service name and output artifact path.
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

/// Run a short warmup phase followed by a measured phase.
///
/// This utility keeps demo shaping consistent when services need runtime
/// warmup before collecting the artifact-relevant requests.
pub async fn run_warmup_then_measured<F, Fut>(
    warmup_requests: usize,
    measured_requests: usize,
    mut run_request: F,
) where
    F: FnMut(bool) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    for _ in 0..warmup_requests {
        run_request(false).await;
    }
    if warmup_requests > 0 {
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    for _ in 0..measured_requests {
        run_request(true).await;
    }
}
