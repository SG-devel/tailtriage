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
use tailtriage_tracing::tokio::TracingTokioSession;
use tailtriage_tracing::TracingRecorder;
use tokio::sync::{Mutex, Semaphore};
use tracing::Instrument;
use tracing_subscriber::prelude::*;

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
enum InstrumentationKind {
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
    fn instrumentation(self) -> InstrumentationKind {
        match self {
            Self::Baseline => InstrumentationKind::Baseline,
            Self::TracingLight | Self::TracingLightTokioSampler | Self::TracingLightDropPath => {
                InstrumentationKind::Tracing
            }
            _ => InstrumentationKind::Native,
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
    fn artifact_name(self) -> Option<&'static str> {
        match self {
            Self::Baseline
            | Self::BakedInNoRequestContext
            | Self::CoreInvestigation
            | Self::CoreInvestigationTokioSampler
            | Self::CoreInvestigationDropPath => None,
            Self::CoreLight => Some("run-core_light.json"),
            Self::CoreLightTokioSampler => Some("run-core_light_tokio_sampler.json"),
            Self::CoreLightDropPath => Some("run-core_light_drop_path.json"),
            Self::TracingLight => Some("run-tracing_light.json"),
            Self::TracingLightTokioSampler => Some("run-tracing_light_tokio_sampler.json"),
            Self::TracingLightDropPath => Some("run-tracing_light_drop_path.json"),
        }
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
    instrumentation: InstrumentationKind,
    uses_runtime_sampler: bool,
    uses_drop_path_limits: bool,
    requests: usize,
    concurrency: usize,
    work_ms: u64,
    throughput_rps: f64,
    latency_p50_ms: f64,
    latency_p95_ms: f64,
    latency_p99_ms: f64,
    truncation: Option<TruncationMeasurement>,
    run_requests: u64,
    run_stages: u64,
    run_queues: u64,
    runtime_snapshots: u64,
    artifact_finalize_ms: f64,
    analyze_ms: f64,
    report_render_ms: f64,
    effective_tokio_sampler_config_present: bool,
    inflight_supported: bool,
    artifact_path: Option<String>,
    drop_path_signal_present: bool,
    lifecycle_warning_count: u64,
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

struct Instr {
    tailtriage: Option<Arc<Tailtriage>>,
    sampler: Option<RuntimeSampler>,
    tracing: Option<TracingRecorder>,
    tokio_session: Option<TracingTokioSession>,
}

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() -> anyhow::Result<()> {
    let cli = parse_cli()?;
    std::fs::create_dir_all(&cli.output_dir)?;
    let mut instr = build_instrumentation(&cli)?;
    let (mut latencies, elapsed) = run_requests(&cli, &instr).await?;
    if let Some(s) = instr.sampler.take() {
        s.shutdown().await;
    }
    latencies.sort_unstable();
    let finalize_start = Instant::now();
    let (run, trunc, artifact_path) = finalize_run(&cli, &mut instr).await?;
    let artifact_finalize_ms = finalize_start.elapsed().as_secs_f64() * 1000.0;
    let (
        run_requests,
        run_stages,
        run_queues,
        runtime_snapshots,
        effective_tokio_sampler_config_present,
        lifecycle_warning_count,
        drop_path_signal_present,
    ) = if let Some(r) = run.as_ref() {
        (
            r.requests.len() as u64,
            r.stages.len() as u64,
            r.queues.len() as u64,
            r.runtime_snapshots.len() as u64,
            r.metadata.effective_tokio_sampler_config.is_some(),
            r.metadata.lifecycle_warnings.len() as u64,
            has_drop_signal(r),
        )
    } else {
        (0, 0, 0, 0, false, 0, false)
    };
    let (analyze_ms, report_render_ms) = if let Some(r) = run.as_ref() {
        let t = Instant::now();
        let report = try_analyze_run(r, AnalyzeOptions::default())?;
        let analyze = t.elapsed().as_secs_f64() * 1000.0;
        let t2 = Instant::now();
        let out = render_json_pretty(&report)?;
        black_box(out);
        (analyze, t2.elapsed().as_secs_f64() * 1000.0)
    } else {
        (0.0, 0.0)
    };
    let measurement = Measurement {
        mode: cli.mode,
        instrumentation: cli.mode.instrumentation(),
        uses_runtime_sampler: cli.mode.uses_tokio_sampler(),
        uses_drop_path_limits: cli.mode.uses_drop_path_limits(),
        requests: cli.requests,
        concurrency: cli.concurrency,
        work_ms: cli.work_ms,
        throughput_rps: requests_per_second(cli.requests, elapsed)?,
        latency_p50_ms: percentile_ms(&latencies, 50, 100)?,
        latency_p95_ms: percentile_ms(&latencies, 95, 100)?,
        latency_p99_ms: percentile_ms(&latencies, 99, 100)?,
        truncation: trunc,
        run_requests,
        run_stages,
        run_queues,
        runtime_snapshots,
        artifact_finalize_ms,
        analyze_ms,
        report_render_ms,
        effective_tokio_sampler_config_present,
        inflight_supported: cli.mode.instrumentation() != InstrumentationKind::Tracing,
        artifact_path: artifact_path.map(|p| p.display().to_string()),
        drop_path_signal_present,
        lifecycle_warning_count,
    };
    println!("{}", serde_json::to_string(&measurement)?);
    Ok(())
}

fn has_drop_signal(run: &Run) -> bool {
    let t = &run.truncation;
    t.limits_hit
        || t.dropped_requests > 0
        || t.dropped_stages > 0
        || t.dropped_queues > 0
        || t.dropped_inflight_snapshots > 0
        || t.dropped_runtime_snapshots > 0
        || !run.metadata.lifecycle_warnings.is_empty()
}
async fn finalize_run(
    cli: &Cli,
    instr: &mut Instr,
) -> anyhow::Result<(Option<Run>, Option<TruncationMeasurement>, Option<PathBuf>)> {
    match cli.mode.instrumentation() {
        InstrumentationKind::Baseline => Ok((None, None, None)),
        InstrumentationKind::Native => {
            let Some(tt) = instr.tailtriage.take() else {
                bail!("missing tailtriage")
            };
            let run = tt.snapshot();
            let trunc = Some(TruncationMeasurement {
                dropped_requests: run.truncation.dropped_requests,
                dropped_stages: run.truncation.dropped_stages,
                dropped_queues: run.truncation.dropped_queues,
                dropped_inflight_snapshots: run.truncation.dropped_inflight_snapshots,
                dropped_runtime_snapshots: run.truncation.dropped_runtime_snapshots,
                limits_reached: run.truncation.limits_hit,
            });
            tt.shutdown()?;
            Ok((
                Some(run),
                trunc,
                cli.mode.artifact_name().map(|n| cli.output_dir.join(n)),
            ))
        }
        InstrumentationKind::Tracing => {
            let run = if let Some(s) = instr.tokio_session.take() {
                s.shutdown().await?.into_parts().0
            } else if let Some(r) = instr.tracing.take() {
                r.shutdown()?.into_parts().0
            } else {
                bail!("missing tracing recorder/session")
            };
            let trunc = Some(TruncationMeasurement {
                dropped_requests: run.truncation.dropped_requests,
                dropped_stages: run.truncation.dropped_stages,
                dropped_queues: run.truncation.dropped_queues,
                dropped_inflight_snapshots: run.truncation.dropped_inflight_snapshots,
                dropped_runtime_snapshots: run.truncation.dropped_runtime_snapshots,
                limits_reached: run.truncation.limits_hit,
            });
            let path = cli.mode.artifact_name().map(|n| cli.output_dir.join(n));
            if let Some(p) = path.as_ref() {
                let f = std::fs::File::create(p)?;
                serde_json::to_writer_pretty(f, &run)?;
            }
            Ok((Some(run), trunc, path))
        }
    }
}

fn build_instrumentation(cli: &Cli) -> anyhow::Result<Instr> {
    match cli.mode.instrumentation() {
        InstrumentationKind::Baseline => Ok(Instr {
            tailtriage: None,
            sampler: None,
            tracing: None,
            tokio_session: None,
        }),
        InstrumentationKind::Native => {
            let Some(capture_mode) = cli.mode.core_mode() else {
                bail!("native mode missing capture mode")
            };
            let mut b = Tailtriage::builder("runtime_cost_demo");
            if let Some(n) = cli.mode.artifact_name() {
                b = b.output(cli.output_dir.join(n));
            }
            b = match capture_mode {
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
            Ok(Instr {
                tailtriage: Some(tt),
                sampler,
                tracing: None,
                tokio_session: None,
            })
        }
        InstrumentationKind::Tracing => {
            if cli.mode == Mode::TracingLightTokioSampler {
                let mut session = TracingTokioSession::builder("runtime_cost_demo");
                if cli.mode.uses_drop_path_limits() {
                    session = session
                        .max_open_spans(64)
                        .max_completed_spans(64)
                        .max_runtime_snapshots(64);
                }
                let session = session.start()?;
                tracing::subscriber::set_global_default(
                    tracing_subscriber::registry().with(session.layer().clone()),
                )
                .map_err(|e| anyhow::anyhow!("failed to install global tracing subscriber: {e}"))?;
                Ok(Instr {
                    tailtriage: None,
                    sampler: None,
                    tracing: None,
                    tokio_session: Some(session),
                })
            } else {
                let mut rec = TracingRecorder::builder("runtime_cost_demo");
                if cli.mode.uses_drop_path_limits() {
                    rec = rec.max_open_spans(64).max_completed_spans(64);
                }
                let recorder = rec.build(); // One mode runs per process, so process-global subscriber install is safe here.
                tracing::subscriber::set_global_default(
                    tracing_subscriber::registry().with(recorder.layer().clone()),
                )
                .map_err(|e| anyhow::anyhow!("failed to install global tracing subscriber: {e}"))?;
                Ok(Instr {
                    tailtriage: None,
                    sampler: None,
                    tracing: Some(recorder),
                    tokio_session: None,
                })
            }
        }
    }
}

async fn run_requests(cli: &Cli, instr: &Instr) -> anyhow::Result<(Vec<u64>, Duration)> {
    let latencies_us = Arc::new(Mutex::new(Vec::<u64>::with_capacity(cli.requests)));
    let semaphore = Arc::new(Semaphore::new(cli.concurrency));
    let wall_start = Instant::now();
    let mut tasks = Vec::with_capacity(cli.requests);
    for idx in 0..cli.requests {
        let sem = Arc::clone(&semaphore);
        let lat = Arc::clone(&latencies_us);
        let mode = cli.mode;
        let work_duration = Duration::from_millis(cli.work_ms);
        let tt = instr.tailtriage.as_ref().map(Arc::clone);
        tasks.push(tokio::spawn(async move{let start=Instant::now(); match mode.instrumentation(){InstrumentationKind::Baseline=>{let permit=sem.acquire().await.expect("semaphore closed"); tokio::time::sleep(work_duration).await; drop(permit);},InstrumentationKind::Native=>{if mode.omits_request_context(){let permit=sem.acquire().await.expect("semaphore closed"); tokio::time::sleep(work_duration).await; drop(permit);} else {let ts=tt.expect("collector"); let request=ts.begin_request_with("/runtime-cost",RequestOptions::new().request_id(format!("request-{idx}"))); let handle=request.handle.clone(); let _inflight=handle.inflight("runtime_cost_requests"); let permit=handle.queue("worker_semaphore").await_on(sem.acquire()).await.expect("semaphore closed"); handle.stage("simulated_work").await_value(tokio::time::sleep(work_duration)).await; drop(permit); request.completion.finish(Outcome::Ok);}},InstrumentationKind::Tracing=>{let request_span=tracing::info_span!("tt.request", tt.route="/runtime-cost", tt.request_id=%format!("request-{idx}"), tt.outcome="ok"); async move{ let queue_span=tracing::info_span!("tt.queue", tt.queue="worker_semaphore", tt.depth_at_start=0i64); let permit=async { sem.acquire().await.expect("semaphore closed") }.instrument(queue_span).await; let stage_span=tracing::info_span!("tt.stage", tt.stage="simulated_work"); tokio::time::sleep(work_duration).instrument(stage_span).await; drop(permit); }.instrument(request_span).await;}} let elapsed_us=u64::try_from(start.elapsed().as_micros()).unwrap_or(u64::MAX); lat.lock().await.push(elapsed_us);}));
    }
    for task in tasks {
        task.await.context("request task panicked")?;
    }
    let elapsed = wall_start.elapsed();
    let latencies = Arc::into_inner(latencies_us)
        .expect("all task refs dropped")
        .into_inner();
    Ok((latencies, elapsed))
}

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
                    bail!("invalid --mode {value}");
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
    eprintln!("runtime_cost --mode <baseline|...|tracing_light|tracing_light_tokio_sampler|tracing_light_drop_path> [--requests N] [--concurrency N] [--work-ms N] [--output-dir DIR]");
}
fn requests_per_second(request_count: usize, elapsed: Duration) -> anyhow::Result<f64> {
    let total_requests = u64::try_from(request_count)?;
    let request_rate_input = total_requests.to_string().parse::<f64>()?;
    Ok(request_rate_input / elapsed.as_secs_f64())
}
fn percentile_ms(sorted_us: &[u64], numerator: u64, denominator: u64) -> anyhow::Result<f64> {
    if sorted_us.is_empty() {
        return Ok(0.0);
    }
    anyhow::ensure!(denominator != 0);
    anyhow::ensure!(numerator <= denominator);
    let max_index = sorted_us.len() - 1;
    let max_index_u64 = u64::try_from(max_index)?;
    let scaled = u128::from(max_index_u64) * u128::from(numerator);
    let rounded = scaled + (u128::from(denominator) / 2);
    let index = usize::try_from(rounded / u128::from(denominator))?;
    let micros_value = sorted_us[index].to_string().parse::<f64>()?;
    Ok(micros_value / 1_000.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parse_new_modes() {
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
    fn reject_unknown_mode() {
        assert_eq!(Mode::parse("unknown"), None);
    }
    #[test]
    fn mode_classification() {
        assert_eq!(
            Mode::TracingLight.instrumentation(),
            InstrumentationKind::Tracing
        );
        assert!(Mode::TracingLightTokioSampler.uses_tokio_sampler());
        assert!(Mode::TracingLightDropPath.uses_drop_path_limits());
    }
}
