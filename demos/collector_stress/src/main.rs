use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{bail, Context};
use serde::Serialize;
use tailtriage_core::{CaptureMode, Outcome, RequestOptions, Tailtriage};
use tailtriage_tokio::RuntimeSampler;
use tokio::sync::Mutex;

const DEFAULT_DURATION_SECS: u64 = 30;
const DEFAULT_CONCURRENCY: usize = 256;
const DEFAULT_WORK_MS: u64 = 2;
const DEFAULT_QUEUES_PER_REQUEST: usize = 3;
const DEFAULT_STAGES_PER_REQUEST: usize = 4;
const DEFAULT_INFLIGHT_CYCLES: usize = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Mode {
    Baseline,
    CoreLight,
    CoreInvestigation,
    CoreLightTokioSampler,
    CoreInvestigationTokioSampler,
}

impl Mode {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "baseline" => Some(Self::Baseline),
            "core_light" => Some(Self::CoreLight),
            "core_investigation" => Some(Self::CoreInvestigation),
            "core_light_tokio_sampler" => Some(Self::CoreLightTokioSampler),
            "core_investigation_tokio_sampler" => Some(Self::CoreInvestigationTokioSampler),
            _ => None,
        }
    }

    fn core_mode(self) -> Option<CaptureMode> {
        match self {
            Self::Baseline => None,
            Self::CoreLight | Self::CoreLightTokioSampler => Some(CaptureMode::Light),
            Self::CoreInvestigation | Self::CoreInvestigationTokioSampler => {
                Some(CaptureMode::Investigation)
            }
        }
    }

    fn uses_tokio_sampler(self) -> bool {
        matches!(
            self,
            Self::CoreLightTokioSampler | Self::CoreInvestigationTokioSampler
        )
    }
}

#[derive(Debug)]
struct Cli {
    mode: Mode,
    duration_secs: u64,
    request_limit: Option<usize>,
    concurrency: usize,
    queue_slots: usize,
    queues_per_request: usize,
    stages_per_request: usize,
    inflight_cycles_per_request: usize,
    work_duration: Duration,
    work_label: WorkLabel,
    output_dir: PathBuf,
    sampler_interval_ms: Option<u64>,
    sampler_max_runtime_snapshots: Option<usize>,
}

#[derive(Clone, Copy, Debug)]
enum WorkLabel {
    Millis(u64),
    Micros(u64),
}

#[derive(Debug, Serialize)]
struct Measurement {
    #[serde(rename = "measurement_kind")]
    kind: &'static str,
    mode: Mode,
    requests_completed: usize,
    concurrency: usize,
    run_duration_secs: f64,
    configured_duration_secs: u64,
    request_limit: Option<usize>,
    throughput_rps: f64,
    event_shape: EventShape,
    sampler_settings: SamplerSettings,
    latency: LatencySummary,
    retained_counts: RetainedCounts,
    truncation_counts: TruncationCounts,
    artifact: ArtifactSummary,
    peak_memory: PeakMemory,
    #[serde(rename = "measurement_notes")]
    notes: Vec<String>,
}

#[derive(Debug, Serialize)]
struct EventShape {
    queues_per_request: usize,
    stages_per_request: usize,
    inflight_cycles_per_request: usize,
    work_ms: Option<u64>,
    work_us: Option<u64>,
}

#[derive(Debug, Serialize)]
struct SamplerSettings {
    enabled: bool,
    inherited_mode: Option<String>,
    explicit_mode_override: Option<String>,
    resolved_mode: Option<String>,
    resolved_sampler_cadence_ms: Option<u64>,
    resolved_runtime_snapshot_retention: Option<usize>,
    cli_interval_ms_override: Option<u64>,
    cli_max_runtime_snapshots_override: Option<usize>,
}

#[derive(Debug, Serialize)]
struct LatencySummary {
    count: usize,
    p50_ms: f64,
    p95_ms: f64,
    p99_ms: f64,
    max_ms: f64,
}

#[derive(Debug, Serialize)]
struct RetainedCounts {
    requests: usize,
    stages: usize,
    queues: usize,
    inflight_snapshots: usize,
    runtime_snapshots: usize,
}

