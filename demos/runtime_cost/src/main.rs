use std::hint::black_box;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{bail, Context};
use serde::Serialize;
use tailtriage_analyzer::{render_json_pretty, try_analyze_run, AnalyzeOptions};
use tailtriage_core::{
    CaptureLimitsOverride, CaptureMode, Outcome, RequestOptions, Run, Tailtriage,
};
use tailtriage_tokio::RuntimeSampler;
use tailtriage_tracing::{tokio::TracingTokioSession, TracingRecorder};
use tokio::sync::{Mutex, Semaphore};
use tracing::Instrument;
use tracing_subscriber::{layer::SubscriberExt, Registry};

const DEFAULT_REQUESTS: usize = 800;
const DEFAULT_CONCURRENCY: usize = 32;
const DEFAULT_WORK_MS: u64 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Mode {
    Baseline,
    BakedInNoRequestContext,
    CoreLight,
    CoreInvestigation,
    CoreLightTokioSampler,
    CoreInvestigationTokioSampler,
    CoreLightDropPath,
    CoreInvestigationDropPath,
    TracingLight,
    TracingLightTokioSampler,
    TracingLightDropPath,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum InstrumentationFamily {
    Baseline,
    Native,
    Tracing,
}

impl Mode {
    fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "baseline" => Self::Baseline,
            "baked_in_no_request_context" => Self::BakedInNoRequestContext,
            "core_light" => Self::CoreLight,
            "core_investigation" => Self::CoreInvestigation,
            "core_light_tokio_sampler" => Self::CoreLightTokioSampler,
            "core_investigation_tokio_sampler" => Self::CoreInvestigationTokioSampler,
            "core_light_drop_path" => Self::CoreLightDropPath,
            "core_investigation_drop_path" => Self::CoreInvestigationDropPath,
            "tracing_light" => Self::TracingLight,
            "tracing_light_tokio_sampler" => Self::TracingLightTokioSampler,
            "tracing_light_drop_path" => Self::TracingLightDropPath,
            _ => return None,
        })
    }
    fn instrumentation(self) -> InstrumentationFamily {
        match self {
            Self::Baseline => InstrumentationFamily::Baseline,
            Self::BakedInNoRequestContext
            | Self::CoreLight
            | Self::CoreInvestigation
            | Self::CoreLightTokioSampler
            | Self::CoreInvestigationTokioSampler
            | Self::CoreLightDropPath
            | Self::CoreInvestigationDropPath => InstrumentationFamily::Native,
            Self::TracingLight | Self::TracingLightTokioSampler | Self::TracingLightDropPath => {
                InstrumentationFamily::Tracing
            }
        }
    }
    fn core_mode(self) -> Option<CaptureMode> {
        match self {
            Self::Baseline
            | Self::TracingLight
            | Self::TracingLightTokioSampler
            | Self::TracingLightDropPath => None,
            Self::BakedInNoRequestContext
            | Self::CoreLight
            | Self::CoreLightTokioSampler
            | Self::CoreLightDropPath => Some(CaptureMode::Light),
            Self::CoreInvestigation
            | Self::CoreInvestigationTokioSampler
            | Self::CoreInvestigationDropPath => Some(CaptureMode::Investigation),
        }
    }
    fn uses_tokio_sampler(self) -> bool {
        matches!(
            self,
            Self::CoreLightTokioSampler
                | Self::CoreInvestigationTokioSampler
                | Self::TracingLightTokioSampler
        )
    }
    fn uses_drop_path_limits(self) -> bool {
        matches!(
            self,
            Self::CoreLightDropPath | Self::CoreInvestigationDropPath | Self::TracingLightDropPath
        )
    }
    fn omits_request_context(self) -> bool {
        matches!(self, Self::BakedInNoRequestContext)
    }
    fn supports_inflight(self) -> bool {
        matches!(self.instrumentation(), InstrumentationFamily::Native)
    }
    fn artifact_file_name(self) -> Option<&'static str> {
        Some(match self {
            Self::CoreLight => "run-core_light.json",
            Self::CoreInvestigation => "run-core_investigation.json",
            Self::CoreLightTokioSampler => "run-core_light_tokio_sampler.json",
            Self::CoreInvestigationTokioSampler => "run-core_investigation_tokio_sampler.json",
            Self::CoreLightDropPath => "run-core_light_drop_path.json",
            Self::CoreInvestigationDropPath => "run-core_investigation_drop_path.json",
            Self::TracingLight => "run-tracing_light.json",
            Self::TracingLightTokioSampler => "run-tracing_light_tokio_sampler.json",
            Self::TracingLightDropPath => "run-tracing_light_drop_path.json",
            _ => return None,
        })
    }
}

