use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use tailtriage_core::{Outcome, RequestOptions, Tailtriage};
use tailtriage_tracing::TracingRecorder;
use tokio::sync::Barrier;
use tracing::Instrument;
use tracing_subscriber::prelude::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Demo profile selector used by before/after style demo binaries.
pub enum DemoMode {
    Baseline,
    Mitigated,
}
impl DemoMode {
    /// Parse the optional positional mode argument.
    ///
    /// # Errors
    /// Returns an error for unsupported mode values.
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
/// Instrumentation backend for converted demos.
pub enum InstrumentationMode {
    Native,
    Tracing,
}
impl InstrumentationMode {
    /// Parse instrumentation backend value.
    ///
    /// # Errors
    /// Returns an error for unsupported backend values.
    pub fn from_arg(value: &str) -> anyhow::Result<Self> {
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

/// Parse common demo arguments as `<output_path> [mode] [--instrumentation native|tracing]`.
///
/// # Errors
/// Returns an error when mode/flags are invalid or output directory creation fails.
pub fn parse_demo_args(default_output_path: &str) -> anyhow::Result<DemoArgs> {
    let mut args = std::env::args().skip(1);
    let output_path = args
        .next()
        .map_or_else(|| PathBuf::from(default_output_path), PathBuf::from);
    let mut mode: Option<DemoMode> = None;
    let mut instrumentation = InstrumentationMode::Native;

    let remaining: Vec<String> = args.collect();
    let mut i = 0usize;
    while i < remaining.len() {
        let token = &remaining[i];
        if token == "--instrumentation" {
            let value = remaining
                .get(i + 1)
                .context("missing value for --instrumentation")?;
            instrumentation = InstrumentationMode::from_arg(value)?;
            i += 2;
            continue;
        }
        if let Some(value) = token.strip_prefix("--instrumentation=") {
            instrumentation = InstrumentationMode::from_arg(value)?;
            i += 1;
            continue;
        }
        if token.starts_with("--") {
            anyhow::bail!("unsupported argument '{token}'");
        }
        if mode.is_some() {
            anyhow::bail!("unsupported extra positional argument '{token}'");
        }
        mode = Some(DemoMode::from_arg(Some(token))?);
        i += 1;
    }

    ensure_parent_dir(&output_path)?;
    Ok(DemoArgs {
        output_path,
        mode: mode.unwrap_or(DemoMode::Baseline),
        instrumentation,
    })
}

/// Initialize the legacy native Tailtriage collector used by non-converted demos.
///
/// # Errors
/// Returns an error when collector initialization fails.
pub fn init_collector(service_name: &str, output_path: &Path) -> anyhow::Result<Arc<Tailtriage>> {
    Ok(Arc::new(
        Tailtriage::builder(service_name)
            .output(output_path)
            .build()?,
    ))
}
pub enum DemoRecorder {
    Native(Arc<Tailtriage>),
    Tracing(TracingRecorder),
}

/// Initialize demo recorder backend for the selected instrumentation mode.
///
/// # Errors
/// Returns an error when recorder initialization fails.
pub fn init_recorder(
    service_name: &str,
    output_path: &Path,
    mode: InstrumentationMode,
) -> anyhow::Result<Arc<DemoRecorder>> {
    Ok(Arc::new(match mode {
        InstrumentationMode::Native => DemoRecorder::Native(Arc::new(
            Tailtriage::builder(service_name)
                .output(output_path)
                .build()?,
        )),
        InstrumentationMode::Tracing => {
            let recorder = TracingRecorder::builder(service_name).build();
            let subscriber = tracing_subscriber::registry().with(recorder.layer());
            tracing::subscriber::set_global_default(subscriber)
                .map_err(|err| anyhow::anyhow!("failed to set tracing subscriber: {err}"))?;
            DemoRecorder::Tracing(recorder)
        }
    }))
}

pub struct DemoRequest {
    inner: RequestInner,
}

enum RequestInner {
    Native {
        handle: tailtriage_core::OwnedRequestHandle,
        completion: tailtriage_core::OwnedRequestCompletion,
    },
    Tracing {
        request_id: String,
        request_span: tracing::Span,
    },
}

impl DemoRecorder {
    pub fn begin_request(&self, route: &str, request_id: String) -> DemoRequest {
        match self {
            DemoRecorder::Native(tt) => {
                let started = tt
                    .begin_request_with_owned(route, RequestOptions::new().request_id(request_id));
                DemoRequest {
                    inner: RequestInner::Native {
                        handle: started.handle,
                        completion: started.completion,
                    },
                }
            }
            DemoRecorder::Tracing(_) => {
                let request_span = tracing::info_span!(
                    "http.request",
                    tt.kind = "request",
                    tt.request_id = %request_id,
                    tt.route = %route,
                    tt.outcome = tracing::field::Empty
                );
                DemoRequest {
                    inner: RequestInner::Tracing {
                        request_id,
                        request_span,
                    },
                }
            }
        }
    }

    /// Finalize capture and write the run JSON artifact to `output_path`.
    ///
    /// # Errors
    /// Returns an error when conversion or artifact writing fails.
    pub fn shutdown(&self, output_path: &Path) -> anyhow::Result<()> {
        match self {
            DemoRecorder::Native(tt) => tt.shutdown()?,
            DemoRecorder::Tracing(recorder) => {
                let run = recorder.shutdown()?.run().clone();
                std::fs::write(output_path, serde_json::to_vec_pretty(&run)?)?;
            }
        }
        Ok(())
    }
}

impl DemoRequest {
    pub async fn record_queue_wait<F, T>(
        &self,
        queue_name: &str,
        depth_at_start: Option<u64>,
        fut: F,
    ) -> T
    where
        F: Future<Output = T>,
    {
        match &self.inner {
            RequestInner::Native { handle, .. } => {
                let mut timer = handle.queue(queue_name);
                if let Some(depth) = depth_at_start {
                    timer = timer.with_depth_at_start(depth);
                }
                timer.await_on(fut).await
            }
            RequestInner::Tracing {
                request_id,
                request_span,
            } => {
                fut.instrument(tracing::info_span!(
                    parent: request_span,
                    "queue.wait",
                    tt.kind = "queue",
                    tt.request_id = %request_id,
                    tt.queue = %queue_name,
                    tt.depth_at_start = depth_at_start.unwrap_or(0)
                ))
                .await
            }
        }
    }

    pub async fn record_stage<F, T>(&self, stage_name: &str, fut: F) -> T
    where
        F: Future<Output = T>,
    {
        match &self.inner {
            RequestInner::Native { handle, .. } => handle.stage(stage_name).await_value(fut).await,
            RequestInner::Tracing {
                request_id,
                request_span,
            } => {
                fut.instrument(tracing::info_span!(
                    parent: request_span,
                    "request.stage",
                    tt.kind = "stage",
                    tt.request_id = %request_id,
                    tt.stage = %stage_name
                ))
                .await
            }
        }
    }

    pub fn finish(self, outcome: Outcome) {
        match self.inner {
            RequestInner::Native { completion, .. } => completion.finish(outcome),
            RequestInner::Tracing { request_span, .. } => {
                request_span.record(
                    "tt.outcome",
                    if matches!(outcome, Outcome::Ok) {
                        "ok"
                    } else {
                        "error"
                    },
                );
                drop(request_span);
            }
        }
    }
}

fn ensure_parent_dir(output_path: &Path) -> anyhow::Result<()> {
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create artifact directory {}", parent.display()))?;
    }
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
    WarmupFut: Future<Output = ()>,
    Measured: FnOnce() -> MeasuredFut,
    MeasuredFut: Future<Output = ()>,
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
    fn instrumentation_mode_parse() {
        assert_eq!(
            InstrumentationMode::from_arg("native").unwrap(),
            InstrumentationMode::Native
        );
        assert_eq!(
            InstrumentationMode::from_arg("tracing").unwrap(),
            InstrumentationMode::Tracing
        );
        assert!(InstrumentationMode::from_arg("bad").is_err());
    }
}