#[derive(Debug, Serialize)]
struct TruncationCounts {
    limits_hit: bool,
    dropped_requests: u64,
    dropped_stages: u64,
    dropped_queues: u64,
    dropped_inflight_snapshots: u64,
    dropped_runtime_snapshots: u64,
}

#[derive(Debug, Serialize)]
struct ArtifactSummary {
    artifact_path: Option<String>,
    artifact_size_bytes: Option<u64>,
}

#[derive(Debug, Serialize)]
struct PeakMemory {
    backend: &'static str,
    collector_peak_rss_bytes: Option<u64>,
    collector_start_rss_bytes: Option<u64>,
    collector_end_rss_bytes: Option<u64>,
    notes: Vec<String>,
}

struct Instrumentation {
    tailtriage: Option<Arc<Tailtriage>>,
    sampler: Option<RuntimeSampler>,
    artifact_path: Option<PathBuf>,
}

#[derive(Clone, Copy)]
struct LinuxProcMem {
    vm_rss_bytes: u64,
    vm_hwm_bytes: u64,
}

#[tokio::main(flavor = "multi_thread", worker_threads = 8)]
#[allow(clippy::too_many_lines)]
async fn main() -> anyhow::Result<()> {
    let cli = parse_cli()?;
    std::fs::create_dir_all(&cli.output_dir)
        .with_context(|| format!("failed to create {}", cli.output_dir.display()))?;

    let mem_start = read_linux_proc_mem();
    let instrumentation = build_instrumentation(&cli)?;

    let (mut latencies_us, elapsed, completed_requests) =
        run_requests(&cli, instrumentation.tailtriage.as_ref()).await?;

    if let Some(sampler) = instrumentation.sampler {
        sampler.shutdown().await;
    }

    let mut notes = vec![
        "Synthetic stress shape intentionally amplifies queue/stage/inflight event volume per request.".to_string(),
        "This output ranks collector pressure signals; it does not prove root cause.".to_string(),
        "Operating ranges must be validated from measured output on the current machine/workload; this run does not provide cross-machine numeric guarantees.".to_string(),
    ];

    let (retained_counts, truncation_counts, sampler_settings) =
        if let Some(tailtriage) = instrumentation.tailtriage.as_ref() {
            let snapshot = tailtriage.snapshot();
            let sampler_settings = snapshot
                .metadata
                .effective_tokio_sampler_config
                .map_or_else(
                    || SamplerSettings {
                        enabled: false,
                        inherited_mode: None,
                        explicit_mode_override: None,
                        resolved_mode: None,
                        resolved_sampler_cadence_ms: None,
                        resolved_runtime_snapshot_retention: None,
                        cli_interval_ms_override: cli.sampler_interval_ms,
                        cli_max_runtime_snapshots_override: cli.sampler_max_runtime_snapshots,
                    },
                    |cfg| SamplerSettings {
                        enabled: true,
                        inherited_mode: Some(capture_mode_label(cfg.inherited_mode).to_string()),
                        explicit_mode_override: cfg
                            .explicit_mode_override
                            .map(|mode| capture_mode_label(mode).to_string()),
                        resolved_mode: Some(capture_mode_label(cfg.resolved_mode).to_string()),
                        resolved_sampler_cadence_ms: Some(cfg.resolved_sampler_cadence_ms),
                        resolved_runtime_snapshot_retention: Some(
                            cfg.resolved_runtime_snapshot_retention,
                        ),
                        cli_interval_ms_override: cli.sampler_interval_ms,
                        cli_max_runtime_snapshots_override: cli.sampler_max_runtime_snapshots,
                    },
                );

            (
                RetainedCounts {
                    requests: snapshot.requests.len(),
                    stages: snapshot.stages.len(),
                    queues: snapshot.queues.len(),
                    inflight_snapshots: snapshot.inflight.len(),
                    runtime_snapshots: snapshot.runtime_snapshots.len(),
                },
                TruncationCounts {
                    limits_hit: snapshot.truncation.limits_hit,
                    dropped_requests: snapshot.truncation.dropped_requests,
                    dropped_stages: snapshot.truncation.dropped_stages,
                    dropped_queues: snapshot.truncation.dropped_queues,
                    dropped_inflight_snapshots: snapshot.truncation.dropped_inflight_snapshots,
                    dropped_runtime_snapshots: snapshot.truncation.dropped_runtime_snapshots,
                },
                sampler_settings,
            )
        } else {
            (
                RetainedCounts {
                    requests: 0,
                    stages: 0,
                    queues: 0,
                    inflight_snapshots: 0,
                    runtime_snapshots: 0,
                },
                TruncationCounts {
                    limits_hit: false,
                    dropped_requests: 0,
                    dropped_stages: 0,
                    dropped_queues: 0,
                    dropped_inflight_snapshots: 0,
                    dropped_runtime_snapshots: 0,
                },
                SamplerSettings {
                    enabled: false,
                    inherited_mode: None,
                    explicit_mode_override: None,
                    resolved_mode: None,
                    resolved_sampler_cadence_ms: None,
                    resolved_runtime_snapshot_retention: None,
                    cli_interval_ms_override: cli.sampler_interval_ms,
                    cli_max_runtime_snapshots_override: cli.sampler_max_runtime_snapshots,
                },
            )
        };

    if let Some(tailtriage) = instrumentation.tailtriage {
        tailtriage.shutdown()?;
    }

    let artifact = if let Some(path) = instrumentation.artifact_path {
        let artifact_size_bytes = std::fs::metadata(&path).map(|meta| meta.len()).ok();
        ArtifactSummary {
            artifact_path: Some(path.display().to_string()),
            artifact_size_bytes,
        }
    } else {
        notes.push(
            "baseline mode intentionally skips tailtriage artifact output to isolate workload baseline"
                .to_string(),
        );
        ArtifactSummary {
            artifact_path: None,
            artifact_size_bytes: None,
        }
    };

    let memory = memory_measurement(mem_start, read_linux_proc_mem());

    latencies_us.sort_unstable();
    let measurement = Measurement {
        kind: "collector_stress",
        mode: cli.mode,
        requests_completed: completed_requests,
        concurrency: cli.concurrency,
        run_duration_secs: elapsed.as_secs_f64(),
        configured_duration_secs: cli.duration_secs,
        request_limit: cli.request_limit,
        throughput_rps: requests_per_second(completed_requests, elapsed)?,
        event_shape: EventShape {
            queues_per_request: cli.queues_per_request,
            stages_per_request: cli.stages_per_request,
            inflight_cycles_per_request: cli.inflight_cycles_per_request,
            work_ms: match cli.work_label {
                WorkLabel::Millis(ms) => Some(ms),
                WorkLabel::Micros(_) => None,
            },
            work_us: match cli.work_label {
                WorkLabel::Millis(ms) => ms.checked_mul(1_000),
                WorkLabel::Micros(us) => Some(us),
            },
        },
        sampler_settings,
        latency: LatencySummary {
            count: latencies_us.len(),
            p50_ms: percentile_ms(&latencies_us, 50, 100)?,
            p95_ms: percentile_ms(&latencies_us, 95, 100)?,
            p99_ms: percentile_ms(&latencies_us, 99, 100)?,
            max_ms: latencies_us
                .last()
                .copied()
                .map_or(Ok(0.0), micros_to_millis_f64)?,
        },
        retained_counts,
        truncation_counts,
        artifact,
        peak_memory: memory,
        notes,
    };

    println!("{}", serde_json::to_string(&measurement)?);
    Ok(())
}

