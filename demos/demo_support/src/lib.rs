#![allow(clippy::missing_errors_doc, clippy::must_use_candidate)]

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use tailtriage_core::{LocalJsonSink, Outcome, RequestOptions, RunSink, Tailtriage};
use tailtriage_tracing::TracingRecorder;
use tokio::sync::Barrier;
use tracing::Instrument;
use tracing_subscriber::prelude::*;

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
            other => {
                anyhow::bail!("unsupported instrumentation '{other}', expected: native|tracing")
            }
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
    let mut output_path: Option<PathBuf> = None;
    let mut mode_arg: Option<String> = None;
    let mut instrumentation = InstrumentationMode::Native;

    while let Some(arg) = args.next() {
        if arg == "--instrumentation" {
            let value = args
                .next()
                .context("missing value for --instrumentation; expected native|tracing")?;
            instrumentation = InstrumentationMode::parse(&value)?;
            continue;
        }
        if output_path.is_none() {
            output_path = Some(PathBuf::from(arg));
            continue;
        }
        if mode_arg.is_none() {
            mode_arg = Some(arg);
            continue;
        }
        anyhow::bail!("unexpected argument '{arg}'")
    }

    let output_path = output_path.unwrap_or_else(|| PathBuf::from(default_output_path));
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

pub enum DemoRecorder {
    Native(Arc<Tailtriage>),
    Tracing(TracingRecorder),
}

impl DemoRecorder {
    pub fn start_request(&self, route: &str, request_id: &str) -> DemoRequest {
        match self {
            Self::Native(collector) => {
                let started = collector.begin_request_with_owned(
                    route,
                    RequestOptions::new().request_id(request_id.to_string()),
                );
                DemoRequest::Native(started)
            }
            Self::Tracing(_) => DemoRequest::Tracing {
                route: route.to_string(),
                request_id: request_id.to_string(),
            },
        }
    }

    pub fn shutdown(self, output_path: &Path) -> anyhow::Result<()> {
        match self {
            Self::Native(collector) => collector.shutdown()?,
            Self::Tracing(recorder) => {
                let imported = recorder.shutdown()?;
                LocalJsonSink::new(output_path).write(imported.run())?;
            }
        }
        Ok(())
    }
}

pub enum DemoRequest {
    Native(tailtriage_core::OwnedStartedRequest),
    Tracing { route: String, request_id: String },
}

impl DemoRequest {
    pub async fn record_queue<T, F>(
        &self,
        queue_name: &'static str,
        depth_at_start: u64,
        future: F,
    ) -> T
    where
        F: Future<Output = T>,
    {
        match self {
            Self::Native(started) => {
                started
                    .handle
                    .queue(queue_name)
                    .with_depth_at_start(depth_at_start)
                    .await_on(future)
                    .await
            }
            Self::Tracing { request_id, .. } => {
                let queue_span = tracing::info_span!(
                    "demo.queue",
                    tt.kind = "queue",
                    tt.request_id = %request_id,
                    tt.queue = queue_name,
                    tt.depth_at_start = depth_at_start,
                );
                future.instrument(queue_span).await
            }
        }
    }

    pub async fn record_stage<T, F>(&self, stage_name: &'static str, success: bool, future: F) -> T
    where
        F: Future<Output = T>,
    {
        match self {
            Self::Native(started) => started.handle.stage(stage_name).await_value(future).await,
            Self::Tracing { request_id, .. } => {
                let stage_span = tracing::info_span!(
                    "demo.stage",
                    tt.kind = "stage",
                    tt.request_id = %request_id,
                    tt.stage = stage_name,
                    tt.success = success,
                );
                future.instrument(stage_span).await
            }
        }
    }

    pub fn finish(self, outcome: Outcome) {
        match self {
            Self::Native(started) => started.completion.finish(outcome),
            Self::Tracing { route, request_id } => {
                let outcome_value = outcome.as_str();
                tracing::info_span!(
                    "demo.request",
                    tt.kind = "request",
                    tt.request_id = %request_id,
                    tt.route = %route,
                    tt.outcome = outcome_value,
                )
                .in_scope(|| {});
            }
        }
    }
}

pub fn init_demo_recorder(
    service_name: &str,
    mode: InstrumentationMode,
    _output_path: &Path,
) -> anyhow::Result<DemoRecorder> {
    match mode {
        InstrumentationMode::Native => Ok(DemoRecorder::Native(Arc::new(
            Tailtriage::builder(service_name).build()?,
        ))),
        InstrumentationMode::Tracing => {
            let recorder = TracingRecorder::builder(service_name).strict(false).build();
            let subscriber = tracing_subscriber::registry().with(recorder.layer());
            tracing::subscriber::set_global_default(subscriber)?;
            Ok(DemoRecorder::Tracing(recorder))
        }
    }
}

pub fn init_collector(service_name: &str, output_path: &Path) -> anyhow::Result<Arc<Tailtriage>> {
    Ok(Arc::new(
        Tailtriage::builder(service_name)
            .output(output_path)
            .build()?,
    ))
}

#[derive(Clone)]
pub struct CohortStart {
    barrier: Arc<Barrier>,
}
impl CohortStart {
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
    use super::InstrumentationMode;

    #[test]
    fn instrumentation_mode_parses_native() {
        assert_eq!(
            InstrumentationMode::parse("native").expect("native should parse"),
            InstrumentationMode::Native
        );
    }

    #[test]
    fn instrumentation_mode_parses_tracing() {
        assert_eq!(
            InstrumentationMode::parse("tracing").expect("tracing should parse"),
            InstrumentationMode::Tracing
        );
    }

    #[test]
    fn instrumentation_mode_rejects_invalid_value() {
        assert!(InstrumentationMode::parse("otlp").is_err());
    }
}