#[derive(Debug)]
struct Cli {
    mode: Mode,
    requests: usize,
    concurrency: usize,
    work_ms: u64,
    output_dir: PathBuf,
}
#[derive(Debug, Serialize)]
#[allow(clippy::struct_excessive_bools)]
struct Measurement {
    mode: Mode,
    instrumentation: InstrumentationFamily,
    uses_runtime_sampler: bool,
    uses_drop_path_limits: bool,
    inflight_supported: bool,
    requests: usize,
    concurrency: usize,
    work_ms: u64,
    throughput_rps: f64,
    latency_p50_ms: f64,
    latency_p95_ms: f64,
    latency_p99_ms: f64,
    run_requests: u64,
    run_stages: u64,
    run_queues: u64,
    runtime_snapshots: u64,
    artifact_finalize_ms: f64,
    analyze_ms: f64,
    report_render_ms: f64,
    effective_tokio_sampler_config_present: bool,
    drop_path_signal_present: bool,
    lifecycle_warning_count: u64,
    artifact_path: Option<String>,
    truncation: Option<TruncationMeasurement>,
}
#[derive(Debug, Serialize)]
struct TruncationMeasurement {
    dropped_requests: u64,
    dropped_stages: u64,
    dropped_queues: u64,
    dropped_inflight_snapshots: u64,
    dropped_runtime_snapshots: u64,
    limits_reached: bool,
}

struct NativeInst {
    tailtriage: Arc<Tailtriage>,
    sampler: Option<RuntimeSampler>,
}
struct TracingInst {
    recorder: Arc<TracingRecorder>,
    session: Option<TracingTokioSession>,
}
enum Backend {
    None,
    Native(NativeInst),
    Tracing(TracingInst),
}

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() -> anyhow::Result<()> {
    let cli = parse_cli()?;
    std::fs::create_dir_all(&cli.output_dir)?;
    let mut backend = build_backend(&cli)?;
    let (mut latencies, elapsed) = run_requests(&cli, &backend).await?;
    latencies.sort_unstable();
    let finalize_start = Instant::now();
    let run = finalize_backend(&mut backend, &cli).await?;
    let artifact_finalize_ms = finalize_start.elapsed().as_secs_f64() * 1000.0;
    let (
        run_requests,
        run_stages,
        run_queues,
        runtime_snapshots,
        effective_tokio_sampler_config_present,
        truncation,
        lifecycle_warning_count,
        drop_path_signal_present,
        analyze_ms,
        report_render_ms,
    ) = if let Some(run) = run.as_ref() {
        let t = run.truncation.clone();
        let trunc = Some(TruncationMeasurement {
            dropped_requests: t.dropped_requests,
            dropped_stages: t.dropped_stages,
            dropped_queues: t.dropped_queues,
            dropped_inflight_snapshots: t.dropped_inflight_snapshots,
            dropped_runtime_snapshots: t.dropped_runtime_snapshots,
            limits_reached: t.limits_hit,
        });
        let lifecycle_warning_count =
            u64::try_from(run.metadata.lifecycle_warnings.len()).unwrap_or(u64::MAX);
        let drop_signal = t.limits_hit
            || lifecycle_warning_count > 0
            || t.dropped_requests > 0
            || t.dropped_stages > 0
            || t.dropped_queues > 0
            || t.dropped_inflight_snapshots > 0
            || t.dropped_runtime_snapshots > 0;
        let analyze_start = Instant::now();
        let report = try_analyze_run(run, AnalyzeOptions::default())?;
        let analyze_ms = analyze_start.elapsed().as_secs_f64() * 1000.0;
        let render_start = Instant::now();
        let rendered = render_json_pretty(&report)?;
        black_box(rendered);
        let report_render_ms = render_start.elapsed().as_secs_f64() * 1000.0;
        (
            u64::try_from(run.requests.len()).unwrap_or(u64::MAX),
            u64::try_from(run.stages.len()).unwrap_or(u64::MAX),
            u64::try_from(run.queues.len()).unwrap_or(u64::MAX),
            u64::try_from(run.runtime_snapshots.len()).unwrap_or(u64::MAX),
            run.metadata.effective_tokio_sampler_config.is_some(),
            trunc,
            lifecycle_warning_count,
            drop_signal,
            analyze_ms,
            report_render_ms,
        )
    } else {
        (0, 0, 0, 0, false, None, 0, false, 0.0, 0.0)
    };
    let measurement = Measurement {
        mode: cli.mode,
        instrumentation: cli.mode.instrumentation(),
        uses_runtime_sampler: cli.mode.uses_tokio_sampler(),
        uses_drop_path_limits: cli.mode.uses_drop_path_limits(),
        inflight_supported: cli.mode.supports_inflight(),
        requests: cli.requests,
        concurrency: cli.concurrency,
        work_ms: cli.work_ms,
        throughput_rps: requests_per_second(cli.requests, elapsed)?,
        latency_p50_ms: percentile_ms(&latencies, 50, 100)?,
        latency_p95_ms: percentile_ms(&latencies, 95, 100)?,
        latency_p99_ms: percentile_ms(&latencies, 99, 100)?,
        run_requests,
        run_stages,
        run_queues,
        runtime_snapshots,
        artifact_finalize_ms,
        analyze_ms,
        report_render_ms,
        effective_tokio_sampler_config_present,
        drop_path_signal_present,
        lifecycle_warning_count,
        artifact_path: cli
            .mode
            .artifact_file_name()
            .map(|n| cli.output_dir.join(n).display().to_string()),
        truncation,
    };
    println!("{}", serde_json::to_string(&measurement)?);
    Ok(())
}