fn build_instrumentation(cli: &Cli) -> anyhow::Result<Instrumentation> {
    let Some(capture_mode) = cli.mode.core_mode() else {
        return Ok(Instrumentation {
            tailtriage: None,
            sampler: None,
            artifact_path: None,
        });
    };

    let artifact_path = cli
        .output_dir
        .join(format!("collector-stress-run-{:?}.json", cli.mode).to_lowercase());

    let mut builder = Tailtriage::builder("collector_stress_demo").output(artifact_path.clone());
    builder = match capture_mode {
        CaptureMode::Light => builder.light(),
        CaptureMode::Investigation => builder.investigation(),
    };

    let tailtriage = Arc::new(builder.build()?);
    let sampler = if cli.mode.uses_tokio_sampler() {
        let mut sampler_builder = RuntimeSampler::builder(Arc::clone(&tailtriage));
        if let Some(interval_ms) = cli.sampler_interval_ms {
            sampler_builder = sampler_builder.interval(Duration::from_millis(interval_ms));
        }
        if let Some(max_runtime_snapshots) = cli.sampler_max_runtime_snapshots {
            sampler_builder = sampler_builder.max_runtime_snapshots(max_runtime_snapshots);
        }
        Some(sampler_builder.start()?)
    } else {
        None
    };

    Ok(Instrumentation {
        tailtriage: Some(tailtriage),
        sampler,
        artifact_path: Some(artifact_path),
    })
}

