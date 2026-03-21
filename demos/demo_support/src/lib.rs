use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context;
use tailtriage_core::Tailtriage;

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

/// Build a Tailtriage collector for a demo service name and output artifact path.
pub fn init_collector(service_name: &str, output_path: &Path) -> anyhow::Result<Arc<Tailtriage>> {
    let collector = Tailtriage::builder(service_name)
        .output(output_path)
        .build()?;
    Ok(Arc::new(collector))
}
