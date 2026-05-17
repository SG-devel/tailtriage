#![allow(clippy::missing_errors_doc, clippy::must_use_candidate)]
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use tailtriage_core::Tailtriage;
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
    fn from_arg(value: Option<&str>) -> anyhow::Result<Self> {
        match value {
            None | Some("native") => Ok(Self::Native),
            Some("tracing") => Ok(Self::Tracing),
            Some(other) => anyhow::bail!(
                "unsupported instrumentation '{other}', expected one of: native, tracing"
            ),
        }
    }
}

#[derive(Debug)]
pub struct DemoArgs {
    pub output_path: PathBuf,
    pub mode: DemoMode,
    pub instrumentation: InstrumentationMode,
}

pub fn parse_demo_args(default_output_path: &str) -> anyhow::Result<DemoArgs> {
    {
        let args: Vec<String> = std::env::args().skip(1).collect();
        parse_demo_args_from(&args, default_output_path)
    }
}

fn parse_demo_args_from(
    raw_args: &[String],
    default_output_path: &str,
) -> anyhow::Result<DemoArgs> {
    let mut output_path: Option<PathBuf> = None;
    let mut mode: Option<DemoMode> = None;
    let mut instrumentation: Option<InstrumentationMode> = None;

    let mut idx = 0_usize;
    while idx < raw_args.len() {
        let arg = &raw_args[idx];
        if arg == "--instrumentation" {
            let value = raw_args
                .get(idx + 1)
                .map(String::as_str)
                .ok_or_else(|| anyhow::anyhow!("missing value for --instrumentation"))?;
            instrumentation = Some(InstrumentationMode::from_arg(Some(value))?);
            idx += 2;
            continue;
        }

        if output_path.is_none() {
            output_path = Some(PathBuf::from(arg));
            idx += 1;
            continue;
        }

        if mode.is_none() {
            mode = Some(DemoMode::from_arg(Some(arg))?);
            idx += 1;
            continue;
        }

        anyhow::bail!("unexpected extra argument '{arg}'");
    }

    let output_path = output_path.unwrap_or_else(|| PathBuf::from(default_output_path));
    let mode = mode.unwrap_or(DemoMode::Baseline);
    let instrumentation = instrumentation.unwrap_or(InstrumentationMode::Native);
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

pub struct DemoInstrumentation {
    backend: DemoInstrumentationBackend,
}

enum DemoInstrumentationBackend {
    Native(Arc<Tailtriage>),
    Tracing(TracingState),
}

pub struct DemoRequest {
    inner: DemoRequestInner,
}

enum DemoRequestInner {
    Native(tailtriage_core::OwnedRequestHandle),
    Tracing(TracingRequest),
}

struct TracingState {
    recorder: TracingRecorder,
}

struct TracingRequest {
    request_id: String,
}

impl DemoInstrumentation {
    pub fn new(
        service_name: &str,
        output_path: &Path,
        mode: InstrumentationMode,
    ) -> anyhow::Result<Self> {
        match mode {
            InstrumentationMode::Native => Ok(Self {
                backend: DemoInstrumentationBackend::Native(init_collector(
                    service_name,
                    output_path,
                )?),
            }),
            InstrumentationMode::Tracing => {
                let recorder = TracingRecorder::builder(service_name).strict(false).build();
                let subscriber = tracing_subscriber::registry().with(recorder.layer());
                tracing::subscriber::set_global_default(subscriber)
                    .map_err(|e| anyhow::anyhow!("failed to install tracing subscriber: {e}"))?;
                Ok(Self {
                    backend: DemoInstrumentationBackend::Tracing(TracingState { recorder }),
                })
            }
        }
    }

    pub async fn run_request<F, Fut>(&self, route: &str, request_id: String, outcome: &str, body: F)
    where
        F: FnOnce(DemoRequest) -> Fut,
        Fut: Future<Output = ()>,
    {
        match &self.backend {
            DemoInstrumentationBackend::Native(tailtriage) => {
                let started = tailtriage.begin_request_with_owned(
                    route,
                    tailtriage_core::RequestOptions::new().request_id(request_id.clone()),
                );
                let request = DemoRequest {
                    inner: DemoRequestInner::Native(started.handle.clone()),
                };
                body(request).await;
                let native_outcome = if outcome == "ok" {
                    tailtriage_core::Outcome::Ok
                } else {
                    tailtriage_core::Outcome::Error
                };
                started.completion.finish(native_outcome);
            }
            DemoInstrumentationBackend::Tracing(_) => {
                let request_span = tracing::info_span!(
                    "tt.request",
                    tt.kind = "request",
                    tt.request_id = request_id.as_str(),
                    tt.route = route,
                    tt.outcome = outcome
                );
                body(DemoRequest {
                    inner: DemoRequestInner::Tracing(TracingRequest { request_id }),
                })
                .instrument(request_span)
                .await;
            }
        }
    }

    pub fn shutdown(self, output_path: &Path) -> anyhow::Result<()> {
        match &self.backend {
            DemoInstrumentationBackend::Native(tailtriage) => {
                tailtriage.shutdown()?;
                Ok(())
            }
            DemoInstrumentationBackend::Tracing(state) => {
                let imported = state.recorder.shutdown()?;
                let mut file = std::fs::File::create(output_path)?;
                serde_json::to_writer_pretty(&mut file, imported.run())?;
                Ok(())
            }
        }
    }
}

impl DemoRequest {
    pub fn inflight(&self, label: &str) -> Option<tailtriage_core::InflightGuard<'_>> {
        match &self.inner {
            DemoRequestInner::Native(request) => Some(request.inflight(label)),
            DemoRequestInner::Tracing(_) => None,
        }
    }

    pub async fn queue_wait<Fut>(
        &self,
        queue: &str,
        depth_at_start: u64,
        future: Fut,
    ) -> Fut::Output
    where
        Fut: Future,
    {
        match &self.inner {
            DemoRequestInner::Native(request) => {
                request
                    .queue(queue)
                    .with_depth_at_start(depth_at_start)
                    .await_on(future)
                    .await
            }
            DemoRequestInner::Tracing(tracing_request) => {
                let span = tracing::info_span!(
                    "tt.queue",
                    tt.kind = "queue",
                    tt.request_id = tracing_request.request_id.as_str(),
                    tt.queue = queue,
                    tt.depth_at_start = depth_at_start
                );
                future.instrument(span).await
            }
        }
    }

    pub async fn stage<Fut>(&self, stage: &str, future: Fut)
    where
        Fut: Future,
    {
        match &self.inner {
            DemoRequestInner::Native(request) => {
                request.stage(stage).await_value(future).await;
            }
            DemoRequestInner::Tracing(tracing_request) => {
                let span = tracing::info_span!(
                    "tt.stage",
                    tt.kind = "stage",
                    tt.request_id = tracing_request.request_id.as_str(),
                    tt.stage = stage,
                    tt.success = true
                );
                future.instrument(span).await;
            }
        }
    }
}

