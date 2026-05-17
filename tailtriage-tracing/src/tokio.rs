use std::sync::Arc;
use std::time::Duration;

use tailtriage_core::{MemorySink, Tailtriage};

use crate::{ImportError, ImportedRun, RecorderLimits, TailtriageLayer, TracingRecorder};

/// Session that couples tracing span recording with optional Tokio runtime sampling.
#[derive(Debug)]
pub struct TracingTokioSession {
    recorder: TracingRecorder,
    runtime_tailtriage: Arc<Tailtriage>,
    sampler: tailtriage_tokio::RuntimeSampler,
}

/// Builder for [`TracingTokioSession`].
#[derive(Debug, Clone)]
pub struct TracingTokioSessionBuilder {
    service_name: String,
    service_version: Option<String>,
    run_id: Option<String>,
    strict: bool,
    recorder_limits: RecorderLimits,
    sampler_interval: Option<Duration>,
    max_runtime_snapshots: Option<usize>,
}

/// Start error for [`TracingTokioSessionBuilder::start`].
#[derive(Debug)]
pub enum TracingTokioSessionStartError {
    /// Tracing recorder import/setup failed.
    Import(ImportError),
    /// Internal runtime collector setup failed.
    Build(tailtriage_core::BuildError),
    /// Tokio sampler setup/start failed.
    SamplerStart(tailtriage_tokio::SamplerStartError),
}

impl std::fmt::Display for TracingTokioSessionStartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Import(err) => write!(f, "failed to start tracing+tokio session: {err}"),
            Self::Build(err) => write!(f, "failed to build internal runtime collector: {err}"),
            Self::SamplerStart(err) => write!(f, "failed to start tokio runtime sampler: {err}"),
        }
    }
}
impl std::error::Error for TracingTokioSessionStartError {}

/// Shutdown error for [`TracingTokioSession::shutdown`].
#[derive(Debug)]
pub enum TracingTokioSessionShutdownError {
    /// Tracing recorder import failed.
    Import(ImportError),
}

impl std::fmt::Display for TracingTokioSessionShutdownError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Import(err) => write!(f, "failed to shutdown tracing+tokio session: {err}"),
        }
    }
}
impl std::error::Error for TracingTokioSessionShutdownError {}

impl TracingTokioSession {
    /// Creates a builder for a tracing+Tokio runtime-sampler session.
    #[must_use]
    pub fn builder(service_name: impl Into<String>) -> TracingTokioSessionBuilder {
        TracingTokioSessionBuilder {
            service_name: service_name.into(),
            service_version: None,
            run_id: None,
            strict: false,
            recorder_limits: RecorderLimits::default(),
            sampler_interval: None,
            max_runtime_snapshots: None,
        }
    }

    /// Returns the tracing subscriber layer for this session.
    #[must_use]
    pub fn layer(&self) -> TailtriageLayer {
        self.recorder.layer()
    }

    /// Returns a merged run of tracing request/stage/queue evidence plus runtime sampler evidence.
    ///
    /// # Errors
    ///
    /// Returns [`ImportError`] when strict tracing import fails.
    pub fn snapshot_run(&self) -> Result<ImportedRun, ImportError> {
        let imported = self.recorder.snapshot_run()?;
        Ok(merge_runtime_into_tracing_run(
            imported,
            self.runtime_tailtriage.snapshot(),
        ))
    }

    /// Stops runtime sampling and returns a final merged run.
    ///
    /// # Errors
    ///
    /// Returns [`TracingTokioSessionShutdownError`] when tracing import fails at shutdown.
    pub async fn shutdown(self) -> Result<ImportedRun, TracingTokioSessionShutdownError> {
        self.sampler.shutdown().await;
        let imported = self
            .recorder
            .shutdown()
            .map_err(TracingTokioSessionShutdownError::Import)?;
        Ok(merge_runtime_into_tracing_run(
            imported,
            self.runtime_tailtriage.snapshot(),
        ))
    }
}

impl TracingTokioSessionBuilder {
    #[must_use]
    /// Sets service version metadata for both tracing and runtime sampling runs.
    pub fn service_version(mut self, service_version: impl Into<String>) -> Self {
        self.service_version = Some(service_version.into());
        self
    }
    #[must_use]
    /// Sets explicit run ID metadata for both tracing and runtime sampling runs.
    pub fn run_id(mut self, run_id: impl Into<String>) -> Self {
        self.run_id = Some(run_id.into());
        self
    }
    #[must_use]
    /// Enables or disables strict tracing span conversion mode.
    pub fn strict(mut self, strict: bool) -> Self {
        self.strict = strict;
        self
    }
    #[must_use]
    /// Sets both tracing recorder span-retention limits.
    pub fn recorder_limits(mut self, recorder_limits: RecorderLimits) -> Self {
        self.recorder_limits = recorder_limits;
        self
    }
    #[must_use]
    /// Sets maximum concurrently tracked open tracing spans.
    pub fn max_open_spans(mut self, max_open_spans: usize) -> Self {
        self.recorder_limits.max_open_spans = max_open_spans;
        self
    }
    #[must_use]
    /// Sets maximum retained completed tracing spans.
    pub fn max_completed_spans(mut self, max_completed_spans: usize) -> Self {
        self.recorder_limits.max_completed_spans = max_completed_spans;
        self
    }
    #[must_use]
    /// Sets Tokio runtime sampler interval.
    pub fn sampler_interval(mut self, sampler_interval: Duration) -> Self {
        self.sampler_interval = Some(sampler_interval);
        self
    }
    #[must_use]
    /// Sets max runtime snapshots retained by the Tokio sampler collector.
    pub fn max_runtime_snapshots(mut self, max_runtime_snapshots: usize) -> Self {
        self.max_runtime_snapshots = Some(max_runtime_snapshots);
        self
    }