async fn run_requests(
    cli: &Cli,
    tailtriage: Option<&Arc<Tailtriage>>,
) -> anyhow::Result<(Vec<u64>, Duration, usize)> {
    let latencies_us = Arc::new(Mutex::new(Vec::<u64>::new()));
    let queue_semaphore = Arc::new(tokio::sync::Semaphore::new(cli.queue_slots));
    let issued_requests = Arc::new(AtomicUsize::new(0));
    let completed_requests = Arc::new(AtomicUsize::new(0));

    let deadline = Instant::now() + Duration::from_secs(cli.duration_secs);
    let wall_start = Instant::now();

    let mut tasks = Vec::with_capacity(cli.concurrency);
    for _worker in 0..cli.concurrency {
        let latencies = Arc::clone(&latencies_us);
        let sem = Arc::clone(&queue_semaphore);
        let issued = Arc::clone(&issued_requests);
        let completed = Arc::clone(&completed_requests);
        let mode = cli.mode;
        let request_limit = cli.request_limit;
        let work_duration = cli.work_duration;
        let queues_per_request = cli.queues_per_request;
        let stages_per_request = cli.stages_per_request;
        let inflight_cycles = cli.inflight_cycles_per_request;
        let tailtriage = tailtriage.map(Arc::clone);

        tasks.push(tokio::spawn(async move {
            loop {
                if Instant::now() >= deadline {
                    break;
                }

                let request_idx = issued.fetch_add(1, Ordering::Relaxed);
                if request_limit.is_some_and(|max| request_idx >= max) {
                    break;
                }

                let start = Instant::now();
                match (mode, &tailtriage) {
                    (Mode::Baseline, _) => {
                        run_baseline_request(
                            work_duration,
                            &sem,
                            queues_per_request,
                            stages_per_request,
                        )
                        .await;
                    }
                    (_, Some(ts)) => {
                        run_instrumented_request(
                            ts,
                            request_idx,
                            work_duration,
                            &sem,
                            queues_per_request,
                            stages_per_request,
                            inflight_cycles,
                        )
                        .await;
                    }
                    (_, None) => unreachable!("instrumented modes require collector"),
                }

                completed.fetch_add(1, Ordering::Relaxed);
                let elapsed_us = u64::try_from(start.elapsed().as_micros()).unwrap_or(u64::MAX);
                latencies.lock().await.push(elapsed_us);
            }
        }));
    }

    for task in tasks {
        task.await.context("worker task panicked")?;
    }

    let elapsed = wall_start.elapsed();
    let latencies = Arc::into_inner(latencies_us)
        .expect("all latency refs dropped")
        .into_inner();

    Ok((
        latencies,
        elapsed,
        completed_requests.load(Ordering::Relaxed),
    ))
}

async fn run_baseline_request(
    work_duration: Duration,
    queue_semaphore: &tokio::sync::Semaphore,
    queues_per_request: usize,
    stages_per_request: usize,
) {
    for _ in 0..queues_per_request {
        if let Ok(permit) = queue_semaphore.acquire().await {
            tokio::time::sleep(work_duration).await;
            drop(permit);
        }
    }

    for _ in 0..stages_per_request {
        tokio::time::sleep(work_duration).await;
    }
}

