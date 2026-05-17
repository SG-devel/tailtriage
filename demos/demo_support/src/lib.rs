use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use tailtriage_core::{Outcome, RequestOptions, Run, Tailtriage};
use tailtriage_tracing::TracingRecorder;
use tokio::sync::Barrier;
use tracing::Instrument;
use tracing_subscriber::layer::SubscriberExt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DemoMode {
    Baseline,
    Mitigated,
}
impl DemoMode {
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstrumentationMode {
    Native,
    Tracing,
}
impl InstrumentationMode {
    fn from_arg(value: &str) -> anyhow::Result<Self> {
        match value {
            "native" => Ok(Self::Native),
            "tracing" => Ok(Self::Tracing),
            other => anyhow::bail!(
                "unsupported instrumentation '{other}', expected one of: native, tracing"
            ),
        }
    }
}

pub struct DemoArgs {
    pub output_path: PathBuf,
    pub mode: DemoMode,
    pub instrumentation: InstrumentationMode,
}

pub fn parse_demo_args(default_output_path: &str) -> anyhow::Result<DemoArgs> {
    let mut output_path = PathBuf::from(default_output_path);
    let mut mode: Option<DemoMode> = None;
    let mut instrumentation = InstrumentationMode::Native;
    let mut positional: Vec<String> = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--instrumentation" {
            let value = args.next().context("missing value for --instrumentation")?;
            instrumentation = InstrumentationMode::from_arg(&value)?;
        } else {
            positional.push(arg);
        }
    }
    if let Some(path) = positional.first() {
        output_path = PathBuf::from(path);
    }
    if let Some(value) = positional.get(1) {
        mode = Some(DemoMode::from_arg(Some(value))?);
    }
    ensure_parent_dir(&output_path)?;
    Ok(DemoArgs {
        output_path,
        mode: mode.unwrap_or(DemoMode::Baseline),
        instrumentation,
    })
}

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

pub fn init_collector(service_name: &str, output_path: &Path) -> anyhow::Result<Arc<Tailtriage>> {
    let collector = Tailtriage::builder(service_name)
        .output(output_path)
        .build()?;
    Ok(Arc::new(collector))
}

#[derive(Clone)]
pub enum DemoInstrumentation {
    Native {
        collector: Arc<Tailtriage>,
    },
    Tracing {
        recorder: TracingRecorder,
        output_path: PathBuf,
    },
}
impl DemoInstrumentation {
    #[must_use]
    pub fn clone_for_task(&self) -> Self {
        self.clone()
    }
    pub fn new(
        service_name: &str,
        output_path: &Path,
        mode: InstrumentationMode,
    ) -> anyhow::Result<Self> {
        match mode {
            InstrumentationMode::Native => Ok(Self::Native {
                collector: init_collector(service_name, output_path)?,
            }),
            InstrumentationMode::Tracing => {
                let recorder = TracingRecorder::builder(service_name).build();
                let subscriber = tracing_subscriber::registry().with(recorder.layer());
                tracing::subscriber::set_global_default(subscriber).map_err(|err| {
                    anyhow::anyhow!("failed to install global tracing subscriber for demo: {err}")
                })?;
                Ok(Self::Tracing {
                    recorder,
                    output_path: output_path.to_path_buf(),
                })
            }
        }
    }

    pub async fn run_request<B, Fut>(
        &self,
        route: &'static str,
        request_id: String,
        outcome: Outcome,
        body: B,
    ) -> anyhow::Result<()>
    where
        B: FnOnce(DemoRequest) -> Fut,
        Fut: Future<Output = anyhow::Result<()>>,
    {
        match self {
            Self::Native { collector } => {
                let started = collector.begin_request_with_owned(
                    route,
                    RequestOptions::new().request_id(request_id.clone()),
                );
                let request = DemoRequest::Native(started.handle.clone());
                body(request).await?;
                started.completion.finish(outcome);
                Ok(())
            }
            Self::Tracing { .. } => {
                let outcome_label = match outcome {
                    Outcome::Ok => "ok",
                    Outcome::Error => "error",
                    Outcome::Timeout => "timeout",
                    Outcome::Cancelled => "cancelled",
                    Outcome::Rejected => "rejected",
                    Outcome::Other(_) => "other",
                };
                let span = tracing::info_span!(
                    "request",
                    tt.kind = "request",
                    tt.request_id = request_id,
                    tt.route = route,
                    tt.outcome = outcome_label
                );
                body(DemoRequest::Tracing { request_id })
                    .instrument(span)
                    .await?;
                Ok(())
            }
        }
    }

