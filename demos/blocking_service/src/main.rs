use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Context;
use tailscope_core::{Config, RequestMeta, RuntimeSnapshot, Tailscope};

fn main() -> anyhow::Result<()> {
    let output_path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("demos/blocking_service/artifacts/blocking-run.json"));

    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create artifact directory {}", parent.display()))?;
    }

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(2)
        .enable_time()
        .build()
        .context("failed to build Tokio runtime")?;

    runtime.block_on(run_demo(output_path))
}

async fn run_demo(output_path: PathBuf) -> anyhow::Result<()> {
    let mut config = Config::new("blocking_service_demo");
    config.output_path = output_path.clone();
    let tailscope = Arc::new(Tailscope::init(config)?);

    let offered_requests: u64 = 250;
    let blocking_work = Duration::from_millis(30);

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

    let mut tasks = Vec::with_capacity(offered_requests as usize);

    for request_number in 0..offered_requests {
        let tailscope = Arc::clone(&tailscope);
        let pending_blocking = Arc::clone(&pending_blocking);

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

        if request_number % 8 == 0 {
            tokio::time::sleep(Duration::from_millis(1)).await;
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

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before UNIX_EPOCH")
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}