async fn run_instrumented_request(
    tailtriage: &Arc<Tailtriage>,
    request_idx: usize,
    work_duration: Duration,
    queue_semaphore: &tokio::sync::Semaphore,
    queues_per_request: usize,
    stages_per_request: usize,
    inflight_cycles: usize,
) {
    let request_id = format!("request-{request_idx}");
    let started = tailtriage.begin_request_with(
        "/collector-stress",
        RequestOptions::new().request_id(request_id),
    );
    let request = started.handle.clone();

    for cycle in 0..inflight_cycles {
        let gauge = format!("collector_stress_inflight_{cycle}");
        let inflight_guard = request.inflight(gauge);
        tokio::task::yield_now().await;
        drop(inflight_guard);
    }

    for queue_idx in 0..queues_per_request {
        let queue_name = format!("worker_queue_{queue_idx}");
        if let Ok(permit) = request
            .queue(queue_name)
            .await_on(queue_semaphore.acquire())
            .await
        {
            tokio::time::sleep(work_duration).await;
            drop(permit);
        }
    }

    for stage_idx in 0..stages_per_request {
        let stage_name = format!("simulated_stage_{stage_idx}");
        request
            .stage(stage_name)
            .await_value(tokio::time::sleep(work_duration))
            .await;
    }

    started.completion.finish(Outcome::Ok);
}

fn memory_measurement(start: Option<LinuxProcMem>, end: Option<LinuxProcMem>) -> PeakMemory {
    match (start, end) {
        (Some(start_mem), Some(end_mem)) => PeakMemory {
            backend: "linux_proc_status",
            collector_peak_rss_bytes: Some(end_mem.vm_hwm_bytes),
            collector_start_rss_bytes: Some(start_mem.vm_rss_bytes),
            collector_end_rss_bytes: Some(end_mem.vm_rss_bytes),
            notes: vec![
                "VmRSS is point-in-time resident memory and VmHWM is process-lifetime peak RSS."
                    .to_string(),
                "If external orchestration measures memory separately, join by mode/concurrency/event_shape from this JSON output."
                    .to_string(),
            ],
        },
        _ => PeakMemory {
            backend: "unsupported",
            collector_peak_rss_bytes: None,
            collector_start_rss_bytes: None,
            collector_end_rss_bytes: None,
            notes: vec![
                "Memory measurement unavailable in-process; orchestration should join external memory stats by run metadata."
                    .to_string(),
            ],
        },
    }
}

fn read_linux_proc_mem() -> Option<LinuxProcMem> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let mut vm_rss_kib = None;
    let mut vm_hwm_kib = None;

    for line in status.lines() {
        if let Some(value) = parse_status_kib(line, "VmRSS:") {
            vm_rss_kib = Some(value);
        }
        if let Some(value) = parse_status_kib(line, "VmHWM:") {
            vm_hwm_kib = Some(value);
        }
    }

    Some(LinuxProcMem {
        vm_rss_bytes: vm_rss_kib?.saturating_mul(1024),
        vm_hwm_bytes: vm_hwm_kib?.saturating_mul(1024),
    })
}

fn parse_status_kib(line: &str, key: &str) -> Option<u64> {
    let rest = line.strip_prefix(key)?.trim();
    let mut parts = rest.split_whitespace();
    let value: u64 = parts.next()?.parse().ok()?;
    let unit = parts.next()?;
    if unit != "kB" {
        return None;
    }
    Some(value)
}

fn parse_cli() -> anyhow::Result<Cli> {
    parse_cli_from(std::env::args_os().skip(1))
}

