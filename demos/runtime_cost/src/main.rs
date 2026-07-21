use std::hint::black_box;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context};
use serde::Serialize;
use tailtriage_analyzer::{render_json_pretty, try_analyze_run, AnalyzeOptions};
use tailtriage_core::{CaptureLimitsOverride, CaptureMode, MemorySink, Run, Tailtriage};
use tailtriage_tokio::RuntimeSampler;
use tailtriage_tracing::TracingSession;
use tokio::sync::{Mutex, Semaphore};
use tracing::Instrument;
use tracing_subscriber::layer::SubscriberExt;

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
    fn parse(v: &str) -> Option<Self> {
        Some(match v {
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
    fn core_mode(self) -> Option<CaptureMode> {
        match self {
            Self::CoreLight
            | Self::CoreLightTokioSampler
            | Self::CoreLightDropPath
            | Self::BakedInNoRequestContext => Some(CaptureMode::Light),
            Self::CoreInvestigation
            | Self::CoreInvestigationTokioSampler
            | Self::CoreInvestigationDropPath => Some(CaptureMode::Investigation),
            _ => None,
        }
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
    fn uses_runtime_sampler(self) -> bool {
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
    fn artifact_file_name(self) -> Option<&'static str> {
        Some(match self {
            Self::Baseline => return None,
            Self::BakedInNoRequestContext => "run-baked_in_no_request_context.json",
            Self::CoreLight => "run-core_light.json",
            Self::CoreInvestigation => "run-core_investigation.json",
            Self::CoreLightTokioSampler => "run-core_light_tokio_sampler.json",
            Self::CoreInvestigationTokioSampler => "run-core_investigation_tokio_sampler.json",
            Self::CoreLightDropPath => "run-core_light_drop_path.json",
            Self::CoreInvestigationDropPath => "run-core_investigation_drop_path.json",
            Self::TracingLight => "run-tracing_light.json",
            Self::TracingLightTokioSampler => "run-tracing_light_tokio_sampler.json",
            Self::TracingLightDropPath => "run-tracing_light_drop_path.json",
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
    instrumentation: InstrumentationKind,
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

enum Backend {
    None,
    Native {
        tailtriage: Arc<Tailtriage>,
        sampler: Option<RuntimeSampler>,
        sink: MemorySink,
    },
    TracingSession {
        session: TracingSession,
    },
    TracingTokio {
        session: TracingSession,
    },
}

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() -> anyhow::Result<()> {
    let cli = parse_cli()?;
    std::fs::create_dir_all(&cli.output_dir)?;
    let mut backend = build_backend(&cli)?;
    let (mut latencies, elapsed) = run_requests(&cli, &backend).await?;
    let finalize_start = Instant::now();
    let (run, artifact_path) = finalize_backend_and_write_artifact(&cli, &mut backend).await?;
    let artifact_finalize_ms = finalize_start.elapsed().as_secs_f64() * 1000.0;
    let (analyze_ms, report_render_ms) = measure_analysis_and_render_ms(run.as_ref())?;
    latencies.sort_unstable();
    let truncation = run.as_ref().map(|r| TruncationMeasurement {
        dropped_requests: r.truncation.dropped_requests,
        dropped_stages: r.truncation.dropped_stages,
        dropped_queues: r.truncation.dropped_queues,
        dropped_inflight_snapshots: r.truncation.dropped_inflight_snapshots,
        dropped_runtime_snapshots: r.truncation.dropped_runtime_snapshots,
        limits_reached: r.truncation.limits_hit,
    });
    let run_requests = run.as_ref().map_or(0, |r| r.requests.len() as u64);
    let run_stages = run.as_ref().map_or(0, |r| r.stages.len() as u64);
    let run_queues = run.as_ref().map_or(0, |r| r.queues.len() as u64);
    let runtime_snapshots = run.as_ref().map_or(0, |r| r.runtime_snapshots.len() as u64);
    let effective_tokio_sampler_config_present = run
        .as_ref()
        .is_some_and(|r| r.metadata.effective_tokio_sampler_config.is_some());
    let lifecycle_warning_count = run
        .as_ref()
        .map_or(0, |r| r.metadata.lifecycle_warnings.len() as u64);
    let drop_path_signal_present = truncation.as_ref().is_some_and(|t| {
        t.limits_reached
            || t.dropped_requests > 0
            || t.dropped_stages > 0
            || t.dropped_queues > 0
            || t.dropped_inflight_snapshots > 0
            || t.dropped_runtime_snapshots > 0
    }) || lifecycle_warning_count > 0;
    let m = Measurement {
        mode: cli.mode,
        instrumentation: cli.mode.instrumentation(),
        uses_runtime_sampler: cli.mode.uses_runtime_sampler(),
        uses_drop_path_limits: cli.mode.uses_drop_path_limits(),
        inflight_supported: matches!(cli.mode.instrumentation(), InstrumentationKind::Native),
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
        artifact_path,
        truncation,
    };
    println!("{}", serde_json::to_string(&m)?);
    Ok(())
}

fn build_backend(cli: &Cli) -> anyhow::Result<Backend> {
    match cli.mode.instrumentation() {
        InstrumentationKind::Baseline => Ok(Backend::None),
        InstrumentationKind::Native => {
            let mode = cli
                .mode
                .core_mode()
                .ok_or_else(|| anyhow!("missing capture mode"))?;
            let sink = MemorySink::new();
            let mut b = Tailtriage::builder("runtime_cost_demo").sink(sink.clone());
            b = match mode {
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
            let sampler = if cli.mode.uses_runtime_sampler() {
                Some(RuntimeSampler::builder(Arc::clone(&tt)).start()?)
            } else {
                None
            };
            Ok(Backend::Native {
                tailtriage: tt,
                sampler,
                sink,
            })
        }
        InstrumentationKind::Tracing => {
            if cli.mode.uses_runtime_sampler() {
                let mut session_builder =
                    TracingSession::builder("runtime_cost_demo").strict(false);
                if cli.mode.uses_drop_path_limits() {
                    session_builder =
                        session_builder.capture_limits_override(CaptureLimitsOverride {
                            max_requests: Some(64),
                            max_stages: Some(64),
                            max_queues: Some(64),
                            max_inflight_snapshots: Some(64),
                            max_runtime_snapshots: Some(64),
                        });
                }
                let session = session_builder
                    .sampler_interval(Duration::from_millis(10))
                    .build()?;
                // One mode runs per process in this demo, so process-global subscriber init is acceptable.
                tracing::subscriber::set_global_default(
                    tracing_subscriber::registry().with(session.layer()),
                )
                .map_err(|e| anyhow!("failed installing tracing Tokio session subscriber: {e}"))?;
                Ok(Backend::TracingTokio { session })
            } else {
                let mut b = TracingSession::builder("runtime_cost_demo").strict(false);
                if cli.mode.uses_drop_path_limits() {
                    b = b.max_open_spans(64);
                }
                let rec = b.build()?;
                // One mode runs per process in this demo, so process-global subscriber init is acceptable.
                tracing::subscriber::set_global_default(
                    tracing_subscriber::registry().with(rec.layer()),
                )
                .map_err(|e| anyhow!("failed installing tracing recorder subscriber: {e}"))?;
                Ok(Backend::TracingSession { session: rec })
            }
        }
    }
}

async fn finalize_backend_and_write_artifact(
    cli: &Cli,
    b: &mut Backend,
) -> anyhow::Result<(Option<Run>, Option<String>)> {
    let run = match std::mem::replace(b, Backend::None) {
        Backend::None => None,
        Backend::Native {
            tailtriage,
            sampler,
            sink,
        } => {
            if let Some(s) = sampler {
                s.shutdown().await;
            }
            tailtriage.shutdown()?;
            Some(sink.last_run().ok_or_else(|| {
                anyhow!("native runtime-cost run sink did not receive finalized run")
            })?)
        }
        Backend::TracingSession { session } | Backend::TracingTokio { session } => {
            Some(session.shutdown().await?.into_parts().0)
        }
    };
    let artifact_path = write_run_artifact(cli, run.as_ref())?;
    Ok((run, artifact_path))
}
fn write_run_artifact(cli: &Cli, run: Option<&Run>) -> anyhow::Result<Option<String>> {
    let Some(run) = run else { return Ok(None) };
    let path = cli.output_dir.join(
        cli.mode
            .artifact_file_name()
            .context("missing artifact file name")?,
    );
    let file = std::fs::File::create(&path)?;
    serde_json::to_writer_pretty(file, run)?;
    Ok(Some(path.display().to_string()))
}

fn measure_analysis_and_render_ms(run: Option<&Run>) -> anyhow::Result<(f64, f64)> {
    let Some(run_value) = run else {
        return Ok((0.0, 0.0));
    };
    if run_value.requests.is_empty() {
        return Ok((0.0, 0.0));
    }
    let analyze_start = Instant::now();
    let report = try_analyze_run(run_value, AnalyzeOptions::default())?;
    let analyze_ms = analyze_start.elapsed().as_secs_f64() * 1000.0;
    let render_start = Instant::now();
    black_box(render_json_pretty(&report)?);
    Ok((analyze_ms, render_start.elapsed().as_secs_f64() * 1000.0))
}

async fn run_requests(cli: &Cli, b: &Backend) -> anyhow::Result<(Vec<u64>, Duration)> {
    let native_tailtriage = match b {
        Backend::Native { tailtriage, .. } => Some(Arc::clone(tailtriage)),
        _ => None,
    };
    let latencies = Arc::new(Mutex::new(Vec::<u64>::with_capacity(cli.requests)));
    let sem = Arc::new(Semaphore::new(cli.concurrency));
    let wall = Instant::now();
    let mut tasks = Vec::with_capacity(cli.requests);
    for idx in 0..cli.requests {
        let sem = Arc::clone(&sem);
        let lat = Arc::clone(&latencies);
        let work = Duration::from_millis(cli.work_ms);
        let mode = cli.mode;
        let native_tailtriage_for_task = native_tailtriage.clone();
        tasks.push(tokio::spawn(async move {
            let start = Instant::now();
            if matches!(mode, Mode::Baseline | Mode::BakedInNoRequestContext) {
                let permit = sem.acquire().await.expect("semaphore closed");
                tokio::time::sleep(work).await;
                drop(permit);
            } else if matches!(mode.instrumentation(), InstrumentationKind::Native) {
                native_request(sem, work, idx, native_tailtriage_for_task).await;
            } else {
                tracing_request(sem, work, idx).await;
            }
            lat.lock()
                .await
                .push(u64::try_from(start.elapsed().as_micros()).unwrap_or(u64::MAX));
        }));
    }
    for t in tasks {
        t.await.context("request task panicked")?;
    }
    Ok((
        Arc::into_inner(latencies)
            .expect("all refs dropped")
            .into_inner(),
        wall.elapsed(),
    ))
}

async fn native_request(
    sem: Arc<Semaphore>,
    work: Duration,
    idx: usize,
    tailtriage: Option<Arc<Tailtriage>>,
) {
    let Some(tailtriage) = tailtriage else {
        return;
    };
    let request_id = format!("request-{idx}");
    let started = tailtriage.begin_request_with(
        "/runtime-cost",
        tailtriage_core::RequestOptions::new().request_id(request_id),
    );
    let request = started.handle.clone();
    let _inflight = request.inflight("runtime_cost_requests");
    let permit = request
        .queue("worker_semaphore")
        .await_on(sem.acquire())
        .await
        .expect("semaphore closed");
    request
        .stage("simulated_work")
        .await_value(tokio::time::sleep(work))
        .await;
    drop(permit);
    started.completion.finish(tailtriage_core::Outcome::Ok);
}
async fn tracing_request(sem: Arc<Semaphore>, work: Duration, idx: usize) {
    let request_id = format!("request-{idx}");
    let request_id_for_request = request_id.clone();
    async move {
        let queue_span = tracing::info_span!(
            "runtime.queue",
            tt.kind = "queue",
            tt.request_id = %request_id,
            tt.queue = "worker_semaphore",
            tt.depth_at_start = 0_u64
        );
        let permit = sem
            .acquire()
            .instrument(queue_span)
            .await
            .expect("semaphore closed");
        let stage_span = tracing::info_span!(
            "runtime.stage",
            tt.kind = "stage",
            tt.request_id = %request_id,
            tt.stage = "simulated_work",
            tt.success = true
        );
        tokio::time::sleep(work).instrument(stage_span).await;
        drop(permit);
    }
    .instrument(tracing::info_span!(
        "runtime.request",
        tt.kind = "request",
        tt.request_id = %request_id_for_request,
        tt.route = "/runtime-cost",
        tt.outcome = "ok"
    ))
    .await;
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
                let v = args.next().context("missing value for --mode")?;
                mode = Mode::parse(&v);
                if mode.is_none() {
                    bail!("invalid --mode {v}; expected baseline|baked_in_no_request_context|core_light|core_investigation|core_light_tokio_sampler|core_investigation_tokio_sampler|core_light_drop_path|core_investigation_drop_path|tracing_light|tracing_light_tokio_sampler|tracing_light_drop_path");
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
        bail!("--requests, --concurrency, and --work-ms must be > 0")
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
fn requests_per_second(n: usize, e: Duration) -> anyhow::Result<f64> {
    Ok(u64::try_from(n)?.to_string().parse::<f64>()? / e.as_secs_f64())
}
fn percentile_ms(sorted_us: &[u64], num: u64, den: u64) -> anyhow::Result<f64> {
    if sorted_us.is_empty() {
        return Ok(0.0);
    }
    anyhow::ensure!(den != 0, "percentile denominator must be non-zero");
    anyhow::ensure!(num <= den, "percentile numerator must be <= denominator");
    let max = sorted_us.len() - 1;
    let scaled = u128::from(u64::try_from(max)?) * u128::from(num);
    let idx = usize::try_from((scaled + (u128::from(den) / 2)) / u128::from(den))?;
    micros_to_millis_f64(sorted_us[idx])
}
fn micros_to_millis_f64(m: u64) -> anyhow::Result<f64> {
    Ok(m.to_string().parse::<f64>()? / 1_000.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test(flavor = "current_thread")]
    async fn native_finalize_reads_finalized_run_from_memory_sink() {
        let output_dir = std::env::temp_dir().join(format!(
            "tailtriage-runtime-cost-test-{}",
            std::process::id()
        ));
        let cli = Cli {
            mode: Mode::CoreLight,
            requests: 1,
            concurrency: 1,
            work_ms: 1,
            output_dir: output_dir.clone(),
        };
        std::fs::create_dir_all(&output_dir).expect("create output dir");
        let mut backend = build_backend(&cli).expect("build backend");
        let (latencies, _) = run_requests(&cli, &backend).await.expect("run requests");
        assert_eq!(latencies.len(), 1);
        let (run, artifact_path) = finalize_backend_and_write_artifact(&cli, &mut backend)
            .await
            .expect("finalize and write");
        let run = run.expect("native run should exist");
        assert!(!run.requests.is_empty());
        let artifact_path = artifact_path.expect("artifact path should exist");
        assert_eq!(
            PathBuf::from(artifact_path),
            output_dir.join("run-core_light.json")
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn baked_in_no_request_context_finalizes_and_skips_analyzer_render() {
        let output_dir = std::env::temp_dir().join(format!(
            "tailtriage-runtime-cost-no-request-test-{}",
            std::process::id()
        ));
        let cli = Cli {
            mode: Mode::BakedInNoRequestContext,
            requests: 1,
            concurrency: 1,
            work_ms: 1,
            output_dir: output_dir.clone(),
        };
        std::fs::create_dir_all(&output_dir).expect("create output dir");
        let mut backend = build_backend(&cli).expect("build backend");
        let (latencies, _) = run_requests(&cli, &backend).await.expect("run requests");
        assert_eq!(latencies.len(), 1);
        let (run, artifact_path) = finalize_backend_and_write_artifact(&cli, &mut backend)
            .await
            .expect("finalize and write");
        let run = run.expect("native run should exist");
        assert!(run.requests.is_empty());
        let artifact_path = artifact_path.expect("artifact path should exist");
        assert_eq!(
            PathBuf::from(artifact_path),
            output_dir.join("run-baked_in_no_request_context.json")
        );
        let (analyze_ms, report_render_ms) =
            measure_analysis_and_render_ms(Some(&run)).expect("skip analyze/render");
        assert!(
            analyze_ms.abs() <= f64::EPSILON,
            "expected skipped analysis timing to be zero, got {analyze_ms}"
        );
        assert!(
            report_render_ms.abs() <= f64::EPSILON,
            "expected skipped report rendering timing to be zero, got {report_render_ms}"
        );
    }

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
    fn mode_classification_works() {
        let m = Mode::TracingLightTokioSampler;
        assert_eq!(m.instrumentation(), InstrumentationKind::Tracing);
        assert!(m.uses_runtime_sampler());
        assert!(!m.uses_drop_path_limits());
    }
}
