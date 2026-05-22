use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use tailtriage_core::{CaptureLimitsOverride, CaptureMode, Tailtriage};
use tailtriage_tracing::{tokio::TracingTokioSession, TracingRecorder};
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
    /// Parse a positional mode argument into a demo mode.
    ///
    /// Accepts legacy aliases so existing demo commands continue to work.
    ///
    /// # Errors
    ///
    /// Returns an error when the provided mode string is not supported.
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
    /// Parse the `--instrumentation` mode argument.
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
    pub capture_mode: CaptureMode,
    pub capture_limits_override: CaptureLimitsOverride,
}

/// Parse demo CLI args for output path, mode, and instrumentation backend.
///
/// # Errors
///
/// Returns an error for unsupported arguments or when output directory creation fails.
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
    let mut capture_mode: CaptureMode = CaptureMode::Light;
    let mut capture_limits_override = CaptureLimitsOverride::default();

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
        if arg == "--mode" {
            let value = raw_args
                .get(idx + 1)
                .map(String::as_str)
                .ok_or_else(|| anyhow::anyhow!("missing value for --mode"))?;
            capture_mode = match value {
                "light" => CaptureMode::Light,
                "investigation" => CaptureMode::Investigation,
                other => anyhow::bail!("unsupported --mode '{other}', expected one of: light, investigation"),
            };
            idx += 2;
            continue;
        }
        if arg == "--max-requests" || arg == "--max-stages" || arg == "--max-queues" {
            let value = raw_args
                .get(idx + 1)
                .ok_or_else(|| anyhow::anyhow!("missing value for {arg}"))?
                .parse::<usize>()
                .map_err(|e| anyhow::anyhow!("invalid integer for {arg}: {e}"))?;
            match arg.as_str() {
                "--max-requests" => capture_limits_override.max_requests = Some(value),
                "--max-stages" => capture_limits_override.max_stages = Some(value),
                "--max-queues" => capture_limits_override.max_queues = Some(value),
                _ => {}
            }
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
        capture_mode,
        capture_limits_override,
    })
}

/// Parse demo output path from argv, preserving legacy positional behavior.
///
/// # Errors
///
/// Returns an error when creating the output artifact parent directory fails.
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

/// Demo instrumentation helper for runtime-sensitive scenarios (`blocking` and `executor`).
///
/// This helper supports `native` and `tracing` modes while keeping one request API surface.
/// Use [`Self::record_runtime_snapshot`] to attach deterministic runtime-pressure evidence
/// captured during workload execution for parity validation.
pub struct RuntimeDemoInstrumentation {
    backend: RuntimeDemoBackend,
}

enum RuntimeDemoBackend {
    Native(Arc<Tailtriage>),
    Tracing(Box<TracingTokioSession>),
}

enum DemoInstrumentationBackend {
    Native(Arc<Tailtriage>),
    Tracing(Box<TracingState>),
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
    /// Build demo instrumentation for either native or tracing capture.
    ///
    /// Native mode writes through `tailtriage-core`. Tracing mode records spans
    /// and converts them to a run artifact at shutdown.
    ///
    /// # Errors
    ///
    /// Returns an error when backend initialization fails.
    pub fn new(
        service_name: &str,
        output_path: &Path,
        mode: InstrumentationMode,
        capture_mode: CaptureMode,
        capture_limits_override: CaptureLimitsOverride,
    ) -> anyhow::Result<Self> {
        match mode {
            InstrumentationMode::Native => Ok(Self {
                backend: DemoInstrumentationBackend::Native(init_collector(
                    service_name,
                    output_path,
                    capture_mode,
                    capture_limits_override,
                )?),
            }),
            InstrumentationMode::Tracing => {
                let recorder = TracingRecorder::builder(service_name)
                    .strict(false)
                    .mode(capture_mode)
                    .capture_limits_override(capture_limits_override)
                    .build();
                let subscriber = tracing_subscriber::registry().with(recorder.layer());
                // Demo binaries run one instrumentation backend per process, so installing a
                // global subscriber is acceptable here. Do not reuse this helper in libraries
                // or tests that need multiple subscribers in one process.
                tracing::subscriber::set_global_default(subscriber)
                    .map_err(|e| anyhow::anyhow!("failed to install tracing subscriber: {e}"))?;
                Ok(Self {
                    backend: DemoInstrumentationBackend::Tracing(Box::new(TracingState {
                        recorder,
                    })),
                })
            }
        }
    }