#[allow(clippy::too_many_lines)]
fn parse_cli_from<I, T>(args: I) -> anyhow::Result<Cli>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString>,
{
    let mut mode = None;
    let mut duration_secs = DEFAULT_DURATION_SECS;
    let mut request_limit = None;
    let mut concurrency = DEFAULT_CONCURRENCY;
    let mut queue_slots = None;
    let mut queues_per_request = DEFAULT_QUEUES_PER_REQUEST;
    let mut stages_per_request = DEFAULT_STAGES_PER_REQUEST;
    let mut inflight_cycles_per_request = DEFAULT_INFLIGHT_CYCLES;
    let mut work_label = WorkLabel::Millis(DEFAULT_WORK_MS);
    let mut output_dir = PathBuf::from("demos/collector_stress/artifacts");
    let mut sampler_interval_ms = None;
    let mut sampler_max_runtime_snapshots = None;

    let mut iter = args.into_iter().map(|value| {
        let value: OsString = value.into();
        value.to_string_lossy().into_owned()
    });
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--mode" => {
                let value = iter.next().context("missing value for --mode")?;
                mode = Mode::parse(&value);
                if mode.is_none() {
                    bail!(
                        "invalid --mode {value}; expected baseline|core_light|core_investigation|core_light_tokio_sampler|core_investigation_tokio_sampler"
                    );
                }
            }
            "--duration-secs" => {
                duration_secs = iter
                    .next()
                    .context("missing value for --duration-secs")?
                    .parse()
                    .context("invalid integer for --duration-secs")?;
            }
            "--requests" | "--max-requests" => {
                request_limit = Some(
                    iter.next()
                        .context("missing value for --requests")?
                        .parse()
                        .context("invalid integer for --requests")?,
                );
            }
            "--concurrency" => {
                concurrency = iter
                    .next()
                    .context("missing value for --concurrency")?
                    .parse()
                    .context("invalid integer for --concurrency")?;
            }
            "--queue-slots" => {
                queue_slots = Some(
                    iter.next()
                        .context("missing value for --queue-slots")?
                        .parse()
                        .context("invalid integer for --queue-slots")?,
                );
            }
            "--queues-per-request" => {
                queues_per_request = iter
                    .next()
                    .context("missing value for --queues-per-request")?
                    .parse()
                    .context("invalid integer for --queues-per-request")?;
            }
            "--stages-per-request" => {
                stages_per_request = iter
                    .next()
                    .context("missing value for --stages-per-request")?
                    .parse()
                    .context("invalid integer for --stages-per-request")?;
            }
            "--inflight-cycles-per-request" | "--inflight-transitions-per-request" => {
                inflight_cycles_per_request = iter
                    .next()
                    .context("missing value for --inflight-cycles-per-request")?
                    .parse()
                    .context("invalid integer for --inflight-cycles-per-request")?;
            }
            "--work-ms" => {
                let work_ms = iter
                    .next()
                    .context("missing value for --work-ms")?
                    .parse()
                    .context("invalid integer for --work-ms")?;
                work_label = WorkLabel::Millis(work_ms);
            }
            "--work-us" => {
                let work_us = iter
                    .next()
                    .context("missing value for --work-us")?
                    .parse()
                    .context("invalid integer for --work-us")?;
                work_label = WorkLabel::Micros(work_us);
            }
            "--output-dir" => {
                output_dir = PathBuf::from(iter.next().context("missing value for --output-dir")?);
            }
            "--sampler-interval-ms" => {
                sampler_interval_ms = Some(
                    iter.next()
                        .context("missing value for --sampler-interval-ms")?
                        .parse()
                        .context("invalid integer for --sampler-interval-ms")?,
                );
            }
            "--sampler-max-runtime-snapshots" => {
                sampler_max_runtime_snapshots = Some(
                    iter.next()
                        .context("missing value for --sampler-max-runtime-snapshots")?
                        .parse()
                        .context("invalid integer for --sampler-max-runtime-snapshots")?,
                );
            }
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            _ => bail!("unknown arg: {arg}"),
        }
    }

    let mode = mode.context("--mode is required")?;
    let queue_slots = queue_slots.unwrap_or_else(|| concurrency.saturating_div(2).max(1));

    if duration_secs == 0 {
        bail!("--duration-secs must be > 0");
    }

    let work_duration = match work_label {
        WorkLabel::Millis(ms) if ms > 0 => Duration::from_millis(ms),
        WorkLabel::Micros(us) if us > 0 => Duration::from_micros(us),
        WorkLabel::Millis(_) | WorkLabel::Micros(_) => {
            bail!("--work-ms/--work-us must be > 0")
        }
    };

    if concurrency == 0
        || queue_slots == 0
        || queues_per_request == 0
        || stages_per_request == 0
        || inflight_cycles_per_request == 0
    {
        bail!(
            "--concurrency, --queue-slots, --queues-per-request, --stages-per-request, and --inflight-cycles-per-request must be > 0"
        );
    }

    if request_limit.is_some_and(|limit| limit == 0) {
        bail!("--requests must be > 0 when provided");
    }

    if sampler_interval_ms.is_some_and(|value| value == 0) {
        bail!("--sampler-interval-ms must be > 0 when provided");
    }

    if sampler_max_runtime_snapshots.is_some_and(|value| value == 0) {
        bail!("--sampler-max-runtime-snapshots must be > 0 when provided");
    }

    Ok(Cli {
        mode,
        duration_secs,
        request_limit,
        concurrency,
        queue_slots,
        queues_per_request,
        stages_per_request,
        inflight_cycles_per_request,
        work_duration,
        work_label,
        output_dir,
        sampler_interval_ms,
        sampler_max_runtime_snapshots,
    })
}