pub fn init_collector(service_name: &str, output_path: &Path) -> anyhow::Result<Arc<Tailtriage>> {
    let collector = Tailtriage::builder(service_name)
        .output(output_path)
        .build()?;
    Ok(Arc::new(collector))
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
    use super::{parse_demo_args_from, DemoMode, InstrumentationMode};

    #[test]
    fn demo_args_default_instrumentation_is_native() {
        let args = parse_demo_args_from(&["out.json".to_string()], "ignored").expect("parse args");
        assert_eq!(args.mode, DemoMode::Baseline);
        assert_eq!(args.instrumentation, InstrumentationMode::Native);
    }

    #[test]
    fn demo_args_explicit_native_instrumentation() {
        let args = parse_demo_args_from(
            &[
                "out.json".to_string(),
                "--instrumentation".to_string(),
                "native".to_string(),
            ],
            "ignored",
        )
        .expect("parse args");
        assert_eq!(args.instrumentation, InstrumentationMode::Native);
    }

    #[test]
    fn demo_args_explicit_tracing_instrumentation() {
        let args = parse_demo_args_from(
            &[
                "out.json".to_string(),
                "--instrumentation".to_string(),
                "tracing".to_string(),
            ],
            "ignored",
        )
        .expect("parse args");
        assert_eq!(args.instrumentation, InstrumentationMode::Tracing);
    }

    #[test]
    fn demo_args_unsupported_instrumentation_errors() {
        let err = parse_demo_args_from(
            &[
                "out.json".to_string(),
                "--instrumentation".to_string(),
                "otel".to_string(),
            ],
            "ignored",
        )
        .expect_err("expected unsupported instrumentation error");
        assert!(err.to_string().contains("unsupported instrumentation"));
    }

    #[test]
    fn demo_args_old_positional_mode_aliases_still_work() {
        let before_args =
            parse_demo_args_from(&["out.json".to_string(), "before".to_string()], "ignored")
                .expect("parse before alias");
        assert_eq!(before_args.mode, DemoMode::Baseline);

        let after_args =
            parse_demo_args_from(&["out.json".to_string(), "after".to_string()], "ignored")
                .expect("parse after alias");
        assert_eq!(after_args.mode, DemoMode::Mitigated);
    }
}
