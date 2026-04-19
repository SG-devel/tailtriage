use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{bail, Context};
use serde::Serialize;
use tailtriage_core::{CaptureLimitsOverride, CaptureMode, Tailtriage};
use tailtriage_tokio::RuntimeSampler;
use tokio::sync::{Mutex, Semaphore};

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
}

impl Mode {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "baseline" => Some(Self::Baseline),
            "baked_in_no_request_context" => Some(Self::BakedInNoRequestContext),
            "core_light" => Some(Self::CoreLight),
            "core_investigation" => Some(Self::CoreInvestigation),
            "core_light_tokio_sampler" => Some(Self::CoreLightTokioSampler),
            "core_investigation_tokio_sampler" => Some(Self::CoreInvestigationTokioSampler),
            "core_light_drop_path" => Some(Self::CoreLightDropPath),
            "core_investigation_drop_path" => Some(Self::CoreInvestigationDropPath),
            _ => None,
        }
    }

    fn core_mode(self) -> Option<CaptureMode> {
        match self {
            Self::Baseline => None,
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
            Self::CoreLightTokioSampler | Self::CoreInvestigationTokioSampler
        )
    }

    fn uses_drop_path_limits(self) -> bool {
        matches!(
            self,
            Self::CoreLightDropPath | Self::CoreInvestigationDropPath
        )
    }

    fn omits_request_context(self) -> bool {
        matches!(self, Self::BakedInNoRequestContext)
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
struct Measurement {
    mode: Mode,
    requests: usize,
    concurrency: usize,
    work_ms: u64,
    throughput_rps: f64,
    latency_p50_ms: f64,
    latency_p95_ms: f64,
    latency_p99_ms: f64,
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

struct Instrumentation {
    tailtriage: Option<Arc<Tailtriage>>,
    sampler: Option<RuntimeSampler>,
}

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() -> anyhow::Result<()> {
    let cli = parse_cli()?;
    std::fs::create_dir_all(&cli.output_dir)
        .with_context(|| format!("failed to create {}", cli.output_dir.display()))?;

    let instrumentation = build_instrumentation(&cli)?;
    let (mut latencies, elapsed) = run_requests(&cli, instrumentation.tailtriage.as_ref()).await?;

    if let Some(sampler) = instrumentation.sampler {
        sampler.shutdown().await;
    }

    let truncation = if let Some(tailtriage) = instrumentation.tailtriage.as_ref() {
        let snapshot = tailtriage.snapshot();
        let truncation = snapshot.truncation;
        Some(TruncationMeasurement {
            dropped_requests: truncation.dropped_requests,
            dropped_stages: truncation.dropped_stages,
            dropped_queues: truncation.dropped_queues,
            dropped_inflight_snapshots: truncation.dropped_inflight_snapshots,
            dropped_runtime_snapshots: truncation.dropped_runtime_snapshots,
            limits_reached: truncation.limits_hit,
        })
    } else {
        None
    };

    if let Some(tailtriage) = instrumentation.tailtriage {
        tailtriage.shutdown()?;
    }

    latencies.sort_unstable();

    let measurement = Measurement {
        mode: cli.mode,
        requests: cli.requests,
        concurrency: cli.concurrency,
        work_ms: cli.work_ms,
        throughput_rps: requests_per_second(cli.requests, elapsed)?,
        latency_p50_ms: percentile_ms(&latencies, 50, 100)?,
        latency_p95_ms: percentile_ms(&latencies, 95, 100)?,
        latency_p99_ms: percentile_ms(&latencies, 99, 100)?,
        truncation,
    };

    println!("{}", serde_json::to_string(&measurement)?);

    Ok(())
}

fn build_instrumentation(cli: &Cli) -> anyhow::Result<Instrumentation> {
    let Some(capture_mode) = cli.mode.core_mode() else {
        return Ok(Instrumentation {
            tailtriage: None,
            sampler: None,
        });
    };

    let mut builder = Tailtriage::builder("runtime_cost_demo").output(
        cli.output_dir
            .join(format!("run-{:?}.json", cli.mode).to_lowercase()),
    );
    builder = match capture_mode {
        CaptureMode::Light => builder.light(),
        CaptureMode::Investigation => builder.investigation(),
    };

    if cli.mode.uses_drop_path_limits() {
        builder = builder.capture_limits_override(CaptureLimitsOverride {
            max_requests: Some(64),
            max_stages: Some(64),
            max_queues: Some(64),
            max_inflight_snapshots: Some(64),
            max_runtime_snapshots: Some(64),
        });
    }

    let tailtriage = Arc::new(builder.build()?);
    let sampler = if cli.mode.uses_tokio_sampler() {
        Some(RuntimeSampler::builder(Arc::clone(&tailtriage)).start()?)
    } else {
        None
    };

    Ok(Instrumentation {
        tailtriage: Some(tailtriage),
        sampler,
    })
}

async fn run_requests(
    cli: &Cli,
    tailtriage: Option<&Arc<Tailtriage>>,
) -> anyhow::Result<(Vec<u64>, Duration)> {
    let latencies_us = Arc::new(Mutex::new(Vec::<u64>::with_capacity(cli.requests)));
    let semaphore = Arc::new(Semaphore::new(cli.concurrency));

    let wall_start = Instant::now();
    let mut tasks = Vec::with_capacity(cli.requests);

    for idx in 0..cli.requests {
        let sem = Arc::clone(&semaphore);
        let latencies = Arc::clone(&latencies_us);
        let mode = cli.mode;
        let work_duration = Duration::from_millis(cli.work_ms);
        let tailtriage = tailtriage.map(Arc::clone);

        tasks.push(tokio::spawn(async move {
            let start = Instant::now();

            match (mode, tailtriage) {
                (Mode::Baseline, _) => {
                    let permit = sem.acquire().await.expect("semaphore closed");
                    tokio::time::sleep(work_duration).await;
                    drop(permit);
                }
                (mode, Some(_)) if mode.omits_request_context() => {
                    let permit = sem.acquire().await.expect("semaphore closed");
                    tokio::time::sleep(work_duration).await;
                    drop(permit);
                }
                (_, Some(ts)) => {
                    let request_id = format!("request-{idx}");
                    let started = ts.begin_request_with(
                        "/runtime-cost",
                        tailtriage_core::RequestOptions::new().request_id(request_id),
                    );
                    let request = started.handle.clone();

                    {
                        let _inflight = request.inflight("runtime_cost_requests");
                        let permit = request
                            .queue("worker_semaphore")
                            .await_on(sem.acquire())
                            .await
                            .expect("semaphore closed");

                        request
                            .stage("simulated_work")
                            .await_value(tokio::time::sleep(work_duration))
                            .await;

                        drop(permit);
                    }

                    started.completion.finish(tailtriage_core::Outcome::Ok);
                }
                (_, None) => unreachable!("instrumented modes require a collector"),
            }

            let elapsed_us = u64::try_from(start.elapsed().as_micros()).unwrap_or(u64::MAX);
            latencies.lock().await.push(elapsed_us);
        }));
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
                    bail!(
                        "invalid --mode {value}; expected baseline|baked_in_no_request_context|core_light|core_investigation|core_light_tokio_sampler|core_investigation_tokio_sampler|core_light_drop_path|core_investigation_drop_path"
                    );
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
    eprintln!(
        "runtime_cost --mode <baseline|baked_in_no_request_context|core_light|core_investigation|core_light_tokio_sampler|core_investigation_tokio_sampler|core_light_drop_path|core_investigation_drop_path> [--requests N] [--concurrency N] [--work-ms N] [--output-dir DIR]"
    );
    eprintln!(
        "mode semantics: baked_in_no_request_context starts tailtriage but skips request-context instrumentation; core_* adds request-context instrumentation; *_tokio_sampler additionally starts RuntimeSampler; *_drop_path intentionally hits capture limits."
    );
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

    anyhow::ensure!(denominator != 0, "percentile denominator must be non-zero");
    anyhow::ensure!(
        numerator <= denominator,
        "percentile numerator must be <= denominator"
    );

    let max_index = sorted_us.len() - 1;
    let max_index_u64 = u64::try_from(max_index)?;
    let scaled = u128::from(max_index_u64) * u128::from(numerator);
    let rounded = scaled + (u128::from(denominator) / 2);
    let index_u128 = rounded / u128::from(denominator);
    let index = usize::try_from(index_u128)?;

    micros_to_millis_f64(sorted_us[index])
}

fn micros_to_millis_f64(micros: u64) -> anyhow::Result<f64> {
    let micros_value = micros.to_string().parse::<f64>()?;
    Ok(micros_value / 1_000.0)
}