fn build_backend(cli: &Cli) -> anyhow::Result<Backend> {
    match cli.mode.instrumentation() {
        InstrumentationFamily::Baseline => Ok(Backend::None),
        InstrumentationFamily::Native => {
            let mut b = Tailtriage::builder("runtime_cost_demo")
                .output(cli.output_dir.join(cli.mode.artifact_file_name().unwrap()));
            b = match cli.mode.core_mode().unwrap() {
                CaptureMode::Light => b.light(),
                CaptureMode::Investigation => b.investigation(),
            };
            if cli.mode.uses_drop_path_limits() {
                b = b.capture_limits_override(CaptureLimitsOverride {
                    max_requests: Some(64),
                    max_stages: Some(64),
                    max_queues: Some(64),
                    max_inflight_snapshots: Some(64),
                    max_runtime_snapshots: Some(64),
                });
            }
            let tt = Arc::new(b.build()?);
            let sampler = if cli.mode.uses_tokio_sampler() {
                Some(RuntimeSampler::builder(Arc::clone(&tt)).start()?)
            } else {
                None
            };
            Ok(Backend::Native(NativeInst {
                tailtriage: tt,
                sampler,
            }))
        }
        InstrumentationFamily::Tracing => {
            let recorder = if cli.mode.uses_drop_path_limits() {
                Arc::new(
                    TracingRecorder::builder("runtime_cost_demo")
                        .max_open_spans(64)
                        .max_completed_spans(64)
                        .build(),
                )
            } else {
                Arc::new(TracingRecorder::builder("runtime_cost_demo").build())
            };
            if cli.mode.uses_tokio_sampler() {
                let session = TracingTokioSession::builder("runtime_cost_demo")
                    .strict(false)
                    .start()?;
                let subscriber = Registry::default().with(session.layer());
                tracing::subscriber::set_global_default(subscriber).map_err(|e| {
                    anyhow::anyhow!("failed to install tracing tokio session subscriber: {e}")
                })?;
                Ok(Backend::Tracing(TracingInst {
                    recorder,
                    session: Some(session),
                }))
            } else {
                let subscriber = Registry::default().with(recorder.layer());
                tracing::subscriber::set_global_default(subscriber)
                    .map_err(|e| anyhow::anyhow!("failed to install tracing subscriber: {e}"))?;
                Ok(Backend::Tracing(TracingInst {
                    recorder,
                    session: None,
                }))
            }
        }
    }
}