    pub fn shutdown(&self) -> anyhow::Result<()> {
        match self {
            Self::Native { collector } => collector.shutdown().map_err(Into::into),
            Self::Tracing {
                recorder,
                output_path,
            } => {
                let imported = recorder.shutdown()?;
                write_run(output_path, imported.run())
            }
        }
    }
}

pub enum DemoRequest {
    Native(tailtriage_core::OwnedRequestHandle),
    Tracing { request_id: String },
}
impl DemoRequest {
    pub fn inflight(&self, label: &'static str) -> Option<tailtriage_core::InflightGuard<'_>> {
        match self {
            Self::Native(req) => Some(req.inflight(label)),
            Self::Tracing { .. } => None,
        }
    }
    pub async fn queue_wait<Fut>(
        &self,
        queue: &'static str,
        depth_at_start: u64,
        fut: Fut,
    ) -> anyhow::Result<Fut::Output>
    where
        Fut: Future,
    {
        match self {
            Self::Native(req) => Ok(req
                .queue(queue)
                .with_depth_at_start(depth_at_start)
                .await_on(fut)
                .await),
            Self::Tracing { request_id } => {
                let span = tracing::info_span!(
                    "queue",
                    tt.kind = "queue",
                    tt.request_id = request_id.as_str(),
                    tt.queue = queue,
                    tt.depth_at_start = depth_at_start
                );
                Ok(fut.instrument(span).await)
            }
        }
    }
    pub async fn stage<Fut>(&self, stage: &'static str, fut: Fut) -> anyhow::Result<()>
    where
        Fut: Future<Output = ()>,
    {
        match self {
            Self::Native(req) => {
                req.stage(stage)
                    .await_on(async move {
                        fut.await;
                        Ok::<(), anyhow::Error>(())
                    })
                    .await?;
                Ok(())
            }
            Self::Tracing { request_id } => {
                let span = tracing::info_span!(
                    "stage",
                    tt.kind = "stage",
                    tt.request_id = request_id.as_str(),
                    tt.stage = stage,
                    tt.success = true
                );
                fut.instrument(span).await;
                Ok(())
            }
        }
    }
}

fn write_run(output_path: &Path, run: &Run) -> anyhow::Result<()> {
    let file = std::fs::File::create(output_path)
        .with_context(|| format!("failed to create run artifact {}", output_path.display()))?;
    serde_json::to_writer_pretty(file, run)
        .with_context(|| format!("failed to write run artifact {}", output_path.display()))?;
    Ok(())
}

#[derive(Clone)]
pub struct CohortStart {
    barrier: Arc<Barrier>,
}
impl CohortStart {
    #[must_use]
    pub fn new(participant_count: usize) -> Self {
        Self {
            barrier: Arc::new(Barrier::new(participant_count)),
        }
    }
    pub async fn wait(&self) {
        self.barrier.wait().await;
    }
}

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
    use super::*;
    #[test]
    fn mode_parse_native() {
        assert_eq!(
            InstrumentationMode::from_arg("native").unwrap(),
            InstrumentationMode::Native
        );
    }
    #[test]
    fn mode_parse_tracing() {
        assert_eq!(
            InstrumentationMode::from_arg("tracing").unwrap(),
            InstrumentationMode::Tracing
        );
    }
    #[test]
    fn mode_parse_invalid() {
        assert!(InstrumentationMode::from_arg("x").is_err());
    }
}