fn print_help() {
    eprintln!(
        "collector_stress --mode <baseline|core_light|core_investigation|core_light_tokio_sampler|core_investigation_tokio_sampler> [--duration-secs N|--requests N] [--concurrency N] [--work-ms N|--work-us N] [--queues-per-request N] [--stages-per-request N] [--inflight-cycles-per-request N] [--output-dir DIR] [--sampler-interval-ms N] [--sampler-max-runtime-snapshots N]"
    );
    eprintln!(
        "collector-stress note: this path is for sustained high-concurrency collector behavior and artifact growth characterization, distinct from runtime_cost overhead attribution."
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

const fn capture_mode_label(mode: CaptureMode) -> &'static str {
    match mode {
        CaptureMode::Light => "light",
        CaptureMode::Investigation => "investigation",
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_cli_from, Mode, WorkLabel};

    #[test]
    fn parse_cli_supports_required_knobs() {
        let cli = parse_cli_from([
            "--mode",
            "core_light_tokio_sampler",
            "--duration-secs",
            "12",
            "--requests",
            "150",
            "--concurrency",
            "64",
            "--work-us",
            "350",
            "--queues-per-request",
            "5",
            "--stages-per-request",
            "7",
            "--inflight-cycles-per-request",
            "9",
            "--sampler-interval-ms",
            "17",
            "--sampler-max-runtime-snapshots",
            "321",
            "--output-dir",
            "tmp/out",
        ])
        .expect("args should parse");

        assert_eq!(cli.mode, Mode::CoreLightTokioSampler);
        assert_eq!(cli.duration_secs, 12);
        assert_eq!(cli.request_limit, Some(150));
        assert_eq!(cli.concurrency, 64);
        assert_eq!(cli.queues_per_request, 5);
        assert_eq!(cli.stages_per_request, 7);
        assert_eq!(cli.inflight_cycles_per_request, 9);
        assert_eq!(cli.sampler_interval_ms, Some(17));
        assert_eq!(cli.sampler_max_runtime_snapshots, Some(321));
        assert!(matches!(cli.work_label, WorkLabel::Micros(350)));
        assert_eq!(cli.output_dir.to_string_lossy(), "tmp/out");
    }

    #[test]
    fn parse_cli_rejects_zero_requests() {
        let err = parse_cli_from(["--mode", "baseline", "--requests", "0"])
            .expect_err("zero requests must fail");
        assert!(err.to_string().contains("--requests must be > 0"));
    }

    #[test]
    fn parse_cli_accepts_legacy_aliases() {
        let cli = parse_cli_from([
            "--mode",
            "core_investigation",
            "--max-requests",
            "12",
            "--inflight-transitions-per-request",
            "4",
            "--work-ms",
            "3",
        ])
        .expect("legacy aliases should parse");

        assert_eq!(cli.request_limit, Some(12));
        assert_eq!(cli.inflight_cycles_per_request, 4);
        assert!(matches!(cli.work_label, WorkLabel::Millis(3)));
    }
}