async fn finalize_backend(backend: &mut Backend, cli: &Cli) -> anyhow::Result<Option<Run>> {
    match backend {
        Backend::None => Ok(None),
        Backend::Native(inst) => {
            if let Some(s) = inst.sampler.take() {
                s.shutdown().await;
            }
            let run = inst.tailtriage.snapshot();
            inst.tailtriage.shutdown()?;
            Ok(Some(run))
        }
        Backend::Tracing(inst) => {
            let run = if let Some(session) = inst.session.take() {
                session.shutdown().await?.into_parts().0
            } else {
                inst.recorder.snapshot_run()?.into_parts().0
            };
            let path = cli.output_dir.join(
                cli.mode
                    .artifact_file_name()
                    .expect("artifact for tracing mode"),
            );
            let f = std::fs::File::create(path)?;
            serde_json::to_writer_pretty(f, &run)?;
            Ok(Some(run))
        }
    }
}

async fn run_requests(cli: &Cli, backend: &Backend) -> anyhow::Result<(Vec<u64>, Duration)> {
    let latencies_us = Arc::new(Mutex::new(Vec::with_capacity(cli.requests)));
    let semaphore = Arc::new(Semaphore::new(cli.concurrency));
    let wall_start = Instant::now();
    let mut tasks = Vec::with_capacity(cli.requests);
    for idx in 0..cli.requests {
        let sem = Arc::clone(&semaphore);
        let latencies = Arc::clone(&latencies_us);
        let work_duration = Duration::from_millis(cli.work_ms);
        let mode = cli.mode;
        let tt = match backend {
            Backend::Native(inst) => Some(Arc::clone(&inst.tailtriage)),
            _ => None,
        };
        tasks.push(tokio::spawn(async move { let start=Instant::now(); match mode.instrumentation() { InstrumentationFamily::Baseline=>{ let permit=sem.acquire().await.expect("semaphore closed"); tokio::time::sleep(work_duration).await; drop(permit);}, InstrumentationFamily::Native if mode.omits_request_context()=>{ let permit=sem.acquire().await.expect("semaphore closed"); tokio::time::sleep(work_duration).await; drop(permit);}, InstrumentationFamily::Native=>{ let ts=tt.expect("collector"); let request_id=format!("request-{idx}"); let started=ts.begin_request_with("/runtime-cost",RequestOptions::new().request_id(request_id)); let request=started.handle.clone(); let _inflight=request.inflight("runtime_cost_requests"); let permit=request.queue("worker_semaphore").await_on(sem.acquire()).await.expect("semaphore closed"); request.stage("simulated_work").await_value(tokio::time::sleep(work_duration)).await; drop(permit); started.completion.finish(Outcome::Ok); }, InstrumentationFamily::Tracing=>{ let rid=format!("request-{idx}"); async { let permit= async { sem.acquire().await.expect("semaphore closed") }.instrument(tracing::info_span!("tt.queue", tt.route="/runtime-cost", tt.request_id=%rid, tt.queue.name="worker_semaphore", tt.depth_at_start=0_u64)).await; tokio::time::sleep(work_duration).instrument(tracing::info_span!("tt.stage", tt.route="/runtime-cost", tt.request_id=%rid, tt.stage.name="simulated_work", tt.outcome="ok")).await; drop(permit);} .instrument(tracing::info_span!("tt.request", tt.route="/runtime-cost", tt.request_id=%rid, tt.outcome="ok")).await; } }
 let elapsed_us=u64::try_from(start.elapsed().as_micros()).unwrap_or(u64::MAX); latencies.lock().await.push(elapsed_us); }));
    }
    for task in tasks {
        task.await.context("request task panicked")?;
    }
    let elapsed = wall_start.elapsed();
    let l = Arc::into_inner(latencies_us)
        .expect("all refs dropped")
        .into_inner();
    Ok((l, elapsed))
}

