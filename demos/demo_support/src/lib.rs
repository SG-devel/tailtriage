use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use tailtriage_core::{Outcome, RequestOptions, Run, Tailtriage};
use tailtriage_tracing::TracingRecorder;
use tokio::sync::Barrier;
use tracing::Instrument;
use tracing_subscriber::prelude::__tracing_subscriber_SubscriberExt;

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
    pub fn parse(value: &str) -> anyhow::Result<Self> {
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
    let mut args = std::env::args().skip(1);
    let output_path = args
        .next()
        .map_or_else(|| PathBuf::from(default_output_path), PathBuf::from);
    let mut mode_arg: Option<String> = None;
    let mut instrumentation = InstrumentationMode::Native;
    while let Some(arg) = args.next() {
        if arg == "--instrumentation" {
            let value = args.next().context("missing value for --instrumentation")?;
            instrumentation = InstrumentationMode::parse(&value)?;
        } else if mode_arg.is_none() {
            mode_arg = Some(arg);
        } else {
            anyhow::bail!("unexpected argument '{arg}'")
        }
    }
    let mode = DemoMode::from_arg(mode_arg.as_ref())?;
    ensure_parent_dir(&output_path)?;
    Ok(DemoArgs {
        output_path,
        mode,
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

#[derive(Clone)]
pub enum DemoInstrumentation {
    Native(Arc<Tailtriage>),
    Tracing(TracingInstrumentation),
}
#[derive(Clone)]
pub struct TracingInstrumentation {
    recorder: TracingRecorder,
}

impl DemoInstrumentation {
    pub fn new(service_name: &str, mode: InstrumentationMode) -> anyhow::Result<Self> {
        match mode {
            InstrumentationMode::Native => Ok(Self::Native(Arc::new(
                Tailtriage::builder(service_name).build()?,
            ))),
            InstrumentationMode::Tracing => {
                let recorder = TracingRecorder::builder(service_name).build();
                let subscriber = tracing_subscriber::registry().with(recorder.layer());
                tracing::subscriber::set_global_default(subscriber)
                    .context("failed to install tracing subscriber (already set?)")?;
                Ok(Self::Tracing(TracingInstrumentation { recorder }))
            }
        }
    }
    pub fn begin_request(&self, route: &str, request_id: &str) -> DemoRequest {
        match self {
            Self::Native(tt) => {
                let started = tt.begin_request_with_owned(
                    route,
                    RequestOptions::new().request_id(request_id.to_owned()),
                );
                DemoRequest::Native(started)
            }
            Self::Tracing(_) => DemoRequest::Tracing(TracingRequest {
                route: route.to_owned(),
                request_id: request_id.to_owned(),
            }),
        }
    }
    pub fn shutdown(self, output_path: &Path) -> anyhow::Result<()> {
        match self {
            Self::Native(tt) => {
                tt.shutdown()?;
            }
            Self::Tracing(t) => {
                let imported = t.recorder.shutdown()?;
                let run: &Run = imported.run();
                let file = std::fs::File::create(output_path)
                    .with_context(|| format!("failed to create {}", output_path.display()))?;
                serde_json::to_writer_pretty(file, run)?;
            }
        }
        Ok(())
    }
}

pub enum DemoRequest {
    Native(tailtriage_core::OwnedStartedRequest),
    Tracing(TracingRequest),
}
pub struct TracingRequest {
    route: String,
    request_id: String,
}

impl DemoRequest {
    pub async fn queue_wait<F, T>(&self, queue: &str, depth_at_start: u64, fut: F) -> T
    where
        F: Future<Output = T>,
    {
        match self {
            Self::Native(started) => {
                started
                    .handle
                    .queue(queue)
                    .with_depth_at_start(depth_at_start)
                    .await_on(fut)
                    .await
            }
            Self::Tracing(tr) => {
                let span = tracing::info_span!("queue", tt.kind="queue", tt.request_id=%tr.request_id, tt.queue=%queue, tt.depth_at_start=depth_at_start);
                fut.instrument(span).await
            }
        }
    }
    pub async fn stage<F, T>(&self, stage: &str, fut: F) -> T
    where
        F: Future<Output = T>,
    {
        match self {
            Self::Native(started) => started.handle.stage(stage).await_value(fut).await,
            Self::Tracing(tr) => {
                let span = tracing::info_span!("stage", tt.kind="stage", tt.request_id=%tr.request_id, tt.stage=%stage, tt.success=true);
                fut.instrument(span).await
            }
        }
    }
    pub async fn finish(self, outcome: Outcome) {
        match self {
            Self::Native(started) => started.completion.finish(outcome),
            Self::Tracing(tr) => {
                let outcome_label = outcome.as_str();
                let request_span = tracing::info_span!("request", tt.kind="request", tt.request_id=%tr.request_id, tt.route=%tr.route, tt.outcome=outcome_label);
                async {}.instrument(request_span).await;
            }
        }
    }
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
    fn parse_from(args: &[&str]) -> anyhow::Result<DemoArgs> {
        let mut output = PathBuf::from("out.json");
        let mut mode_arg: Option<String> = None;
        let mut instrumentation = InstrumentationMode::Native;
        let mut it = args.iter();
        if let Some(path) = it.next() {
            output = PathBuf::from(path);
        }
        while let Some(arg) = it.next() {
            if *arg == "--instrumentation" {
                instrumentation = InstrumentationMode::parse(
                    it.next().context("missing value for --instrumentation")?,
                )?;
            } else if mode_arg.is_none() {
                mode_arg = Some((*arg).to_string());
            }
        }
        Ok(DemoArgs {
            output_path: output,
            mode: DemoMode::from_arg(mode_arg.as_ref())?,
            instrumentation,
        })
    }
    #[test]
    fn default_instrumentation_native() {
        assert_eq!(
            parse_from(&[]).unwrap().instrumentation,
            InstrumentationMode::Native
        );
    }
    #[test]
    fn explicit_native() {
        assert_eq!(
            parse_from(&["x", "--instrumentation", "native"])
                .unwrap()
                .instrumentation,
            InstrumentationMode::Native
        );
    }
    #[test]
    fn explicit_tracing() {
        assert_eq!(
            parse_from(&["x", "--instrumentation", "tracing"])
                .unwrap()
                .instrumentation,
            InstrumentationMode::Tracing
        );
    }
    #[test]
    fn unsupported_instrumentation_errors() {
        assert!(parse_from(&["x", "--instrumentation", "bad"]).is_err());
    }
    #[test]
    fn positional_mode_still_works() {
        assert_eq!(
            parse_from(&["x", "before"]).unwrap().mode,
            DemoMode::Baseline
        );
    }
}
