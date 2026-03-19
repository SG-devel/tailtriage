use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use tailscope_core::{unix_time_ms, Config, RequestMeta, RuntimeSnapshot, Tailscope};

#[derive(Clone, Copy)]
enum DemoMode {
    Baseline,
    Mitigated,
}

impl DemoMode {
    fn from_arg(value: Option<String>) -> anyhow::Result<Self> {
        match value.as_deref() {
            None | Some("baseline") | Some("before") => Ok(Self::Baseline),
            Some("mitigated") | Some("after") => Ok(Self::Mitigated),
            Some(other) => anyhow::bail!(
                "unsupported mode '{other}', expected one of: baseline, before, mitigated, after"
            ),
        }
    }
}

struct ModeSettings {
    offered_requests: u64,
    blocking_work: Duration,
    inter_arrival_pause_every: u64,
    inter_arrival_delay: Duration,
    max_blocking_threads: usize,
}

impl ModeSettings {
    fn for_mode(mode: DemoMode) -> Self {
        match mode {
            DemoMode::Baseline => Self {
                offered_requests: 250,
                blocking_work: Duration::from_millis(30),
                inter_arrival_pause_every: 8,
                inter_arrival_delay: Duration::from_millis(1),
                max_blocking_threads: 2,
            },
            DemoMode::Mitigated => Self {
                offered_requests: 250,
                blocking_work: Duration::from_millis(15),
                inter_arrival_pause_every: 2,
                inter_arrival_delay: Duration::from_millis(2),
                max_blocking_threads: 8,
            },
        }
    }
}

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let output_path = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("demos/blocking_service/artifacts/blocking-run.json"));
    let mode = DemoMode::from_arg(args.next())?;
    let settings = ModeSettings::for_mode(mode);

    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create artifact directory {}", parent.display()))?;
    }

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(settings.max_blocking_threads)
        .enable_time()
        .build()
        .context("failed to build Tokio runtime")?;

    runtime.block_on(run_demo(output_path, settings))
}

async fn run_demo(output_path: PathBuf, settings: ModeSettings) -> anyhow::Result<()> {
    let mut config = Config::new("blocking_service_demo");
    config.output_path = output_path.clone();
    let tailscope = Arc::new(Tailscope::init(config)?);

    let pending_blocking = Arc::new(AtomicU64::new(0));

    let sampler = {
        let tailscope = Arc::clone(&tailscope);
        let pending_blocking = Arc::clone(&pending_blocking);

        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_millis(5));
            for _ in 0..200 {
                ticker.tick().await;
                let pending = pending_blocking.load(Ordering::SeqCst);
                tailscope.record_runtime_snapshot(RuntimeSnapshot {
                    at_unix_ms: unix_time_ms(),
                    alive_tasks: None,
                    global_queue_depth: Some(0),
                    local_queue_depth: None,
                    blocking_queue_depth: Some(pending),
                    remote_schedule_count: None,
                });
            }
        })
    };

    let mut tasks = Vec::with_capacity(settings.offered_requests as usize);

    for request_number in 0..settings.offered_requests {
        let tailscope = Arc::clone(&tailscope);
        let pending_blocking = Arc::clone(&pending_blocking);
        let blocking_work = settings.blocking_work;

        tasks.push(tokio::spawn(async move {
            let request_id = format!("request-{request_number}");
            let meta = RequestMeta::new(request_id.clone(), "/blocking-demo");

            tailscope
                .request(meta, "ok", async {
                    let _inflight = tailscope.inflight("blocking_service_inflight");
                    let _wait = tailscope
                        .queue(request_id.clone(), "dispatch_overhead")
                        .await_on(tokio::time::sleep(Duration::from_micros(10)))
                        .await;

                    pending_blocking.fetch_add(1, Ordering::SeqCst);
                    let handle = tokio::task::spawn_blocking(move || {
                        std::thread::sleep(blocking_work);
                    });

                    tailscope
                        .stage(request_id, "spawn_blocking_path")
                        .await_on(async {
                            handle
                                .await
                                .expect("spawn_blocking workload should complete")
                        })
                        .await;
                    pending_blocking.fetch_sub(1, Ordering::SeqCst);
                })
                .await;
        }));

        if request_number % settings.inter_arrival_pause_every == 0 {
            tokio::time::sleep(settings.inter_arrival_delay).await;
        }
    }

    for task in tasks {
        task.await.context("request task panicked")?;
    }

    sampler.await.context("sampler task panicked")?;

    tailscope.flush()?;
    println!("wrote {}", output_path.display());

    Ok(())
}