// parse/help etc unchanged shortened
fn parse_cli() -> anyhow::Result<Cli> {
    let mut mode = None;
    let mut requests = DEFAULT_REQUESTS;
    let mut concurrency = DEFAULT_CONCURRENCY;
    let mut work_ms = DEFAULT_WORK_MS;
    let mut output_dir = PathBuf::from("demos/runtime_cost/artifacts");
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--mode" => {
                let value = args.next().context("missing value for --mode")?;
                mode = Mode::parse(&value);
                if mode.is_none() {
                    bail!("invalid --mode {value}; expected baseline|baked_in_no_request_context|core_light|core_investigation|core_light_tokio_sampler|core_investigation_tokio_sampler|core_light_drop_path|core_investigation_drop_path|tracing_light|tracing_light_tokio_sampler|tracing_light_drop_path");
                }
            }
            "--requests" => {
                requests = args
                    .next()
                    .context("missing value for --requests")?
                    .parse()
                    .context("invalid integer for --requests")?;
            }
            "--concurrency" => {
                concurrency = args
                    .next()
                    .context("missing value for --concurrency")?
                    .parse()
                    .context("invalid integer for --concurrency")?;
            }
            "--work-ms" => {
                work_ms = args
                    .next()
                    .context("missing value for --work-ms")?
                    .parse()
                    .context("invalid integer for --work-ms")?;
            }
            "--output-dir" => {
                output_dir = PathBuf::from(args.next().context("missing value for --output-dir")?);
            }
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            _ => bail!("unknown arg: {arg}"),
        }
    }
    let mode = mode.context("--mode is required")?;
    if requests == 0 || concurrency == 0 || work_ms == 0 {
        bail!("--requests, --concurrency, and --work-ms must be > 0");
    }
    Ok(Cli {
        mode,
        requests,
        concurrency,
        work_ms,
        output_dir,
    })
}
fn print_help() {
    eprintln!("runtime_cost --mode <baseline|baked_in_no_request_context|core_light|core_investigation|core_light_tokio_sampler|core_investigation_tokio_sampler|core_light_drop_path|core_investigation_drop_path|tracing_light|tracing_light_tokio_sampler|tracing_light_drop_path> [--requests N] [--concurrency N] [--work-ms N] [--output-dir DIR]");
}
fn requests_per_second(request_count: usize, elapsed: Duration) -> anyhow::Result<f64> {
    let total_requests = u64::try_from(request_count)?;
    Ok(total_requests.to_string().parse::<f64>()? / elapsed.as_secs_f64())
}
fn percentile_ms(sorted_us: &[u64], numerator: u64, denominator: u64) -> anyhow::Result<f64> {
    if sorted_us.is_empty() {
        return Ok(0.0);
    }
    anyhow::ensure!(denominator != 0, "percentile denominator must be non-zero");
    anyhow::ensure!(
        numerator <= denominator,
        "percentile numerator must be <= denominator"
    );
    let max_index = sorted_us.len() - 1;
    let max_index_u64 = u64::try_from(max_index)?;
    let scaled = u128::from(max_index_u64) * u128::from(numerator);
    let rounded = scaled + (u128::from(denominator) / 2);
    let index = usize::try_from(rounded / u128::from(denominator))?;
    micros_to_millis_f64(sorted_us[index])
}
fn micros_to_millis_f64(micros: u64) -> anyhow::Result<f64> {
    Ok(micros.to_string().parse::<f64>()? / 1_000.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mode_parse_accepts_tracing_modes() {
        assert_eq!(Mode::parse("tracing_light"), Some(Mode::TracingLight));
        assert_eq!(
            Mode::parse("tracing_light_tokio_sampler"),
            Some(Mode::TracingLightTokioSampler)
        );
        assert_eq!(
            Mode::parse("tracing_light_drop_path"),
            Some(Mode::TracingLightDropPath)
        );
    }
    #[test]
    fn mode_parse_rejects_unknown() {
        assert_eq!(Mode::parse("wat"), None);
    }
    #[test]
    fn mode_classification() {
        assert_eq!(
            Mode::TracingLight.instrumentation(),
            InstrumentationFamily::Tracing
        );
        assert!(Mode::TracingLightTokioSampler.uses_tokio_sampler());
        assert!(Mode::TracingLightDropPath.uses_drop_path_limits());
    }
}
