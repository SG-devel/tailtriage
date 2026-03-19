use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{bail, Context};
use serde::Serialize;
use tailscope_core::{CaptureMode, Config, RequestMeta, Tailscope};
use tailscope_tokio::RuntimeSampler;
use tokio::sync::{Mutex, Semaphore};

const DEFAULT_REQUESTS: usize = 800;
const DEFAULT_CONCURRENCY: usize = 32;
const DEFAULT_WORK_MS: u64 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Mode {
    Baseline,
    Light,
    Investigation,
}

impl Mode {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "baseline" => Some(Self::Baseline),
            "light" => Some(Self::Light),
            "investigation" => Some(Self::Investigation),
            _ => None,
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
struct Measurement {
    mode: Mode,
    requests: usize,
    concurrency: usize,
    work_ms: u64,
    throughput_rps: f64,
    latency_p50_ms: f64,
    latency_p95_ms: f64,
    latency_p99_ms: f64,
}

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() -> anyhow::Result<()> {
    let cli = parse_cli()?;
    std::fs::create_dir_all(&cli.output_dir)
        .with_context(|| format!("failed to create {}", cli.output_dir.display()))?;

    let mut tailscope = None;
    let mut sampler = None;

    if cli.mode != Mode::Baseline {
        let mut config = Config::new("runtime_cost_demo");
        config.mode = match cli.mode {
            Mode::Light => CaptureMode::Light,
            Mode::Investigation => CaptureMode::Investigation,
            Mode::Baseline => CaptureMode::Light,
        };
        config.output_path = cli
            .output_dir
            .join(format!("run-{:?}.json", cli.mode).to_lowercase());

        let instance = Arc::new(Tailscope::init(config)?);

        if cli.mode == Mode::Investigation {
            sampler = Some(RuntimeSampler::start(
                Arc::clone(&instance),
                Duration::from_millis(2),
            )?);
        }

        tailscope = Some(instance);
    }

    let latencies_us = Arc::new(Mutex::new(Vec::<u64>::with_capacity(cli.requests)));
    let semaphore = Arc::new(Semaphore::new(cli.concurrency));

    let wall_start = Instant::now();
    let mut tasks = Vec::with_capacity(cli.requests);

    for idx in 0..cli.requests {
        let sem = Arc::clone(&semaphore);
        let latencies = Arc::clone(&latencies_us);
        let mode = cli.mode;
        let work_duration = Duration::from_millis(cli.work_ms);
        let tailscope = tailscope.as_ref().map(Arc::clone);

        tasks.push(tokio::spawn(async move {
            let start = Instant::now();

            match (mode, tailscope) {
                (Mode::Baseline, _) => {
                    let permit = sem.acquire().await.expect("semaphore closed");
                    tokio::time::sleep(work_duration).await;
                    drop(permit);
                }
                (_, Some(ts)) => {
                    let request_id = format!("request-{idx}");
                    let meta = RequestMeta::new(request_id.clone(), "/runtime-cost");

                    ts.request(meta, "ok", async {
                        let _inflight = ts.inflight("runtime_cost_requests");
                        let permit = ts
                            .queue(request_id.clone(), "worker_semaphore")
                            .await_on(sem.acquire())
                            .await
                            .expect("semaphore closed");

                        if mode == Mode::Investigation {
                            ts.stage(request_id.clone(), "pre_work_marker")
                                .await_on(tokio::time::sleep(Duration::from_micros(300)))
                                .await;
                        }

                        ts.stage(request_id, "simulated_work")
                            .await_on(tokio::time::sleep(work_duration))
                            .await;

                        drop(permit);
                    })
                    .await;
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

    if let Some(sampler) = sampler {
        sampler.shutdown().await;
    }

    if let Some(ts) = tailscope {
        ts.flush()?;
    }

    let mut latencies = Arc::into_inner(latencies_us)
        .expect("all task refs dropped")
        .into_inner();
    latencies.sort_unstable();

    let measurement = Measurement {
        mode: cli.mode,
        requests: cli.requests,
        concurrency: cli.concurrency,
        work_ms: cli.work_ms,
        throughput_rps: cli.requests as f64 / elapsed.as_secs_f64(),
        latency_p50_ms: percentile_ms(&latencies, 0.50),
        latency_p95_ms: percentile_ms(&latencies, 0.95),
        latency_p99_ms: percentile_ms(&latencies, 0.99),
    };

    println!("{}", serde_json::to_string(&measurement)?);

    Ok(())
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
                    bail!("invalid --mode {value}; expected baseline|light|investigation");
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
        "runtime_cost --mode <baseline|light|investigation> [--requests N] [--concurrency N] [--work-ms N] [--output-dir DIR]"
    );
}

fn percentile_ms(sorted_us: &[u64], percentile: f64) -> f64 {
    let len = sorted_us.len();
    if len == 0 {
        return 0.0;
    }

    let max_index = len - 1;
    let target = (max_index as f64 * percentile).round();
    let index = target.clamp(0.0, max_index as f64) as usize;

    sorted_us[index] as f64 / 1_000.0
}