    /// Run one request lifecycle with request-level instrumentation.
    pub async fn run_request<F, Fut>(
        &self,
        route: &str,
        request_id: String,
        outcome: tailtriage_core::Outcome,
        body: F,
    ) where
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
                started.completion.finish(outcome);
            }
            DemoInstrumentationBackend::Tracing(_) => {
                let outcome_label = outcome.as_str();
                let request_span = tracing::info_span!(
                    "tt.request",
                    tt.kind = "request",
                    tt.request_id = request_id.as_str(),
                    tt.route = route,
                    tt.outcome = outcome_label
                );
                body(DemoRequest {
                    inner: DemoRequestInner::Tracing(TracingRequest { request_id }),
                })
                .instrument(request_span)
                .await;
            }
        }
    }

    /// Flush instrumentation and write the final run artifact.
    ///
    /// # Errors
    ///
    /// Returns an error when shutting down instrumentation or writing output fails.
    pub fn shutdown(self, output_path: &Path) -> anyhow::Result<()> {
        match self.backend {
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

impl RuntimeDemoInstrumentation {
    /// Build runtime-sensitive demo instrumentation for native or tracing capture.
    ///
    /// # Errors
    ///
    /// Returns an error when backend initialization or tracing subscriber setup fails.
    pub fn new(
        service_name: &str,
        output_path: &Path,
        mode: InstrumentationMode,
        capture_mode: CaptureMode,
        capture_limits_override: CaptureLimitsOverride,
    ) -> anyhow::Result<Self> {
        match mode {
            InstrumentationMode::Native => Ok(Self {
                backend: RuntimeDemoBackend::Native(init_collector(
                    service_name,
                    output_path,
                    capture_mode,
                    capture_limits_override,
                )?),
            }),
            InstrumentationMode::Tracing => {
                let session = TracingTokioSession::builder(service_name)
                    .strict(false)
                    .mode(capture_mode)
                    .capture_limits_override(capture_limits_override)
                    .start()?;
                let subscriber = tracing_subscriber::registry().with(session.layer());
                // Demo binaries run one instrumentation backend per process, so installing a
                // global subscriber is acceptable here. Do not reuse this helper in libraries
                // or tests that need multiple subscribers in one process.
                tracing::subscriber::set_global_default(subscriber)
                    .map_err(|e| anyhow::anyhow!("failed to install tracing subscriber: {e}"))?;
                Ok(Self {
                    backend: RuntimeDemoBackend::Tracing(Box::new(session)),
                })
            }
        }
    }

    /// Run one request lifecycle with request-level instrumentation.
    pub async fn run_request<F, Fut>(
        &self,
        route: &str,
        request_id: String,
        outcome: tailtriage_core::Outcome,
        body: F,
    ) where
        F: FnOnce(DemoRequest) -> Fut,
        Fut: Future<Output = ()>,
    {
        match &self.backend {
            RuntimeDemoBackend::Native(tailtriage) => {
                let started = tailtriage.begin_request_with_owned(
                    route,
                    tailtriage_core::RequestOptions::new().request_id(request_id.clone()),
                );
                body(DemoRequest {
                    inner: DemoRequestInner::Native(started.handle.clone()),
                })
                .await;
                started.completion.finish(outcome);
            }
            RuntimeDemoBackend::Tracing(_) => {
                let request_span = tracing::info_span!(
                    "tt.request",
                    tt.kind = "request",
                    tt.request_id = request_id.as_str(),
                    tt.route = route,
                    tt.outcome = outcome.as_str()
                );
                body(DemoRequest {
                    inner: DemoRequestInner::Tracing(TracingRequest { request_id }),
                })
                .instrument(request_span)
                .await;
            }
        }
    }

    /// Record a deterministic Tokio runtime snapshot during workload execution.
    ///
    /// Runtime-sensitive tracing parity uses this to inject runtime-pressure evidence because
    /// tracing request/stage/queue spans alone do not infer runtime-pressure signals.
    pub fn record_runtime_snapshot(&self, snapshot: tailtriage_core::RuntimeSnapshot) {
        match &self.backend {
            RuntimeDemoBackend::Native(tailtriage) => tailtriage.record_runtime_snapshot(snapshot),
            RuntimeDemoBackend::Tracing(session) => session.record_runtime_snapshot(snapshot),
        }
    }

    /// Flush instrumentation and write the final run artifact.
    ///
    /// # Errors
    ///
    /// Returns an error when shutting down instrumentation or writing output fails.
    pub async fn shutdown(self, output_path: &Path) -> anyhow::Result<()> {
        match self.backend {
            RuntimeDemoBackend::Native(t) => {
                t.shutdown()?;
                Ok(())
            }
            RuntimeDemoBackend::Tracing(s) => {
                let imported = s.shutdown().await?;
                let mut file = std::fs::File::create(output_path)?;
                serde_json::to_writer_pretty(&mut file, imported.run())?;
                Ok(())
            }
        }
    }
}

impl DemoRequest {
    #[must_use]
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

    pub async fn stage<Fut>(&self, stage: &str, future: Fut) -> Fut::Output
    where
        Fut: Future,
    {
        match &self.inner {
            DemoRequestInner::Native(request) => request.stage(stage).await_value(future).await,
            DemoRequestInner::Tracing(tracing_request) => {
                let span = tracing::info_span!(
                    "tt.stage",
                    tt.kind = "stage",
                    tt.request_id = tracing_request.request_id.as_str(),
                    tt.stage = stage,
                    tt.success = true
                );
                future.instrument(span).await
            }
        }
    }
}

/// Initialize a native tailtriage collector for demos.
///
/// # Errors
///
/// Returns an error when collector initialization fails.
pub fn init_collector(
    service_name: &str,
    output_path: &Path,
    capture_mode: CaptureMode,
    capture_limits_override: CaptureLimitsOverride,
) -> anyhow::Result<Arc<Tailtriage>> {
    let collector = Tailtriage::builder(service_name)
        .output(output_path)
        .light()
        .capture_limits_override(capture_limits_override)
        .build()?;
    let collector = if matches!(capture_mode, CaptureMode::Investigation) {
        Tailtriage::builder(service_name)
            .output(output_path)
            .investigation()
            .capture_limits_override(capture_limits_override)
            .build()?
    } else {
        collector
    };
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
    use tailtriage_core::Outcome;

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

    #[test]
    fn outcome_other_preserves_custom_label() {
        let outcome = Outcome::Other("custom".to_string());
        assert_eq!(outcome.as_str(), "custom");
    }
}