    /// Starts the tracing+Tokio session.
    ///
    /// # Errors
    ///
    /// Returns [`TracingTokioSessionStartError`] when recorder setup, internal runtime collector setup, or runtime sampler startup fails.
    pub fn start(self) -> Result<TracingTokioSession, TracingTokioSessionStartError> {
        let service_version = self.service_version.clone();
        let run_id = self.run_id.clone();

        let mut recorder_builder = TracingRecorder::builder(self.service_name)
            .strict(self.strict)
            .limits(self.recorder_limits);
        if let Some(service_version) = service_version.clone() {
            recorder_builder = recorder_builder.service_version(service_version);
        }
        if let Some(run_id) = &run_id {
            recorder_builder = recorder_builder.run_id(run_id.clone());
        }
        let recorder = recorder_builder.build();
        recorder
            .snapshot_run()
            .map_err(TracingTokioSessionStartError::Import)?;

        let mut tailtriage_builder = Tailtriage::builder("tailtriage-tracing-runtime")
            .sink(MemorySink::new())
            .strict_lifecycle(false);
        if let Some(service_version) = service_version {
            tailtriage_builder = tailtriage_builder.service_version(service_version);
        }
        if let Some(run_id) = run_id {
            tailtriage_builder = tailtriage_builder.run_id(run_id);
        }
        let runtime_tailtriage = Arc::new(
            tailtriage_builder
                .build()
                .map_err(TracingTokioSessionStartError::Build)?,
        );

        let mut sampler_builder =
            tailtriage_tokio::RuntimeSampler::builder(Arc::clone(&runtime_tailtriage));
        if let Some(interval) = self.sampler_interval {
            sampler_builder = sampler_builder.interval(interval);
        }
        if let Some(max_runtime_snapshots) = self.max_runtime_snapshots {
            sampler_builder = sampler_builder.max_runtime_snapshots(max_runtime_snapshots);
        }
        let sampler = sampler_builder
            .start()
            .map_err(TracingTokioSessionStartError::SamplerStart)?;

        Ok(TracingTokioSession {
            recorder,
            runtime_tailtriage,
            sampler,
        })
    }
}

fn merge_runtime_into_tracing_run(
    imported: ImportedRun,
    runtime_run: tailtriage_core::Run,
) -> ImportedRun {
    let (mut run, warnings) = imported.into_parts();
    run.runtime_snapshots = runtime_run.runtime_snapshots;
    run.metadata.effective_tokio_sampler_config =
        runtime_run.metadata.effective_tokio_sampler_config;
    run.truncation.dropped_runtime_snapshots = runtime_run.truncation.dropped_runtime_snapshots;
    run.truncation.limits_hit = run.truncation.limits_hit || runtime_run.truncation.limits_hit;
    run.metadata.lifecycle_warnings.extend(
        runtime_run
            .metadata
            .lifecycle_warnings
            .into_iter()
            .filter(|warning| warning.contains("runtime") || warning.contains("sampler")),
    );
    ImportedRun::new(run, warnings)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tracing_subscriber::prelude::*;

    use crate::{TracingTokioSession, TT_KIND, TT_REQUEST_ID, TT_ROUTE, TT_STAGE};

    #[tokio::test(flavor = "current_thread")]
    async fn session_captures_tracing_and_runtime() {
        let session = TracingTokioSession::builder("svc")
            .sampler_interval(Duration::from_millis(1))
            .start()
            .expect("start");
        let subscriber = tracing_subscriber::registry().with(session.layer());
        tracing::subscriber::with_default(subscriber, || {
            let request = tracing::info_span!(
                "req",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/a"
            );
            let stage = tracing::info_span!(
                "stage",
                tt.kind = "stage",
                tt.request_id = "r1",
                tt.stage = "db"
            );
            drop(stage);
            drop(request);
        });

        tokio::time::sleep(Duration::from_millis(10)).await;
        let imported = session.snapshot_run().expect("snapshot");
        assert_eq!(imported.run().requests.len(), 1);
        assert_eq!(imported.run().stages.len(), 1);
        assert!(!imported.run().runtime_snapshots.is_empty());
        assert!(imported
            .run()
            .metadata
            .effective_tokio_sampler_config
            .is_some());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn shutdown_preserves_tracing_events() {
        let session = TracingTokioSession::builder("svc")
            .sampler_interval(Duration::from_millis(1))
            .start()
            .expect("start");
        let subscriber = tracing_subscriber::registry().with(session.layer());
        tracing::subscriber::with_default(subscriber, || {
            let request = tracing::info_span!(
                "req",
                tt.kind = "request",
                tt.request_id = "r1",
                tt.route = "/a"
            );
            drop(request);
        });
        let imported = session.shutdown().await.expect("shutdown");
        assert_eq!(imported.run().requests.len(), 1);
    }

    #[test]
    fn start_outside_runtime_fails_clearly() {
        let err = TracingTokioSession::builder("svc")
            .start()
            .expect_err("should fail outside runtime");
        assert!(matches!(
            err,
            crate::tokio::TracingTokioSessionStartError::SamplerStart(
                tailtriage_tokio::SamplerStartError::MissingRuntime
            )
        ));
    }

    #[test]
    fn zero_interval_fails_clearly() {
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let err = rt.block_on(async {
            TracingTokioSession::builder("svc")
                .sampler_interval(Duration::ZERO)
                .start()
                .expect_err("zero interval should fail")
        });
        assert!(matches!(
            err,
            crate::tokio::TracingTokioSessionStartError::SamplerStart(
                tailtriage_tokio::SamplerStartError::ZeroInterval
            )
        ));
    }
}
